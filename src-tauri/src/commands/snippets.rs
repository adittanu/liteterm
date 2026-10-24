use tauri::State;

use crate::store::models::Snippet;
use crate::store::db;
use crate::Db;

#[tauri::command]
pub fn list_snippets(db: State<Db>) -> Result<Vec<Snippet>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_snippets(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_snippet(db: State<Db>, id: Option<i64>, name: String, command: String) -> Result<Snippet, String> {
    if name.trim().is_empty() || command.trim().is_empty() {
        return Err("Name and command are required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::upsert_snippet(&conn, id, name.trim(), command.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_snippet(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_snippet(&conn, id).map_err(|e| e.to_string())
}
