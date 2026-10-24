use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

use super::models::*;

pub fn open_db(app_data_dir: &Path) -> Result<Connection, rusqlite::Error> {
    std::fs::create_dir_all(app_data_dir).ok();
    let conn = Connection::open(app_data_dir.join("hosts.db"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS groups (
            id    INTEGER PRIMARY KEY AUTOINCREMENT,
            name  TEXT NOT NULL,
            sort  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS hosts (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id        INTEGER REFERENCES groups(id) ON DELETE SET NULL,
            label           TEXT NOT NULL,
            address         TEXT NOT NULL,
            port            INTEGER NOT NULL DEFAULT 22,
            username        TEXT NOT NULL,
            auth_method     TEXT NOT NULL DEFAULT 'password',
            password_saved  INTEGER NOT NULL DEFAULT 0,
            key_path        TEXT,
            sort            INTEGER NOT NULL DEFAULT 0,
            created_at      TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS snippets (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            name     TEXT NOT NULL,
            command  TEXT NOT NULL,
            sort     INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS port_forwards (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            host_id      INTEGER NOT NULL REFERENCES hosts(id) ON DELETE CASCADE,
            name         TEXT NOT NULL,
            local_port   INTEGER NOT NULL,
            remote_host  TEXT NOT NULL,
            remote_port  INTEGER NOT NULL,
            auto_start   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS known_hosts (
            host        TEXT NOT NULL,
            key_type    TEXT NOT NULL,
            public_key  TEXT NOT NULL,
            fingerprint TEXT NOT NULL DEFAULT '',
            added_at    TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (host, key_type)
        );

        CREATE TABLE IF NOT EXISTS credentials (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            name           TEXT NOT NULL,
            username       TEXT NOT NULL,
            auth_method    TEXT NOT NULL DEFAULT 'password',
            password_saved INTEGER NOT NULL DEFAULT 0,
            key_path       TEXT,
            created_at     TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at     TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        "#,
    )?;

    // hosts.credential_id — added in v1.2; ignore the error when the
    // column already exists
    let _ = conn.execute_batch(
        "ALTER TABLE hosts ADD COLUMN credential_id INTEGER REFERENCES credentials(id) ON DELETE SET NULL",
    );

    // Schema v2: known_hosts keys off (host, key_type) — servers offer
    // several host key types and OpenSSH known_hosts stores one line per
    // type. v1 stored a single key per host, which false-alarmed
    // "HOST KEY CHANGED" on healthy servers. One-time rebuild; keys are
    // re-importable from ~/.ssh/known_hosts via the import dialog.
    let version: i64 = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'schemaVersion'",
            [],
            |row| row.get(0),
        )
        .optional()?
        .and_then(|v: String| v.parse().ok())
        .unwrap_or(1);

    if version < 2 {
        conn.execute("DROP TABLE IF EXISTS known_hosts", [])?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS known_hosts (
                host        TEXT NOT NULL,
                key_type    TEXT NOT NULL,
                public_key  TEXT NOT NULL,
                fingerprint TEXT NOT NULL DEFAULT '',
                added_at    TEXT NOT NULL DEFAULT (datetime('now')),
                PRIMARY KEY (host, key_type)
            );
            INSERT INTO settings (key, value) VALUES ('schemaVersion', '2')
                ON CONFLICT(key) DO UPDATE SET value = '2';
            "#,
        )?;
    }

    Ok(())
}

// ---------- hosts ----------

fn row_to_host(row: &rusqlite::Row) -> rusqlite::Result<Host> {
    Ok(Host {
        id: row.get(0)?,
        group_id: row.get(1)?,
        label: row.get(2)?,
        address: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        auth_method: row.get(6)?,
        password_saved: row.get::<_, i64>(7)? != 0,
        has_password: false, // filled by caller via keyring check
        key_path: row.get(8)?,
        credential_id: row.get(10)?,
        credential_name: row.get(11)?,
        sort: row.get(9)?,
    })
}

const HOST_COLS: &str = "h.id, h.group_id, h.label, h.address, h.port, h.username, h.auth_method, \
    h.password_saved, h.key_path, h.sort, h.credential_id, c.name";

pub fn list_hosts(conn: &Connection) -> Result<Vec<Host>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {HOST_COLS} FROM hosts h LEFT JOIN credentials c ON c.id = h.credential_id \
         ORDER BY h.sort, h.label COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], row_to_host)?;
    let mut hosts: Vec<Host> = rows.collect::<Result<_, _>>()?;
    for h in &mut hosts {
        h.has_password = h.password_saved && keyring_get(&password_entry(h.id)).is_some();
    }
    Ok(hosts)
}

pub fn get_host(conn: &Connection, id: i64) -> Result<Option<Host>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {HOST_COLS} FROM hosts h LEFT JOIN credentials c ON c.id = h.credential_id WHERE h.id = ?1"
    ))?;
    let mut hosts = stmt
        .query_map(params![id], row_to_host)?
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(h) = hosts.first_mut() {
        h.has_password = h.password_saved && keyring_get(&password_entry(h.id)).is_some();
    }
    Ok(hosts.pop())
}

pub fn upsert_host(conn: &Connection, input: &HostInput) -> Result<Host, rusqlite::Error> {
    let save_pw = input.save_password.unwrap_or(false);

    // Optionally create a reusable credential from the inline fields
    let created_credential_id = match &input.create_credential {
        Some(cred_input) => Some(upsert_credential(conn, None, cred_input)?.id),
        None => None,
    };
    let credential_id = created_credential_id.or(input.credential_id);

    let id = match input.id {
        Some(id) => {
            conn.execute(
                "UPDATE hosts SET group_id=?2, label=?3, address=?4, port=?5, username=?6, \
                 auth_method=?7, password_saved=?8, key_path=?9, credential_id=?10, \
                 updated_at=datetime('now') WHERE id=?1",
                params![
                    id,
                    input.group_id,
                    input.label,
                    input.address,
                    input.port,
                    input.username,
                    input.auth_method,
                    save_pw as i64,
                    input.key_path,
                    credential_id,
                ],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO hosts (group_id, label, address, port, username, auth_method, password_saved, key_path, credential_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    input.group_id,
                    input.label,
                    input.address,
                    input.port,
                    input.username,
                    input.auth_method,
                    save_pw as i64,
                    input.key_path,
                    credential_id,
                ],
            )?;
            conn.last_insert_rowid()
        }
    };

    // Credentials live in the OS credential manager, never in SQLite
    if let Some(pw) = &input.password {
        if save_pw && !pw.is_empty() {
            keyring_set(&password_entry(id), pw);
        }
    }
    if let Some(pp) = &input.key_passphrase {
        if !pp.is_empty() {
            keyring_set(&keypass_entry(id), pp);
        }
    }
    if !save_pw {
        keyring_delete(&password_entry(id));
    }

    get_host(conn, id)?.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
}

pub fn delete_host(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM hosts WHERE id = ?1", params![id])?;
    keyring_delete(&password_entry(id));
    keyring_delete(&keypass_entry(id));
    Ok(())
}

// ---------- groups ----------

pub fn list_groups(conn: &Connection) -> Result<Vec<HostGroup>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT id, name, sort FROM groups ORDER BY sort, name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |row| {
        Ok(HostGroup { id: row.get(0)?, name: row.get(1)?, sort: row.get(2)? })
    })?;
    rows.collect()
}

pub fn upsert_group(conn: &Connection, id: Option<i64>, name: &str) -> Result<HostGroup, rusqlite::Error> {
    let id = match id {
        Some(id) => {
            conn.execute("UPDATE groups SET name=?2 WHERE id=?1", params![id, name])?;
            id
        }
        None => {
            conn.execute("INSERT INTO groups (name) VALUES (?1)", params![name])?;
            conn.last_insert_rowid()
        }
    };
    let mut stmt = conn.prepare("SELECT id, name, sort FROM groups WHERE id=?1")?;
    stmt.query_row(params![id], |row| {
        Ok(HostGroup { id: row.get(0)?, name: row.get(1)?, sort: row.get(2)? })
    })
}

pub fn delete_group(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------- credentials ----------

const CRED_COLS: &str = "id, name, username, auth_method, password_saved, key_path";

fn row_to_credential(row: &rusqlite::Row) -> rusqlite::Result<Credential> {
    Ok(Credential {
        id: row.get(0)?,
        name: row.get(1)?,
        username: row.get(2)?,
        auth_method: row.get(3)?,
        password_saved: row.get::<_, i64>(4)? != 0,
        has_password: false, // filled by caller via keyring check
        key_path: row.get(5)?,
    })
}

pub fn list_credentials(conn: &Connection) -> Result<Vec<Credential>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {CRED_COLS} FROM credentials ORDER BY name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], row_to_credential)?;
    let mut creds: Vec<Credential> = rows.collect::<Result<_, _>>()?;
    for c in &mut creds {
        c.has_password = c.password_saved && keyring_get(&cred_password_entry(c.id)).is_some();
    }
    Ok(creds)
}

pub fn get_credential(conn: &Connection, id: i64) -> Result<Option<Credential>, rusqlite::Error> {
    let mut stmt =
        conn.prepare(&format!("SELECT {CRED_COLS} FROM credentials WHERE id = ?1"))?;
    let mut creds = stmt
        .query_map(params![id], row_to_credential)?
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(c) = creds.first_mut() {
        c.has_password = c.password_saved && keyring_get(&cred_password_entry(c.id)).is_some();
    }
    Ok(creds.pop())
}

pub fn upsert_credential(
    conn: &Connection,
    id: Option<i64>,
    input: &CredentialInput,
) -> Result<Credential, rusqlite::Error> {
    let save_pw = input.save_password.unwrap_or(false);
    let id = match id {
        Some(id) => {
            conn.execute(
                "UPDATE credentials SET name=?2, username=?3, auth_method=?4, password_saved=?5, \
                 key_path=?6, updated_at=datetime('now') WHERE id=?1",
                params![
                    id,
                    input.name.trim(),
                    input.username.trim(),
                    input.auth_method,
                    save_pw as i64,
                    input.key_path,
                ],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO credentials (name, username, auth_method, password_saved, key_path) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    input.name.trim(),
                    input.username.trim(),
                    input.auth_method,
                    save_pw as i64,
                    input.key_path,
                ],
            )?;
            conn.last_insert_rowid()
        }
    };

    if let Some(pw) = &input.password {
        if save_pw && !pw.is_empty() {
            keyring_set(&cred_password_entry(id), pw);
        }
    }
    if let Some(pp) = &input.key_passphrase {
        if !pp.is_empty() {
            keyring_set(&cred_keypass_entry(id), pp);
        }
    }
    if !save_pw {
        keyring_delete(&cred_password_entry(id));
    }

    get_credential(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn delete_credential(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM credentials WHERE id = ?1", params![id])?;
    keyring_delete(&cred_password_entry(id));
    keyring_delete(&cred_keypass_entry(id));
    Ok(())
}

// ---------- snippets ----------

pub fn list_snippets(conn: &Connection) -> Result<Vec<Snippet>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT id, name, command, sort FROM snippets ORDER BY sort, name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |row| {
        Ok(Snippet { id: row.get(0)?, name: row.get(1)?, command: row.get(2)?, sort: row.get(3)? })
    })?;
    rows.collect()
}

pub fn upsert_snippet(conn: &Connection, id: Option<i64>, name: &str, command: &str) -> Result<Snippet, rusqlite::Error> {
    let id = match id {
        Some(id) => {
            conn.execute("UPDATE snippets SET name=?2, command=?3 WHERE id=?1", params![id, name, command])?;
            id
        }
        None => {
            conn.execute("INSERT INTO snippets (name, command) VALUES (?1, ?2)", params![name, command])?;
            conn.last_insert_rowid()
        }
    };
    let mut stmt = conn.prepare("SELECT id, name, command, sort FROM snippets WHERE id=?1")?;
    stmt.query_row(params![id], |row| {
        Ok(Snippet { id: row.get(0)?, name: row.get(1)?, command: row.get(2)?, sort: row.get(3)? })
    })
}

pub fn delete_snippet(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------- port forwards ----------

const FORWARD_COLS: &str = "id, host_id, name, local_port, remote_host, remote_port, auto_start";

pub fn list_forwards(conn: &Connection) -> Result<Vec<PortForward>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("SELECT {FORWARD_COLS} FROM port_forwards ORDER BY id"))?;
    let rows = stmt.query_map([], |row| {
        Ok(PortForward {
            id: row.get(0)?,
            host_id: row.get(1)?,
            name: row.get(2)?,
            local_port: row.get(3)?,
            remote_host: row.get(4)?,
            remote_port: row.get(5)?,
            auto_start: row.get::<_, i64>(6)? != 0,
        })
    })?;
    rows.collect()
}

pub fn get_forward(conn: &Connection, id: i64) -> Result<Option<PortForward>, rusqlite::Error> {
    let mut stmt = conn.prepare(&format!("SELECT {FORWARD_COLS} FROM port_forwards WHERE id=?1"))?;
    let mut rows = stmt
        .query_map(params![id], |row| {
            Ok(PortForward {
                id: row.get(0)?,
                host_id: row.get(1)?,
                name: row.get(2)?,
                local_port: row.get(3)?,
                remote_host: row.get(4)?,
                remote_port: row.get(5)?,
                auto_start: row.get::<_, i64>(6)? != 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.pop())
}

#[allow(clippy::too_many_arguments)]
pub fn upsert_forward(
    conn: &Connection,
    id: Option<i64>,
    host_id: i64,
    name: &str,
    local_port: i64,
    remote_host: &str,
    remote_port: i64,
    auto_start: bool,
) -> Result<PortForward, rusqlite::Error> {
    let id = match id {
        Some(id) => {
            conn.execute(
                "UPDATE port_forwards SET host_id=?2, name=?3, local_port=?4, remote_host=?5, remote_port=?6, auto_start=?7 WHERE id=?1",
                params![id, host_id, name, local_port, remote_host, remote_port, auto_start as i64],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO port_forwards (host_id, name, local_port, remote_host, remote_port, auto_start) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![host_id, name, local_port, remote_host, remote_port, auto_start as i64],
            )?;
            conn.last_insert_rowid()
        }
    };
    get_forward(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn delete_forward(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM port_forwards WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------- known hosts (TOFU) ----------

/// All trusted keys for a host — servers legitimately offer several
/// host key types (Ed25519, ECDSA, RSA), one row per type.
pub fn get_known_hosts(conn: &Connection, host: &str) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key_type, public_key FROM known_hosts WHERE host = ?1")?;
    let rows = stmt.query_map(params![host], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn remember_host_key(
    conn: &Connection,
    host: &str,
    key_type: &str,
    public_key_b64: &str,
    fingerprint: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO known_hosts (host, key_type, public_key, fingerprint) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(host, key_type) DO UPDATE SET public_key=?3, fingerprint=?4",
        params![host, key_type, public_key_b64, fingerprint],
    )?;
    Ok(())
}

pub fn forget_host_keys(conn: &Connection, host: &str) -> Result<usize, rusqlite::Error> {
    conn.execute("DELETE FROM known_hosts WHERE host = ?1", params![host])
}

// ---------- settings ----------

pub fn get_settings(conn: &Connection) -> Result<AppSettings, rusqlite::Error> {
    let mut settings = AppSettings::default();
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (k, v) = row?;
        match k.as_str() {
            "theme" => settings.theme = v,
            "terminalFontSize" => settings.terminal_font_size = v.parse().unwrap_or(13),
            "confirmDeleteFile" => settings.confirm_delete_file = v == "true",
            "autoSudoPassword" => settings.auto_sudo_password = v != "false",
            _ => {}
        }
    }
    Ok(settings)
}

pub fn set_settings(conn: &Connection, settings: &AppSettings) -> Result<(), rusqlite::Error> {
    for (k, v) in [
        ("theme", settings.theme.clone()),
        ("terminalFontSize", settings.terminal_font_size.to_string()),
        ("confirmDeleteFile", settings.confirm_delete_file.to_string()),
        ("autoSudoPassword", settings.auto_sudo_password.to_string()),
    ] {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value=?2",
            params![k, v],
        )?;
    }
    Ok(())
}

// ---------- keyring helpers ----------

pub fn password_entry(host_id: i64) -> String {
    format!("host:{host_id}:password")
}

pub fn keypass_entry(host_id: i64) -> String {
    format!("host:{host_id}:keypass")
}

pub fn cred_password_entry(cred_id: i64) -> String {
    format!("cred:{cred_id}:password")
}

pub fn cred_keypass_entry(cred_id: i64) -> String {
    format!("cred:{cred_id}:keypass")
}

pub fn keyring_get(account: &str) -> Option<String> {
    keyring::Entry::new("LiteTerm", account)
        .ok()?
        .get_password()
        .ok()
}

pub fn keyring_set(account: &str, value: &str) {
    if let Ok(entry) = keyring::Entry::new("LiteTerm", account) {
        let _ = entry.set_password(value);
    }
}

pub fn keyring_delete(account: &str) {
    if let Ok(entry) = keyring::Entry::new("LiteTerm", account) {
        let _ = entry.delete_credential();
    }
}
