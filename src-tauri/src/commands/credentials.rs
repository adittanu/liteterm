use tauri::State;

use crate::store::db;
use crate::store::models::{Credential, CredentialInput};
use crate::Db;

#[tauri::command]
pub fn list_credentials(db: State<Db>) -> Result<Vec<Credential>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::list_credentials(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_credential(
    db: State<Db>,
    id: Option<i64>,
    input: CredentialInput,
) -> Result<Credential, String> {
    if input.name.trim().is_empty() {
        return Err("Credential name is required".into());
    }
    if input.username.trim().is_empty() {
        return Err("Username is required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::upsert_credential(&conn, id, &input).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_credential(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::delete_credential(&conn, id).map_err(|e| e.to_string())
}
