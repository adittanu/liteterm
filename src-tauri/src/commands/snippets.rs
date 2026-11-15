use tauri::State;

use crate::store::models::Snippet;
use crate::store::db;
use crate::store::vaults::{self, Teamed};
use crate::Db;

#[tauri::command]
pub fn list_snippets(db: State<Db>) -> Result<Vec<Snippet>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_snippets(&conn).map_err(|e| e.to_string())
}

/// `team_id` is the vault a new snippet goes in (None: personal)
#[tauri::command]
pub fn upsert_snippet(
    db: State<Db>,
    id: Option<i64>,
    name: String,
    command: String,
    team_id: Option<String>,
) -> Result<Snippet, String> {
    if name.trim().is_empty() || command.trim().is_empty() {
        return Err("Name and command are required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    match id {
        Some(id) => vaults::check_row(&conn, Teamed::Snippets, id)?,
        None => vaults::check(&conn, team_id.as_deref())?,
    }
    db::upsert_snippet(&conn, id, name.trim(), command.trim(), team_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_snippet(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    vaults::check_row(&conn, Teamed::Snippets, id)?;
    db::delete_snippet(&conn, id).map_err(|e| e.to_string())
}

/// Move a snippet to another vault (None: personal)
#[tauri::command]
pub fn move_snippet(db: State<Db>, id: i64, team_id: Option<String>) -> Result<Snippet, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let moved = vaults::move_snippet(&tx, id, team_id.as_deref())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(moved)
}
