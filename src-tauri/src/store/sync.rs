//! Change tracking for cloud sync (schema v3).
//!
//! Every synced row carries:
//! - `sync_id`: a UUID, the row's identity across devices. The integer `id`
//!   stays local: relations, the frontend and keyring entries keep using it,
//!   and the sync engine maps integer references to sync_ids when it
//!   serializes a row.
//! - `modified_at`: ms since epoch of the last real edit; the newer edit wins a
//!   conflict.
//!
//! `sync_outbox` holds one entry per row the server has not seen yet,
//! tombstones included. Triggers fill it, so no write path (including foreign
//! key cascades) can forget to. The rules:
//! - A local INSERT leaves `modified_at` at its default 0; the trigger then
//!   assigns the sync_id and the time, and queues the row.
//! - An UPDATE that changes a content column bumps `modified_at` and queues the
//!   row. Columns that only describe this device (`last_used_at`, the detected
//!   `os`) are not content, so connecting to a host on a device that has not
//!   synced yet can never make its stale copy "newer" than a real edit made
//!   elsewhere. Writing the same values again is not an edit either.
//! - A DELETE queues a tombstone.
//! - A write that sets `modified_at` itself is the sync engine applying a
//!   remote change: no trigger fires, so it is not echoed back. (Applying a
//!   remote delete still queues a tombstone; the engine drops that entry in
//!   the same transaction.)
//!
//! `modified_at` and outbox `changed_at` only ever grow per row (at least +1),
//! so a push can safely clear an entry only if `changed_at` is still what it
//! read: anything edited during the push stays queued.

use rusqlite::{params, Connection, OptionalExtension};

pub struct SyncedTable {
    pub table: &'static str,
    /// Item kind as stored inside the encrypted payload
    pub kind: &'static str,
    /// Columns whose change is an edit worth syncing
    content: &'static [&'static str],
}

pub const GROUPS: SyncedTable = SyncedTable {
    table: "groups",
    kind: "group",
    content: &["name", "sort"],
};

pub const HOSTS: SyncedTable = SyncedTable {
    table: "hosts",
    kind: "host",
    content: &[
        "group_id",
        "label",
        "address",
        "port",
        "username",
        "auth_method",
        "password_saved",
        "key_path",
        "credential_id",
        "jump_host_id",
        "sort",
    ],
};

pub const CREDENTIALS: SyncedTable = SyncedTable {
    table: "credentials",
    kind: "credential",
    content: &["name", "username", "auth_method", "password_saved", "key_path"],
};

pub const SNIPPETS: SyncedTable = SyncedTable {
    table: "snippets",
    kind: "snippet",
    content: &["name", "command", "sort"],
};

pub const NOTES: SyncedTable = SyncedTable {
    table: "notes",
    kind: "note",
    content: &["title", "body", "pinned", "tags", "host_id", "sort", "archived"],
};

pub const PORT_FORWARDS: SyncedTable = SyncedTable {
    table: "port_forwards",
    kind: "port_forward",
    content: &["host_id", "name", "local_port", "remote_host", "remote_port", "auto_start"],
};

pub const KNOWN_HOSTS: SyncedTable = SyncedTable {
    table: "known_hosts",
    kind: "known_host",
    content: &["host", "key_type", "public_key", "fingerprint"],
};

pub const SYNCED: [&SyncedTable; 7] = [
    &GROUPS,
    &HOSTS,
    &CREDENTIALS,
    &SNIPPETS,
    &NOTES,
    &PORT_FORWARDS,
    &KNOWN_HOSTS,
];

const NOW_MS: &str = "CAST(unixepoch('subsec') * 1000 AS INTEGER)";

/// Random (version 4) UUID, in the text form Postgres' uuid type accepts
const UUID_V4: &str = "lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4' || \
    substr(hex(randomblob(2)), 2) || '-' || substr('89ab', 1 + (abs(random()) % 4), 1) || \
    substr(hex(randomblob(2)), 2) || '-' || hex(randomblob(6)))";

/// Bring the database to schema v3 and (re)create the triggers. The triggers
/// are rebuilt on every start, so they always match the column lists above.
pub fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    let tx = conn.unchecked_transaction()?;

    let version: i64 = tx
        .query_row("SELECT value FROM settings WHERE key = 'schemaVersion'", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    if version < 3 {
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS sync_outbox (
                sync_id    TEXT PRIMARY KEY NOT NULL,
                kind       TEXT NOT NULL,
                deleted    INTEGER NOT NULL DEFAULT 0,
                changed_at INTEGER NOT NULL
            );",
        )?;
        for t in SYNCED {
            add_column_if_missing(&tx, t.table, "sync_id", "TEXT")?;
            add_column_if_missing(&tx, t.table, "modified_at", "INTEGER NOT NULL DEFAULT 0")?;
            let table = t.table;
            tx.execute_batch(&format!(
                "UPDATE {table} SET sync_id = {UUID_V4} WHERE sync_id IS NULL;
                 UPDATE {table} SET modified_at = {NOW_MS} WHERE modified_at = 0;
                 CREATE UNIQUE INDEX IF NOT EXISTS {table}_sync_id ON {table}(sync_id);"
            ))?;
            // Everything that exists today goes up on the first sync
            tx.execute(
                &format!(
                    "INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at) \
                     SELECT sync_id, ?1, 0, modified_at FROM {table} WHERE true \
                     ON CONFLICT(sync_id) DO NOTHING"
                ),
                params![t.kind],
            )?;
        }
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('schemaVersion', '3') \
             ON CONFLICT(key) DO UPDATE SET value = '3'",
            [],
        )?;
    }

    for t in SYNCED {
        tx.execute_batch(&triggers_sql(t))?;
    }
    tx.commit()
}

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> Result<(), rusqlite::Error> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        params![table, column],
        |row| row.get(0),
    )?;
    if !exists {
        conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}

/// Queue a row of `table` (by rowid) for the next push
fn enqueue_sql(t: &SyncedTable, rowid: &str) -> String {
    let (table, kind) = (t.table, t.kind);
    format!(
        "INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at) \
         SELECT sync_id, '{kind}', 0, modified_at FROM {table} WHERE rowid = {rowid} \
         ON CONFLICT(sync_id) DO UPDATE SET deleted = 0, changed_at = excluded.changed_at"
    )
}

fn triggers_sql(t: &SyncedTable) -> String {
    let (table, kind) = (t.table, t.kind);
    let changed = t
        .content
        .iter()
        .map(|c| format!("NEW.{c} IS NOT OLD.{c}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let enqueue = enqueue_sql(t, "NEW.rowid");
    format!(
        "DROP TRIGGER IF EXISTS {table}_sync_insert;
         DROP TRIGGER IF EXISTS {table}_sync_update;
         DROP TRIGGER IF EXISTS {table}_sync_delete;

         CREATE TRIGGER {table}_sync_insert AFTER INSERT ON {table}
         WHEN NEW.modified_at = 0
         BEGIN
             UPDATE {table} SET sync_id = COALESCE(sync_id, {UUID_V4}), modified_at = {NOW_MS}
                 WHERE rowid = NEW.rowid;
             {enqueue};
         END;

         CREATE TRIGGER {table}_sync_update AFTER UPDATE ON {table}
         WHEN NEW.modified_at IS OLD.modified_at AND ({changed})
         BEGIN
             UPDATE {table} SET modified_at = max(OLD.modified_at + 1, {NOW_MS})
                 WHERE rowid = NEW.rowid;
             {enqueue};
         END;

         CREATE TRIGGER {table}_sync_delete AFTER DELETE ON {table}
         WHEN OLD.sync_id IS NOT NULL
         BEGIN
             INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at)
                 VALUES (OLD.sync_id, '{kind}', 1, max(OLD.modified_at + 1, {NOW_MS}))
                 ON CONFLICT(sync_id) DO UPDATE SET deleted = 1, changed_at = excluded.changed_at;
         END;"
    )
}

/// Record an edit the row itself does not show — a password or key
/// passphrase saved to the OS keyring — so the host or credential is pushed
/// again with its new secret.
pub fn mark_changed(conn: &Connection, t: &SyncedTable, id: i64) -> Result<(), rusqlite::Error> {
    let table = t.table;
    // Setting modified_at directly keeps the update trigger out of it
    conn.execute(
        &format!("UPDATE {table} SET modified_at = max(modified_at + 1, {NOW_MS}) WHERE rowid = ?1"),
        params![id],
    )?;
    conn.execute(&enqueue_sql(t, "?1"), params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db;

    // In memory, and never through upsert_host / delete_host / *_credential:
    // those read and delete real OS keyring entries (host:<id>:password)
    fn open() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        db::migrate(&conn).unwrap();
        conn
    }

    fn add_host(conn: &Connection, label: &str) -> i64 {
        conn.execute(
            "INSERT INTO hosts (label, address, port, username, auth_method) \
             VALUES (?1, 'h.example', 22, 'root', 'password')",
            params![label],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// (sync_id, modified_at) of one row
    fn stamp(conn: &Connection, table: &str, id: i64) -> (String, i64) {
        conn.query_row(
            &format!("SELECT sync_id, modified_at FROM {table} WHERE rowid = ?1"),
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// (kind, deleted, changed_at) queued for a sync_id, if any
    fn queued(conn: &Connection, sync_id: &str) -> Option<(String, bool, i64)> {
        conn.query_row(
            "SELECT kind, deleted, changed_at FROM sync_outbox WHERE sync_id = ?1",
            params![sync_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .unwrap()
    }

    fn clear_outbox(conn: &Connection) {
        conn.execute("DELETE FROM sync_outbox", []).unwrap();
    }

    fn is_uuid_v4(s: &str) -> bool {
        let parts: Vec<&str> = s.split('-').collect();
        parts.iter().map(|p| p.len()).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
            && s.chars().all(|c| c == '-' || c.is_ascii_digit() || ('a'..='f').contains(&c))
            && parts[2].starts_with('4')
            && "89ab".contains(&parts[3][..1])
    }

    #[test]
    fn every_local_insert_gets_an_identity_and_is_queued() {
        let conn = open();
        let group = db::upsert_group(&conn, None, "prod").unwrap().id;
        let host = add_host(&conn, "web");
        let snippet = db::upsert_snippet(&conn, None, "df", "df -h").unwrap().id;
        let note = db::create_note(&conn, "t", "b").unwrap().id;
        let fwd = db::upsert_forward(&conn, None, host, "pg", 5432, "localhost", 5432, false)
            .unwrap()
            .id;
        conn.execute(
            "INSERT INTO credentials (name, username) VALUES ('ops', 'alice')",
            [],
        )
        .unwrap();
        let cred = conn.last_insert_rowid();
        db::remember_host_key(&conn, "h.example", "ssh-ed25519", "AAAA", "SHA256:x").unwrap();
        let known: i64 = conn
            .query_row("SELECT rowid FROM known_hosts", [], |r| r.get(0))
            .unwrap();

        let rows = [
            ("groups", group, "group"),
            ("hosts", host, "host"),
            ("snippets", snippet, "snippet"),
            ("notes", note, "note"),
            ("port_forwards", fwd, "port_forward"),
            ("credentials", cred, "credential"),
            ("known_hosts", known, "known_host"),
        ];
        let mut ids = std::collections::HashSet::new();
        for (table, id, kind) in rows {
            let (sync_id, modified_at) = stamp(&conn, table, id);
            assert!(is_uuid_v4(&sync_id), "{table}: {sync_id}");
            assert!(modified_at > 1_700_000_000_000, "{table}: {modified_at}");
            assert_eq!(queued(&conn, &sync_id), Some((kind.into(), false, modified_at)));
            ids.insert(sync_id);
        }
        assert_eq!(ids.len(), rows.len());
    }

    #[test]
    fn only_a_real_edit_bumps_and_queues() {
        let conn = open();
        let host = add_host(&conn, "web");
        let (sync_id, created) = stamp(&conn, "hosts", host);
        clear_outbox(&conn);

        // Device-local bookkeeping is not an edit
        db::touch_host(&conn, host).unwrap();
        db::set_host_os(&conn, host, "ubuntu").unwrap();
        // Neither is saving the same values again
        conn.execute("UPDATE hosts SET label = 'web', port = 22 WHERE id = ?1", params![host])
            .unwrap();
        assert_eq!(stamp(&conn, "hosts", host).1, created);
        assert_eq!(queued(&conn, &sync_id), None);

        conn.execute("UPDATE hosts SET label = 'web-1' WHERE id = ?1", params![host])
            .unwrap();
        let (_, edited) = stamp(&conn, "hosts", host);
        assert!(edited > created);
        assert_eq!(queued(&conn, &sync_id), Some(("host".into(), false, edited)));

        // NULL -> value counts as a change
        let group = db::upsert_group(&conn, None, "prod").unwrap().id;
        conn.execute("UPDATE hosts SET group_id = ?2 WHERE id = ?1", params![host, group])
            .unwrap();
        assert!(stamp(&conn, "hosts", host).1 > edited);
    }

    #[test]
    fn reordering_notes_queues_only_the_ones_that_moved() {
        let conn = open();
        // create_note puts each new note on top: c, b, a
        let a = db::create_note(&conn, "a", "").unwrap().id;
        let b = db::create_note(&conn, "b", "").unwrap().id;
        let c = db::create_note(&conn, "c", "").unwrap().id;
        db::reorder_notes(&conn, &[c, b, a]).unwrap();
        clear_outbox(&conn);

        db::reorder_notes(&conn, &[c, a, b]).unwrap();
        let moved: Vec<String> = conn
            .prepare("SELECT n.title FROM sync_outbox o JOIN notes n ON n.sync_id = o.sync_id ORDER BY n.title")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(moved, ["a", "b"]);
    }

    #[test]
    fn deletes_leave_tombstones_including_cascades() {
        let conn = open();
        let group = db::upsert_group(&conn, None, "prod").unwrap().id;
        let host = add_host(&conn, "web");
        conn.execute("UPDATE hosts SET group_id = ?2 WHERE id = ?1", params![host, group])
            .unwrap();
        let fwd = db::upsert_forward(&conn, None, host, "pg", 5432, "localhost", 5432, false)
            .unwrap()
            .id;
        let note = db::create_note(&conn, "runbook", "").unwrap().id;
        db::set_note_host(&conn, note, Some(host)).unwrap();
        let (host_sid, host_mod) = stamp(&conn, "hosts", host);
        let (fwd_sid, _) = stamp(&conn, "port_forwards", fwd);
        let (note_sid, note_mod) = stamp(&conn, "notes", note);
        clear_outbox(&conn);

        conn.execute("DELETE FROM hosts WHERE id = ?1", params![host]).unwrap();

        let (kind, deleted, at) = queued(&conn, &host_sid).unwrap();
        assert_eq!((kind.as_str(), deleted), ("host", true));
        assert!(at > host_mod);
        // ON DELETE CASCADE removed the forward: it needs a tombstone too
        assert_eq!(queued(&conn, &fwd_sid).map(|q| (q.0, q.1)), Some(("port_forward".into(), true)));
        // ON DELETE SET NULL unlinked the note: an edit
        let (_, unlinked) = stamp(&conn, "notes", note);
        assert!(unlinked > note_mod);
        assert_eq!(queued(&conn, &note_sid), Some(("note".into(), false, unlinked)));

        db::remember_host_key(&conn, "h.example", "ssh-ed25519", "AAAA", "SHA256:x").unwrap();
        let (kh_sid, _) = stamp(&conn, "known_hosts", 1);
        db::forget_host_keys(&conn, "h.example").unwrap();
        assert_eq!(queued(&conn, &kh_sid).map(|q| q.1), Some(true));
    }

    #[test]
    fn a_tombstone_for_a_row_still_queued_replaces_the_edit() {
        let conn = open();
        let snippet = db::upsert_snippet(&conn, None, "df", "df -h").unwrap().id;
        let (sid, _) = stamp(&conn, "snippets", snippet);
        db::delete_snippet(&conn, snippet).unwrap();
        assert_eq!(queued(&conn, &sid).map(|q| q.1), Some(true));
        let n: i64 = conn
            .query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn writes_that_set_modified_at_are_not_echoed() {
        let conn = open();
        let host = add_host(&conn, "web");
        let (sync_id, _) = stamp(&conn, "hosts", host);
        clear_outbox(&conn);

        // The sync engine applying a newer remote edit
        conn.execute(
            "UPDATE hosts SET label = 'from-laptop', modified_at = 9000000000000 WHERE id = ?1",
            params![host],
        )
        .unwrap();
        assert_eq!(stamp(&conn, "hosts", host), (sync_id.clone(), 9_000_000_000_000));
        assert_eq!(queued(&conn, &sync_id), None);

        // ... and a remote insert, which brings its own identity
        let remote = "0b6f5f53-1111-4222-8333-444455556666";
        conn.execute(
            "INSERT INTO snippets (name, command, sync_id, modified_at) VALUES ('up', 'uptime', ?1, 42)",
            params![remote],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        assert_eq!(stamp(&conn, "snippets", id), (remote.into(), 42));
        assert_eq!(queued(&conn, remote), None);
    }

    #[test]
    fn mark_changed_queues_a_row_whose_columns_did_not_change() {
        let conn = open();
        let host = add_host(&conn, "web");
        let (sync_id, before) = stamp(&conn, "hosts", host);
        clear_outbox(&conn);

        mark_changed(&conn, &HOSTS, host).unwrap();
        let (_, after) = stamp(&conn, "hosts", host);
        assert!(after > before);
        assert_eq!(queued(&conn, &sync_id), Some(("host".into(), false, after)));
    }

    #[test]
    fn upgrading_a_v2_database_gives_every_row_an_identity_once() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        db::migrate_v2(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO groups (name) VALUES ('a'), ('b');
             INSERT INTO hosts (label, address, port, username) VALUES ('x', 'x', 22, 'u'), ('y', 'y', 22, 'u');
             INSERT INTO snippets (name, command) VALUES ('s', 'ls');
             INSERT INTO known_hosts (host, key_type, public_key) VALUES ('x', 'ssh-rsa', 'AAAA');",
        )
        .unwrap();

        db::migrate(&conn).unwrap();
        let ids = || -> Vec<String> {
            let mut out = Vec::new();
            for t in SYNCED {
                let mut stmt = conn
                    .prepare(&format!("SELECT sync_id FROM {} ORDER BY rowid", t.table))
                    .unwrap();
                let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
                out.extend(rows.map(Result::unwrap));
            }
            out
        };
        let first = ids();
        assert_eq!(first.len(), 6);
        assert!(first.iter().all(|s| is_uuid_v4(s)));
        assert_eq!(first.iter().collect::<std::collections::HashSet<_>>().len(), 6);
        let queued_count: i64 = conn
            .query_row("SELECT count(*) FROM sync_outbox WHERE deleted = 0", [], |r| r.get(0))
            .unwrap();
        assert_eq!(queued_count, 6);
        let version: String = conn
            .query_row("SELECT value FROM settings WHERE key = 'schemaVersion'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, "3");

        // The next start changes nothing
        db::migrate(&conn).unwrap();
        assert_eq!(ids(), first);
        let again: i64 = conn
            .query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(again, 6);
    }
}
