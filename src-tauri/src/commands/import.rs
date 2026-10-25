use rusqlite::Connection;
use serde::Serialize;
use tauri::State;

use crate::import::{self, FileImport, ImportPreview, ImportResult, ParsedHost};
use crate::store::db;
use crate::store::models::HostInput;
use crate::Db;

fn read_or_empty(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn dedup_against_db(conn: &Connection, mut hosts: Vec<ParsedHost>) -> Vec<ParsedHost> {
    if let Ok(existing) = db::list_hosts(conn) {
        for h in &mut hosts {
            h.exists = existing.iter().any(|e| {
                e.address.eq_ignore_ascii_case(&h.hostname)
                    && e.port == h.port
                    && e.username.eq_ignore_ascii_case(h.user.as_deref().unwrap_or(""))
            });
        }
    }
    hosts
}

/// Directory where private keys embedded in import files (Termius exports
/// inline PEM) are materialized.
fn imported_keys_dir() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    std::path::Path::new(&home).join(".ssh").join("liteterm-keys")
}

fn sanitize_file_stem(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' })
        .collect();
    out.truncate(48);
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
            std::fs::write(&candidate, host.key_pem.as_deref().unwrap_or_default())
                .map_err(|e| format!("cannot write key: {e}"))?;
            return Ok(candidate.to_string_lossy().into_owned());
        }
    }
    Err("too many duplicate key files".into())
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
            username: h.user.clone().unwrap_or_else(|| "root".to_string()),
            auth_method: if key_path.is_some() { "key" } else { "password" }.to_string(),
            password,
            save_password: Some(save_passwords && h.password.as_deref().is_some_and(|p| !p.is_empty())),
            key_path,
            key_passphrase: h.key_passphrase.clone(),
            credential_id: None,
            create_credential: None,
        };
        let host = db::upsert_host(conn, &input).map_err(|e| e.to_string())?;
        host_ids.push(host.id);
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
            if db::remember_host_key(&conn, &entry.host_tag, &entry.key_type, &entry.public_key, "")
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
