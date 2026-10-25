use tauri::State;

use crate::store::db;
use crate::store::models::Note;
use crate::Db;

#[tauri::command]
pub fn list_notes(db: State<Db>) -> Result<Vec<Note>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_notes(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_note(db: State<Db>, title: Option<String>, body: Option<String>) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::create_note(
        &conn,
        title.as_deref().unwrap_or(""),
        body.as_deref().unwrap_or(""),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_note(db: State<Db>, id: i64, title: String, body: String) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::update_note(&conn, id, &title, &body).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_note_pinned(db: State<Db>, id: i64, pinned: bool) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::set_note_pinned(&conn, id, pinned).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_note(&conn, id).map_err(|e| e.to_string())
}
