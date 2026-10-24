use std::sync::Arc;

use tauri::State;

use crate::ssh::pool::{self, AppState};
use crate::store::models::PortForward;
use crate::store::db;
use crate::Db;

#[tauri::command]
pub fn list_forwards(db: State<Db>) -> Result<Vec<PortForward>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_forwards(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_active_forwards(state: State<'_, Arc<AppState>>) -> Result<Vec<i64>, String> {
    Ok(pool::list_active_forwards(state.inner()))
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // mirrors the port_forwards table columns
pub fn upsert_forward(
    db: State<Db>,
    id: Option<i64>,
    host_id: i64,
    name: String,
    local_port: i64,
    remote_host: String,
    remote_port: i64,
    auto_start: bool,
) -> Result<PortForward, String> {
    if name.trim().is_empty() {
        return Err("Name is required".into());
    }
    if !(1..=65535).contains(&local_port) || !(1..=65535).contains(&remote_port) {
        return Err("Ports must be 1–65535".into());
    }
    if remote_host.trim().is_empty() {
        return Err("Remote host is required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::upsert_forward(&conn, id, host_id, name.trim(), local_port, remote_host.trim(), remote_port, auto_start)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_forward(state: State<'_, Arc<AppState>>, db: State<'_, Db>, id: i64) -> Result<(), String> {
    pool::forward_stop(state.inner().clone(), id).await;
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_forward(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn forward_start(state: State<'_, Arc<AppState>>, db: State<'_, Db>, id: i64) -> Result<(), String> {
    let forward = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        db::get_forward(&conn, id)
            .map_err(|e| e.to_string())?
            .ok_or("Forward not found")?
    };
    pool::forward_start(state.inner().clone(), forward).await
}

#[tauri::command]
pub async fn forward_stop(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    pool::forward_stop(state.inner().clone(), id).await;
    Ok(())
}
