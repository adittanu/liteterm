use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD, Engine};
use tauri::{AppHandle, Manager, State};

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
pub fn set_note_archived(db: State<Db>, id: i64, archived: bool) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::set_note_archived(&conn, id, archived).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_note_tags(db: State<Db>, id: i64, tags: Vec<String>) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::set_note_tags(&conn, id, &tags).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_note_host(db: State<Db>, id: i64, host_id: Option<i64>) -> Result<Note, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::set_note_host(&conn, id, host_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_notes(db: State<Db>, ids: Vec<i64>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::reorder_notes(&conn, &ids).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_note(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_note(&conn, id).map_err(|e| e.to_string())
}

const NOTE_IMAGE_EXTS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
const MAX_NOTE_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// Store a pasted or dropped image under `<app data>/note-images/` and
/// return its absolute path. The note body keeps only the asset URL, so
/// the SQLite row stays small no matter how big the picture is.
#[tauri::command]
pub fn save_note_image(app: AppHandle, data_base64: String, ext: String) -> Result<String, String> {
    let ext = ext.to_ascii_lowercase();
    if !NOTE_IMAGE_EXTS.contains(&ext.as_str()) {
        return Err("unsupported image type".into());
    }
    let bytes = STANDARD.decode(data_base64.as_bytes()).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_NOTE_IMAGE_BYTES {
        return Err("image is larger than 25 MB".into());
    }

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("note-images");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = dir.join(format!("{nanos}-{}.{ext}", std::process::id()));
    fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}
