use tauri::State;

use crate::store::models::AppSettings;
use crate::store::db;
use crate::Db;

#[tauri::command]
pub fn get_settings(db: State<Db>) -> Result<AppSettings, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::get_settings(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_settings(db: State<Db>, settings: AppSettings) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::set_settings(&conn, &settings).map_err(|e| e.to_string())
}
