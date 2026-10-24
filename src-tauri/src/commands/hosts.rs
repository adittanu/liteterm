use tauri::State;

use crate::store::models::{Host, HostGroup, HostInput};
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
