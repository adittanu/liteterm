use tauri::State;

use crate::store::models::{Host, HostGroup, HostInput, HOST_PROTOCOLS};
use crate::store::db;
use crate::Db;

#[tauri::command]
pub fn list_hosts(db: State<Db>) -> Result<Vec<Host>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_hosts(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_groups(db: State<Db>) -> Result<Vec<HostGroup>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_groups(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_host(db: State<Db>, input: HostInput) -> Result<Host, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    // A caller that leaves the protocol out keeps the stored one
    let stored = match (input.id, &input.protocol) {
        (Some(id), None) => db::get_host(&conn, id).map_err(|e| e.to_string())?.map(|h| h.protocol),
        _ => None,
    };
    let protocol = input.protocol.clone().or(stored).unwrap_or_else(|| "ssh".into());
    if !HOST_PROTOCOLS.contains(&protocol.as_str()) {
        return Err(format!("Unknown protocol “{protocol}”"));
    }
    if protocol == "serial" {
        if input.address.trim().is_empty() {
            return Err("Choose the serial port".into());
        }
        let baud_given = input.protocol.is_some();
        if baud_given && !input.baud_rate.is_some_and(|b| (50..=4_000_000).contains(&b)) {
            return Err("Baud rate must be 50–4000000".into());
        }
    } else if !(1..=65535).contains(&input.port) {
        // The column is an i64 but connecting casts it `as u16`, which silently
        // wrapped 99999 into 34463
        return Err("Port must be 1–65535".into());
    }
    db::upsert_host(&conn, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_host(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_host(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_group(db: State<Db>, id: Option<i64>, name: String) -> Result<HostGroup, String> {
    if name.trim().is_empty() {
        return Err("Group name is required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::upsert_group(&conn, id, name.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_group(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_group(&conn, id).map_err(|e| e.to_string())
}

/// Clear all trusted host keys for a host (after server rebuilds or a
/// false key-changed alarm). The next connect re-runs the TOFU prompt.
#[tauri::command]
pub fn forget_host_key(db: State<Db>, host_id: i64) -> Result<(), String> {
    let host = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        db::get_host(&conn, host_id)
            .map_err(|e| e.to_string())?
            .ok_or("Host not found")?
    };
    let host_tag = format!("{}:{}", host.address, host.port);
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::forget_host_keys(&conn, &host_tag).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn list_known_hosts(db: State<Db>) -> Result<Vec<crate::store::models::KnownHostRow>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_known_hosts(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_known_host_entry(db: State<Db>, host: String, key_type: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_known_host_entry(&conn, &host, &key_type).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_session_logs(db: State<Db>, limit: Option<i64>) -> Result<Vec<crate::store::models::SessionLog>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_session_logs(&conn, limit.unwrap_or(200)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_session_logs(db: State<Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::clear_session_logs(&conn).map_err(|e| e.to_string())
}
