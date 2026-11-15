//! Which vault an item is in, who may change it, and moving items between
//! vaults. A row's vault is its `team_id` (NULL: the personal vault); see
//! sync.rs for how that steers sync.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use super::db;
use super::models::Snippet;
use crate::cloud::items::Secrets;

/// SQL condition on a teamed table: rows this account may change. The
/// personal vault always; a team's unless it only views it.
pub const WRITABLE: &str = "(team_id IS NULL OR team_id IN (SELECT id FROM teams WHERE role <> 'viewer'))";

pub const READ_ONLY: &str = "You can only view this team's items.";

/// Refuse a change to `team_id`'s vault this account can't make. The server
/// would refuse a viewer's push anyway; this keeps the edit from showing here
/// and quietly never reaching the team.
pub fn check(conn: &Connection, team_id: Option<&str>) -> Result<(), String> {
    let Some(team) = team_id else { return Ok(()) };
    let role: Option<String> = conn
        .query_row("SELECT role FROM teams WHERE id = ?1", [team], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match role.as_deref() {
        None => Err("This computer isn't in that team.".into()),
        Some("viewer") => Err(READ_ONLY.into()),
        Some(_) => Ok(()),
    }
}

/// [`check`] for the vault of an existing row. A row that isn't there passes:
/// the change itself then does nothing, as before.
pub fn check_row(conn: &Connection, table: Teamed, id: i64) -> Result<(), String> {
    let team: Option<Option<String>> = conn
        .query_row(&format!("SELECT team_id FROM {} WHERE id = ?1", table.name()), [id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    match team {
        Some(team) => check(conn, team.as_deref()),
        None => Ok(()),
    }
}

/// The tables whose rows can be in a team vault
#[derive(Clone, Copy)]
pub enum Teamed {
    Hosts,
    Groups,
    Credentials,
    Snippets,
    Notes,
    PortForwards,
}

impl Teamed {
    fn name(self) -> &'static str {
        match self {
            Teamed::Hosts => "hosts",
            Teamed::Groups => "groups",
            Teamed::Credentials => "credentials",
            Teamed::Snippets => "snippets",
            Teamed::Notes => "notes",
            Teamed::PortForwards => "port_forwards",
        }
    }
}

/// What a move did, for the toast
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    pub hosts: usize,
    /// Credentials copied rather than moved: hosts left behind still use them
    pub credentials_copied: Vec<String>,
    /// Moved hosts whose jump host stayed behind; they now connect directly
    pub jumps_cleared: Vec<String>,
    /// Keyring entries to carry over after the commit: (to, from, keep the
    /// original)
    #[serde(skip)]
    secrets: Vec<(String, String, bool)>,
}

impl Moved {
    /// Carry the saved passwords and passphrases over to the new rows. Run it
    /// once the move has committed: a rolled-back insert could hand its id to
    /// another row.
    pub fn write_secrets(&self, secrets: &dyn Secrets) {
        for (to, from, keep) in &self.secrets {
            if let Some(value) = secrets.get(from) {
                secrets.set(to, &value);
            }
            if !keep {
                secrets.delete(from);
            }
        }
    }

    fn carry(&mut self, to: String, from: String, keep: bool) {
        self.secrets.push((to, from, keep));
    }
}

/// Move hosts (and, with `group`, a group and every host in it) into
/// `target`'s vault, with what they need there: their credentials and port
/// forwards. Rows get a new identity in the new vault; the old ones are
/// deleted, which the old vault syncs as a delete.
///
/// A host left behind can't jump through a moved one, so that is refused. A
/// moved host whose jump host stays behind connects directly from then on.
/// Notes linked to a moved host stay where they are, unlinked: what someone
/// wrote down for themselves isn't shared by moving a host.
///
/// Run inside a transaction; then [`Moved::write_secrets`] after the commit.
pub fn move_hosts(
    conn: &Connection,
    host_ids: &[i64],
    group: Option<i64>,
    target: Option<&str>,
) -> Result<Moved, String> {
    let sql = |e: rusqlite::Error| e.to_string();
    check(conn, target)?;

    let mut ids: Vec<i64> = host_ids.to_vec();
    let mut source: Option<Option<String>> = None;
    if let Some(g) = group {
        let team: Option<String> = conn
            .query_row("SELECT team_id FROM groups WHERE id = ?1", [g], |r| r.get(0))
            .optional()
            .map_err(sql)?
            .ok_or("That group is gone.")?;
        source = Some(team);
        let in_group: Vec<i64> = conn
            .prepare("SELECT id FROM hosts WHERE group_id = ?1")
            .and_then(|mut s| s.query_map([g], |r| r.get(0))?.collect())
            .map_err(sql)?;
        ids.extend(in_group);
    }
    ids.sort_unstable();
    ids.dedup();

    // Every host, and the group, must come from one vault
    struct Row {
        id: i64,
        label: String,
        team: Option<String>,
        credential: Option<i64>,
        jump: Option<i64>,
    }
    let mut rows = Vec::with_capacity(ids.len());
    for id in &ids {
        let row = conn
            .query_row(
                "SELECT label, team_id, credential_id, jump_host_id FROM hosts WHERE id = ?1",
                [id],
                |r| Ok(Row { id: *id, label: r.get(0)?, team: r.get(1)?, credential: r.get(2)?, jump: r.get(3)? }),
            )
            .optional()
            .map_err(sql)?
            .ok_or("A host you're moving is gone.")?;
        match &source {
            None => source = Some(row.team.clone()),
            Some(s) if *s != row.team => return Err("Move hosts from one vault at a time.".into()),
            Some(_) => {}
        }
        rows.push(row);
    }
    let Some(source) = source else { return Ok(Moved::default()) };
    if source.as_deref() == target {
        return Err("It's in that vault already.".into());
    }
    check(conn, source.as_deref())?;

    let placeholders = |n: usize| vec!["?"; n].join(", ");
    let left_behind: Option<String> = conn
        .query_row(
            &format!(
                "SELECT label FROM hosts WHERE jump_host_id IN ({0}) AND id NOT IN ({0}) LIMIT 1",
                placeholders(ids.len())
            ),
            rusqlite::params_from_iter(ids.iter().chain(ids.iter())),
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    if let Some(label) = left_behind {
        return Err(format!(
            "“{label}” jumps through a host you're moving. Move them together, or change its jump host first."
        ));
    }

    let mut out = Moved::default();

    let new_group = match group {
        Some(g) => {
            conn.execute(
                "INSERT INTO groups (name, sort, team_id) SELECT name, sort, ?2 FROM groups WHERE id = ?1",
                params![g, target],
            )
            .map_err(sql)?;
            Some(conn.last_insert_rowid())
        }
        None => None,
    };

    // Credentials first: the hosts point at them
    let mut creds: HashMap<i64, i64> = HashMap::new();
    for c in rows.iter().filter_map(|r| r.credential) {
        if creds.contains_key(&c) {
            continue;
        }
        conn.execute(
            "INSERT INTO credentials (name, username, auth_method, password_saved, key_path, created_at, updated_at, team_id) \
             SELECT name, username, auth_method, password_saved, key_path, created_at, updated_at, ?2 \
             FROM credentials WHERE id = ?1",
            params![c, target],
        )
        .map_err(sql)?;
        let copy = conn.last_insert_rowid();
        creds.insert(c, copy);
    }

    let mut hosts: HashMap<i64, i64> = HashMap::new();
    for r in &rows {
        conn.execute(
            "INSERT INTO hosts (group_id, label, address, port, username, auth_method, password_saved, key_path, \
             credential_id, startup_command, agent_forwarding, protocol, baud_rate, sort, os, last_used_at, \
             created_at, updated_at, team_id) \
             SELECT ?2, label, address, port, username, auth_method, password_saved, key_path, \
             ?3, startup_command, agent_forwarding, protocol, baud_rate, sort, os, last_used_at, \
             created_at, updated_at, ?4 FROM hosts WHERE id = ?1",
            params![r.id, new_group, r.credential.map(|c| creds[&c]), target],
        )
        .map_err(sql)?;
        let copy = conn.last_insert_rowid();
        hosts.insert(r.id, copy);
        out.carry(db::password_entry(copy), db::password_entry(r.id), false);
        out.carry(db::keypass_entry(copy), db::keypass_entry(r.id), false);
        // This computer's own records of the host follow it
        let (old_key, new_key) = (db::history_key(Some(r.id)), db::history_key(Some(copy)));
        conn.execute("UPDATE OR REPLACE command_history SET host_key = ?2 WHERE host_key = ?1", params![old_key, new_key])
            .map_err(sql)?;
        conn.execute(
            "UPDATE OR REPLACE command_history_imported SET host_key = ?2 WHERE host_key = ?1",
            params![old_key, new_key],
        )
        .map_err(sql)?;
        conn.execute("UPDATE session_logs SET host_id = ?2 WHERE host_id = ?1", params![r.id, copy]).map_err(sql)?;
        conn.execute(
            "INSERT INTO port_forwards (host_id, name, local_port, remote_host, remote_port, auto_start, kind, team_id) \
             SELECT ?2, name, local_port, remote_host, remote_port, auto_start, kind, ?3 \
             FROM port_forwards WHERE host_id = ?1 ORDER BY id",
            params![r.id, copy, target],
        )
        .map_err(sql)?;
    }
    // Jump hosts once every copy exists
    for r in &rows {
        match r.jump.map(|j| hosts.get(&j)) {
            Some(Some(jump)) => {
                conn.execute("UPDATE hosts SET jump_host_id = ?2 WHERE id = ?1", params![hosts[&r.id], jump])
                    .map_err(sql)?;
            }
            Some(None) => out.jumps_cleared.push(r.label.clone()),
            None => {}
        }
    }

    // The old rows go; their forwards with them (ON DELETE CASCADE)
    for r in &rows {
        conn.execute("DELETE FROM hosts WHERE id = ?1", [r.id]).map_err(sql)?;
    }
    for (&old, &copy) in &creds {
        let still_used: bool = conn
            .query_row("SELECT EXISTS (SELECT 1 FROM hosts WHERE credential_id = ?1)", [old], |r| r.get(0))
            .map_err(sql)?;
        if still_used {
            let name: String =
                conn.query_row("SELECT name FROM credentials WHERE id = ?1", [old], |r| r.get(0)).map_err(sql)?;
            out.credentials_copied.push(name);
        } else {
            conn.execute("DELETE FROM credentials WHERE id = ?1", [old]).map_err(sql)?;
        }
        // The original keeps its secrets while hosts still use it
        out.carry(db::cred_password_entry(copy), db::cred_password_entry(old), still_used);
        out.carry(db::cred_keypass_entry(copy), db::cred_keypass_entry(old), still_used);
    }
    if let Some(g) = group {
        conn.execute("DELETE FROM groups WHERE id = ?1", [g]).map_err(sql)?;
    }
    out.credentials_copied.sort();
    out.hosts = rows.len();
    Ok(out)
}

/// Move a snippet into `target`'s vault, under a new identity
pub fn move_snippet(conn: &Connection, id: i64, target: Option<&str>) -> Result<Snippet, String> {
    let sql = |e: rusqlite::Error| e.to_string();
    let source: Option<String> = conn
        .query_row("SELECT team_id FROM snippets WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(sql)?
        .ok_or("That snippet is gone.")?;
    if source.as_deref() == target {
        return Err("It's in that vault already.".into());
    }
    check(conn, source.as_deref())?;
    check(conn, target)?;
    conn.execute(
        "INSERT INTO snippets (name, command, sort, team_id) SELECT name, command, sort, ?2 FROM snippets WHERE id = ?1",
        params![id, target],
    )
    .map_err(sql)?;
    let copy = conn.last_insert_rowid();
    conn.execute("DELETE FROM snippets WHERE id = ?1", [id]).map_err(sql)?;
    db::list_snippets(conn).map_err(sql)?.into_iter().find(|s| s.id == copy).ok_or_else(|| "The snippet is gone.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const TEAM: &str = "7ea70000-0000-4000-8000-000000000001";

    // In memory, and never through upsert_host / delete_host: those touch the
    // real OS keyring
    fn open(role: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO teams (id, name, role) VALUES (?1, 'Ops', ?2)", params![TEAM, role]).unwrap();
        conn
    }

    #[derive(Default)]
    struct MemSecrets(RefCell<HashMap<String, String>>);

    impl Secrets for MemSecrets {
        fn get(&self, account: &str) -> Option<String> {
            self.0.borrow().get(account).cloned()
        }
        fn set(&self, account: &str, value: &str) {
            self.0.borrow_mut().insert(account.into(), value.into());
        }
        fn delete(&self, account: &str) {
            self.0.borrow_mut().remove(account);
        }
    }

    fn host(conn: &Connection, label: &str, cred: Option<i64>, jump: Option<i64>, group: Option<i64>) -> i64 {
        conn.execute(
            "INSERT INTO hosts (label, address, port, username, auth_method, password_saved, credential_id, jump_host_id, group_id) \
             VALUES (?1, ?1 || '.example', 22, 'root', 'password', 1, ?2, ?3, ?4)",
            params![label, cred, jump, group],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn cred(conn: &Connection, name: &str) -> i64 {
        conn.execute(
            "INSERT INTO credentials (name, username, auth_method, password_saved) VALUES (?1, 'deploy', 'password', 1)",
            [name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn val<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn moved(conn: &Connection, hosts: &[i64], group: Option<i64>, target: Option<&str>) -> Result<Moved, String> {
        let tx = conn.unchecked_transaction().unwrap();
        let out = move_hosts(&tx, hosts, group, target)?;
        tx.commit().unwrap();
        Ok(out)
    }

    #[test]
    fn a_host_moves_into_a_team_with_its_credential_forwards_and_passwords() {
        let conn = open("editor");
        let secrets = MemSecrets::default();
        let ops = cred(&conn, "ops");
        secrets.set(&db::cred_password_entry(ops), "cred-pw");
        let bastion = host(&conn, "bastion", None, None, None);
        let web = host(&conn, "web", Some(ops), Some(bastion), None);
        let other = host(&conn, "other", Some(ops), None, None);
        secrets.set(&db::password_entry(web), "web-pw");
        conn.execute(
            "INSERT INTO port_forwards (host_id, name, local_port, remote_host, remote_port) VALUES (?1, 'pg', 5432, 'localhost', 5432)",
            [web],
        )
        .unwrap();
        let note = db::create_note(&conn, "runbook", "", None).unwrap().id;
        db::set_note_host(&conn, note, Some(web)).unwrap();
        db::history_add(&conn, &db::history_key(Some(web)), &["uptime".into()], 1).unwrap();
        let old_sid: String = val(&conn, &format!("SELECT sync_id FROM hosts WHERE id = {web}"));
        conn.execute("DELETE FROM sync_outbox", []).unwrap();

        let out = moved(&conn, &[web], None, Some(TEAM)).unwrap();
        out.write_secrets(&secrets);

        assert_eq!(out.hosts, 1);
        assert_eq!(out.jumps_cleared, ["web"], "its jump host stayed personal");
        assert_eq!(out.credentials_copied, ["ops"], "another personal host still uses it");
        let new_web: i64 = val(&conn, "SELECT id FROM hosts WHERE label = 'web'");
        assert_ne!(new_web, web);
        assert_eq!(val::<String>(&conn, "SELECT team_id FROM hosts WHERE label = 'web'"), TEAM);
        assert_eq!(val::<Option<i64>>(&conn, "SELECT jump_host_id FROM hosts WHERE label = 'web'"), None);
        let new_cred: i64 = val(&conn, "SELECT credential_id FROM hosts WHERE label = 'web'");
        assert_eq!(val::<String>(&conn, &format!("SELECT team_id FROM credentials WHERE id = {new_cred}")), TEAM);
        assert_eq!(val::<Option<i64>>(&conn, &format!("SELECT credential_id FROM hosts WHERE id = {other}")), Some(ops));
        assert_eq!(
            val::<String>(&conn, "SELECT team_id FROM port_forwards"),
            TEAM,
            "the forward came along, and the old one went with the old host"
        );
        assert_eq!(val::<i64>(&conn, "SELECT host_id FROM port_forwards"), new_web);
        assert_eq!(val::<Option<i64>>(&conn, &format!("SELECT host_id FROM notes WHERE id = {note}")), None);
        assert_eq!(val::<Option<String>>(&conn, &format!("SELECT team_id FROM notes WHERE id = {note}")), None);
        assert_eq!(db::history_list(&conn, &db::history_key(Some(new_web))).unwrap(), ["uptime"]);

        assert_eq!(secrets.get(&db::password_entry(new_web)).as_deref(), Some("web-pw"));
        assert_eq!(secrets.get(&db::password_entry(web)), None);
        assert_eq!(secrets.get(&db::cred_password_entry(new_cred)).as_deref(), Some("cred-pw"));
        assert_eq!(secrets.get(&db::cred_password_entry(ops)).as_deref(), Some("cred-pw"), "the original is still used");

        // The team gets the new rows; the personal vault a delete for the old host
        let queued_in = |team: Option<&str>, deleted: bool| -> i64 {
            conn.query_row(
                "SELECT count(*) FROM sync_outbox WHERE team_id IS ?1 AND deleted = ?2",
                params![team, deleted],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(queued_in(Some(TEAM), false), 3, "host, credential and forward");
        assert_eq!(queued_in(Some(TEAM), true), 0);
        let gone: bool = conn
            .query_row("SELECT deleted FROM sync_outbox WHERE sync_id = ?1", [&old_sid], |r| r.get(0))
            .unwrap();
        assert!(gone);
        assert_eq!(val::<Option<String>>(&conn, &format!("SELECT team_id FROM sync_outbox WHERE sync_id = '{old_sid}'")), None);
    }

    #[test]
    fn a_group_moves_with_its_hosts_but_not_away_from_a_host_jumping_through_it() {
        let conn = open("admin");
        let secrets = MemSecrets::default();
        let prod = db::upsert_group(&conn, None, "prod", None).unwrap().id;
        let ops = cred(&conn, "ops");
        secrets.set(&db::cred_password_entry(ops), "cred-pw");
        let bastion = host(&conn, "bastion", Some(ops), None, Some(prod));
        host(&conn, "app", Some(ops), Some(bastion), Some(prod));
        let laptop = host(&conn, "laptop", None, Some(bastion), None);

        let err = moved(&conn, &[], Some(prod), Some(TEAM)).unwrap_err();
        assert!(err.contains("“laptop”"), "{err}");
        assert_eq!(val::<i64>(&conn, "SELECT count(*) FROM hosts WHERE team_id IS NOT NULL"), 0);

        conn.execute("UPDATE hosts SET jump_host_id = NULL WHERE id = ?1", [laptop]).unwrap();
        let out = moved(&conn, &[], Some(prod), Some(TEAM)).unwrap();
        out.write_secrets(&secrets);
        assert_eq!(out.hosts, 2);
        assert!(out.jumps_cleared.is_empty());
        assert!(out.credentials_copied.is_empty(), "no host left behind uses it: moved");
        assert_eq!(val::<i64>(&conn, "SELECT count(*) FROM groups"), 1);
        let group: i64 = val(&conn, &format!("SELECT id FROM groups WHERE team_id = '{TEAM}'"));
        assert_eq!(val::<i64>(&conn, &format!("SELECT count(*) FROM hosts WHERE group_id = {group}")), 2);
        let new_bastion: i64 = val(&conn, "SELECT id FROM hosts WHERE label = 'bastion'");
        assert_eq!(val::<i64>(&conn, "SELECT jump_host_id FROM hosts WHERE label = 'app'"), new_bastion);
        assert_eq!(val::<i64>(&conn, "SELECT count(*) FROM credentials"), 1);
        let new_cred: i64 = val(&conn, "SELECT id FROM credentials");
        assert_eq!(secrets.get(&db::cred_password_entry(new_cred)).as_deref(), Some("cred-pw"));
        assert_eq!(secrets.get(&db::cred_password_entry(ops)), None);

        // And back, into the personal vault
        let out = moved(&conn, &[], Some(group), None).unwrap();
        assert_eq!(out.hosts, 2);
        assert_eq!(val::<i64>(&conn, "SELECT count(*) FROM hosts WHERE team_id IS NOT NULL"), 0);
        assert_eq!(val::<i64>(&conn, "SELECT count(*) FROM credentials WHERE team_id IS NULL"), 1);
    }

    #[test]
    fn a_viewer_can_not_change_a_team_or_move_into_or_out_of_it() {
        let conn = open("viewer");
        assert_eq!(check(&conn, None), Ok(()));
        assert_eq!(check(&conn, Some(TEAM)).unwrap_err(), READ_ONLY);
        assert!(check(&conn, Some("7ea70000-0000-4000-8000-0000000000ff")).is_err(), "not a team of this computer");

        let mine = host(&conn, "mine", None, None, None);
        assert_eq!(moved(&conn, &[mine], None, Some(TEAM)).unwrap_err(), READ_ONLY);
        conn.execute(
            "INSERT INTO hosts (label, address, port, username, auth_method, team_id) VALUES ('theirs', 'x', 22, 'r', 'password', ?1)",
            [TEAM],
        )
        .unwrap();
        let theirs = conn.last_insert_rowid();
        assert_eq!(moved(&conn, &[theirs], None, None).unwrap_err(), READ_ONLY);
        assert_eq!(check_row(&conn, Teamed::Hosts, theirs).unwrap_err(), READ_ONLY);
        assert_eq!(check_row(&conn, Teamed::Hosts, mine), Ok(()));
        assert!(moved(&conn, &[mine, theirs], None, None).is_err(), "one vault at a time");

        // Reordering leaves the team's notes where the team has them
        let a = db::create_note(&conn, "a", "", None).unwrap().id;
        conn.execute("INSERT INTO notes (title, body, sort, team_id) VALUES ('t', '', 7, ?1)", [TEAM]).unwrap();
        let t = conn.last_insert_rowid();
        db::reorder_notes(&conn, &[t, a]).unwrap();
        assert_eq!(val::<i64>(&conn, &format!("SELECT sort FROM notes WHERE id = {t}")), 7);
        assert_eq!(val::<i64>(&conn, &format!("SELECT sort FROM notes WHERE id = {a}")), 1);
    }

    #[test]
    fn a_snippet_moves_under_a_new_identity() {
        let conn = open("owner");
        let s = db::upsert_snippet(&conn, None, "df", "df -h", None).unwrap();
        let old: String = val(&conn, &format!("SELECT sync_id FROM snippets WHERE id = {}", s.id));
        let m = move_snippet(&conn, s.id, Some(TEAM)).unwrap();
        assert_eq!(m.team_id.as_deref(), Some(TEAM));
        assert_eq!((m.name.as_str(), m.command.as_str()), ("df", "df -h"));
        let new: String = val(&conn, &format!("SELECT sync_id FROM snippets WHERE id = {}", m.id));
        assert_ne!(old, new);
        assert!(move_snippet(&conn, m.id, Some(TEAM)).is_err(), "already there");
    }
}
