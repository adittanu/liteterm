use rusqlite::Connection;
use tauri::State;

use crate::import::{self, ImportPreview, ImportResult, ParsedHost};
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

    // Groups first: create missing ones, map name -> id
    let mut group_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for g in db::list_groups(&conn).map_err(|e| e.to_string())? {
        group_ids.insert(g.name, g.id);
    }

    let mut host_ids = Vec::new();
    for h in &hosts {
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
                    let g = db::upsert_group(&conn, None, name).map_err(|e| e.to_string())?;
                    group_ids.insert(name.to_string(), g.id);
                    Some(g.id)
                }
            }
            None => None,
        };

        let input = HostInput {
            id: None,
            group_id,
            label: h.alias.trim().to_string(),
            address: h.hostname.trim().to_string(),
            port: h.port,
            username: h.user.clone().unwrap_or_else(|| "root".to_string()),
            auth_method: if h.identity_file.is_some() { "key" } else { "password" }.to_string(),
            password: None,
            save_password: Some(false),
            key_path: h.identity_file.clone(),
            key_passphrase: None,
            credential_id: None,
            create_credential: None,
        };
        let host = db::upsert_host(&conn, &input).map_err(|e| e.to_string())?;
        host_ids.push(host.id);
    }

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
