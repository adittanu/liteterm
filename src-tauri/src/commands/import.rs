use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::import::{self, FileImport, ImportPreview, ImportResult, ParsedHost};
use crate::store::db;
use crate::store::models::HostInput;
use crate::Db;

fn read_or_empty(path: &std::path::Path) -> String {
    // Notepad saves UTF-8 with a BOM; left in, it glues itself to the first
    // `Host` line and that whole host silently disappears from the import
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .trim_start_matches('\u{feff}')
        .to_owned()
}

/// The user an import assumes when the source names none. apply_parsed_hosts
/// stores it, so the duplicate check must compare against the same value —
/// comparing against "" made every host without a `User` line look new.
const DEFAULT_IMPORT_USER: &str = "root";

fn dedup_against_db(conn: &Connection, mut hosts: Vec<ParsedHost>) -> Vec<ParsedHost> {
    if let Ok(existing) = db::list_hosts(conn) {
        for h in &mut hosts {
            h.exists = existing.iter().any(|e| {
                e.address.eq_ignore_ascii_case(&h.hostname)
                    && e.port == h.port
                    && e.username
                        .eq_ignore_ascii_case(h.user.as_deref().unwrap_or(DEFAULT_IMPORT_USER))
            });
        }
    }
    hosts
}

/// Directory where private keys embedded in import files (Termius exports
/// inline PEM) are materialized.
fn imported_keys_dir() -> std::path::PathBuf {
    crate::paths::home_dir().unwrap_or_default().join(".ssh").join("liteterm-keys")
}

/// Write a private key that only its owner can read. On Unix a plain
/// `fs::write` would leave it world-readable under the default umask.
fn write_private_file(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(contents.as_bytes())
}

fn sanitize_file_stem(name: &str) -> String {
    // Cut by characters, not bytes: `String::truncate(48)` panics when byte 48
    // falls inside a multibyte letter, and is_alphanumeric keeps those
    let mut out: String = name
        .chars()
        .take(48)
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    if out.is_empty() {
        out.push_str("key");
    }
    out
}

/// Write an inline private key to `~/.ssh/liteterm-keys/` and return its path.
/// Reuses an existing file when the content already matches.
fn materialize_key(host: &ParsedHost) -> Result<String, String> {
    let dir = imported_keys_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let stem = sanitize_file_stem(&host.alias);
    let first = dir.join(format!("{stem}.pem"));
    if let Ok(existing) = std::fs::read_to_string(&first) {
        if existing == host.key_pem.as_deref().unwrap_or_default() {
            return Ok(first.to_string_lossy().into_owned());
        }
    }
    for n in 2..100 {
        let candidate = dir.join(format!("{stem}-{n}.pem"));
        if !candidate.exists() {
            write_private_file(&candidate, host.key_pem.as_deref().unwrap_or_default())
                .map_err(|e| format!("cannot write key: {e}"))?;
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    Err("too many duplicate key files".into())
}

/// A key beats everything; an export that says "agent" keeps that (there is no
/// secret to carry over); anything else falls back to a password.
fn import_auth_method(has_key: bool, declared: Option<&str>) -> &'static str {
    if has_key {
        "key"
    } else if declared == Some("agent") {
        "agent"
    } else {
        "password"
    }
}

/// Shared apply: create groups, then hosts. Handles inline keys and
/// (optionally) passwords that only file imports carry.
fn apply_parsed_hosts(
    conn: &Connection,
    hosts: &[ParsedHost],
    save_passwords: bool,
) -> Result<Vec<i64>, String> {
    // Groups first: create missing ones, map name -> id
    let mut group_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for g in db::list_groups(conn).map_err(|e| e.to_string())? {
        group_ids.insert(g.name, g.id);
    }

    let mut host_ids = Vec::new();
    let mut pending_jumps: Vec<(i64, String)> = Vec::new();
    for h in hosts {
        if h.alias.trim().is_empty() || h.hostname.trim().is_empty() {
            continue;
        }
        let group_id = match &h.group {
            Some(name) => {
                let name = name.trim();
                if name.is_empty() {
                    None
                } else if let Some(id) = group_ids.get(name) {
                    Some(*id)
                } else {
                    let g = db::upsert_group(conn, None, name).map_err(|e| e.to_string())?;
                    group_ids.insert(name.to_string(), g.id);
                    Some(g.id)
                }
            }
            None => None,
        };

        // Inline PEM (Termius) beats a referenced identity file
        let key_path = match (&h.key_pem, &h.identity_file) {
            (Some(_), _) => Some(materialize_key(h)?),
            (None, kf) => kf.clone().filter(|p| !p.trim().is_empty()),
        };

        let password = if save_passwords { h.password.clone() } else { None };
        let input = HostInput {
            id: None,
            group_id,
            label: h.alias.trim().to_string(),
            address: h.hostname.trim().to_string(),
            port: h.port,
            username: h.user.clone().unwrap_or_else(|| DEFAULT_IMPORT_USER.to_string()),
            auth_method: import_auth_method(key_path.is_some(), h.auth_method.as_deref()).to_string(),
            password,
            save_password: Some(save_passwords && h.password.as_deref().is_some_and(|p| !p.is_empty())),
            key_path,
            key_passphrase: h.key_passphrase.clone(),
            credential_id: None,
            create_credential: None,
            jump_host_id: None,
        };
        let host = db::upsert_host(conn, &input).map_err(|e| e.to_string())?;
        host_ids.push(host.id);
        if let Some(jump) = h.proxy_jump.as_deref() {
            pending_jumps.push((host.id, jump.to_string()));
        }
    }

    // ProxyJump second pass: point each host at its bastion, matched by
    // label (imported aliases become labels) or address among saved hosts
    if !pending_jumps.is_empty() {
        let saved = db::list_hosts(conn).map_err(|e| e.to_string())?;
        for (host_id, jump) in pending_jumps {
            let target = saved
                .iter()
                .find(|s| s.label.eq_ignore_ascii_case(&jump))
                .or_else(|| saved.iter().find(|s| s.address.eq_ignore_ascii_case(&jump)));
            if let Some(bastion) = target.filter(|b| b.id != host_id) {
                db::set_host_jump(conn, host_id, Some(bastion.id)).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(host_ids)
}

#[tauri::command]
pub fn import_ssh_preview(db: State<Db>) -> Result<ImportPreview, String> {
    let config_path = import::ssh_config_path();
    let text = read_or_empty(&config_path);
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let hosts = dedup_against_db(&conn, import::parse_ssh_config(&text));
    let known = read_or_empty(&import::known_hosts_path());
    Ok(ImportPreview {
        config_found: config_path.exists(),
        known_host_count: import::parse_known_hosts(&known).len(),
        hosts,
    })
}

#[tauri::command]
pub fn import_ssh_apply(
    db: State<Db>,
    hosts: Vec<ParsedHost>,
    import_known_hosts: bool,
) -> Result<ImportResult, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let host_ids = apply_parsed_hosts(&conn, &hosts, false)?;

    let mut known_imported = 0;
    if import_known_hosts {
        let known = read_or_empty(&import::known_hosts_path());
        for entry in import::parse_known_hosts(&known) {
            // A key the user already verified in LiteTerm stays: silently
            // replacing it with whatever the file holds would defeat the
            // "HOST KEY CHANGED" check for that host
            let trusted_already = db::get_known_hosts(&conn, &entry.host_tag)
                .map(|keys| keys.iter().any(|(key_type, _)| key_type == &entry.key_type))
                .unwrap_or(false);
            if trusted_already {
                continue;
            }
            // known_hosts carries no fingerprint; derive it from the key so the
            // Known Hosts page does not show "No fingerprint"
            let fingerprint = russh::keys::parse_public_key_base64(&entry.public_key)
                .map(|key| key.fingerprint())
                .unwrap_or_default();
            if db::remember_host_key(
                &conn,
                &entry.host_tag,
                &entry.key_type,
                &entry.public_key,
                &fingerprint,
            )
            .is_ok()
            {
                known_imported += 1;
            }
        }
    }

    Ok(ImportResult { host_ids, known_imported })
}

#[derive(Debug, Serialize)]
pub struct FileImportPreview {
    pub format: &'static str,
    pub hosts: Vec<ParsedHost>,
}

#[tauri::command]
pub fn import_file_preview(db: State<Db>, path: String) -> Result<FileImportPreview, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read file: {e}"))?;
    let FileImport { format, hosts } = import::parse_import_text(&text)?;
    let conn = db.0.lock().map_err(|_| "database busy")?;
    Ok(FileImportPreview {
        format,
        hosts: dedup_against_db(&conn, hosts),
    })
}

#[tauri::command]
pub fn import_file_apply(
    db: State<Db>,
    hosts: Vec<ParsedHost>,
    save_passwords: bool,
) -> Result<ImportResult, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let host_ids = apply_parsed_hosts(&conn, &hosts, save_passwords)?;
    Ok(ImportResult { host_ids, known_imported: 0 })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub host_count: usize,
    pub group_count: usize,
    pub path: String,
}

/// Export every host and group to a LiteTerm JSON file. Passwords are only
/// included when explicitly requested; key files are referenced by path,
/// never copied into the export.
#[tauri::command]
pub fn export_hosts(
    db: State<Db>,
    path: String,
    include_secrets: bool,
) -> Result<ExportResult, String> {
    let (groups, hosts, creds) = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        (
            db::list_groups(&conn).map_err(|e| e.to_string())?,
            db::list_hosts(&conn).map_err(|e| e.to_string())?,
            db::list_credentials(&conn).unwrap_or_default(),
        )
    };

    let cred_by_id: std::collections::HashMap<i64, _> =
        creds.into_iter().map(|c| (c.id, c)).collect();

    let mut out_hosts = Vec::with_capacity(hosts.len());
    for h in &hosts {
        // Resolve a linked credential so the export stays standalone
        let (username, auth_method, key_path, secret) = match h.credential_id.and_then(|id| cred_by_id.get(&id)) {
            Some(c) => (
                c.username.clone(),
                c.auth_method.clone(),
                c.key_path.clone(),
                if include_secrets && c.password_saved {
                    db::keyring_get(&db::cred_password_entry(c.id))
                } else {
                    None
                },
            ),
            None => (
                h.username.clone(),
                h.auth_method.clone(),
                h.key_path.clone(),
                if include_secrets && h.password_saved {
                    db::keyring_get(&db::password_entry(h.id))
                } else {
                    None
                },
            ),
        };
        let group_name = h
            .group_id
            .and_then(|gid| groups.iter().find(|g| g.id == gid))
            .map(|g| g.name.clone());

        let mut obj = serde_json::json!({
            "label": h.label,
            "address": h.address,
            "port": h.port,
            "username": username,
            "authMethod": auth_method,
        });
        if let Some(name) = group_name {
            obj["group"] = serde_json::json!(name);
        }
        if let Some(p) = key_path {
            obj["keyPath"] = serde_json::json!(p);
        }
        if let Some(pw) = secret {
            obj["password"] = serde_json::json!(pw);
        }
        if let Some(os) = &h.os {
            obj["os"] = serde_json::json!(os);
        }
        out_hosts.push(obj);
    }

    let group_count = groups.len();
    let doc = serde_json::json!({
        "app": "liteterm",
        "version": 1,
        "exportedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default(),
        "groups": groups.iter().map(|g| serde_json::json!({ "name": g.name })).collect::<Vec<_>>(),
        "hosts": out_hosts,
    });

    let mut path = std::path::PathBuf::from(&path);
    if path.extension().is_none() {
        path.set_extension("json");
    }
    let pretty = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    std::fs::write(&path, pretty + "\n").map_err(|e| format!("cannot write {}: {e}", path.display()))?;

    Ok(ExportResult {
        host_count: hosts.len(),
        group_count,
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_stem_is_never_cut_inside_a_multibyte_letter() {
        // 47 ASCII bytes, then 'é' (2 bytes): byte 48 is mid-letter, which made
        // the old byte-based truncate(48) panic
        let alias = format!("{}é-and-more", "a".repeat(47));
        let stem = sanitize_file_stem(&alias);
        assert_eq!(stem.chars().count(), 48);
        assert!(stem.ends_with('é'));

        assert_eq!(sanitize_file_stem("Серверы-продакшн"), "Серверы-продакшн");
        assert_eq!(sanitize_file_stem("web 01/prod"), "web-01-prod");
        assert_eq!(sanitize_file_stem(""), "key");
    }

    #[test]
    fn auth_method_follows_key_then_declared_agent_then_password() {
        assert_eq!(import_auth_method(true, Some("agent")), "key");
        assert_eq!(import_auth_method(false, Some("agent")), "agent");
        assert_eq!(import_auth_method(false, Some("password")), "password");
        assert_eq!(import_auth_method(false, None), "password");
    }

    #[test]
    fn a_host_imported_without_a_user_is_recognised_afterwards() {
        let dir = std::env::temp_dir().join(format!("liteterm-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let conn = db::open_db(&dir).unwrap();
        // password_saved stays 0, so list_hosts never reads the real keyring
        conn.execute(
            "INSERT INTO hosts (label, address, port, username, auth_method) \
             VALUES ('nas', '192.168.1.10', 22, 'root', 'password')",
            [],
        )
        .unwrap();

        // No `User` line: apply stores it as root, so it must match that row
        let parsed = ParsedHost {
            alias: "nas".into(),
            hostname: "192.168.1.10".into(),
            ..Default::default()
        };
        assert!(dedup_against_db(&conn, vec![parsed])[0].exists);

        let other = ParsedHost {
            hostname: "192.168.1.11".into(),
            ..Default::default()
        };
        assert!(!dedup_against_db(&conn, vec![other])[0].exists);

        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
