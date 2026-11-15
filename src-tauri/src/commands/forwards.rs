use std::sync::Arc;

use tauri::State;

use crate::ssh::pool::{self, AppState};
use crate::store::models::{PortForward, FORWARD_KINDS};
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
    kind: Option<String>,
    local_port: i64,
    remote_host: String,
    remote_port: i64,
    auto_start: bool,
) -> Result<PortForward, String> {
    let kind = kind.unwrap_or_else(|| "local".into());
    let (remote_host, remote_port) = validate_forward(&name, &kind, local_port, remote_host.trim(), remote_port)?;
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::upsert_forward(&conn, id, host_id, name.trim(), &kind, local_port, &remote_host, remote_port, auto_start)
        .map_err(|e| e.to_string())
}

/// Checks a forward before it is saved, and returns the remote host and port
/// to store: a dynamic forward has neither.
fn validate_forward(
    name: &str,
    kind: &str,
    local_port: i64,
    remote_host: &str,
    remote_port: i64,
) -> Result<(String, i64), String> {
    if name.trim().is_empty() {
        return Err("Name is required".into());
    }
    if !FORWARD_KINDS.contains(&kind) {
        return Err(format!("Unknown forward type “{kind}”"));
    }
    let port_ok = |p: i64| (1..=65535).contains(&p);
    if !port_ok(local_port) {
        return Err("Ports must be 1–65535".into());
    }
    if kind == "dynamic" {
        return Ok((String::new(), 0));
    }
    if !port_ok(remote_port) {
        return Err("Ports must be 1–65535".into());
    }
    if remote_host.is_empty() {
        return Err(if kind == "remote" { "Destination host is required" } else { "Remote host is required" }.into());
    }
    Ok((remote_host.to_string(), remote_port))
}

#[cfg(test)]
mod tests {
    use super::validate_forward;

    #[test]
    fn each_kind_needs_its_own_fields() {
        assert_eq!(validate_forward("pg", "local", 5432, "db", 5432), Ok(("db".into(), 5432)));
        assert!(validate_forward("pg", "local", 5432, "", 5432).is_err());
        assert!(validate_forward("pg", "local", 0, "db", 5432).is_err());
        assert!(validate_forward("pg", "remote", 3000, "localhost", 0).is_err());
        assert_eq!(validate_forward("web", "remote", 3000, "localhost", 8080), Ok(("localhost".into(), 8080)));
        // A SOCKS proxy has no destination of its own
        assert_eq!(validate_forward("socks", "dynamic", 1080, "ignored", 99), Ok((String::new(), 0)));
        assert!(validate_forward("socks", "dynamic", 70000, "", 0).is_err());
        assert!(validate_forward("x", "sideways", 1, "a", 1).is_err());
        assert!(validate_forward("  ", "local", 1, "a", 1).is_err());
    }
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
