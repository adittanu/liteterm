use tauri::State;

use crate::cloud::items::Keyring;
use crate::store::db;
use crate::store::models::{Credential, CredentialInput};
use crate::store::stored_keys;
use crate::store::vaults::{self, Teamed};
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
    mut input: CredentialInput,
) -> Result<Credential, String> {
    if input.name.trim().is_empty() {
        return Err("Credential name is required".into());
    }
    if input.username.trim().is_empty() {
        return Err("Username is required".into());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let (in_team, stored_now) = match id {
        Some(id) => {
            vaults::check_row(&conn, Teamed::Credentials, id)?;
            db::get_credential(&conn, id)
                .map_err(|e| e.to_string())?
                .map_or((false, false), |c| (c.team_id.is_some(), c.key_stored))
        }
        None => {
            vaults::check(&conn, input.team_id.as_deref())?;
            (input.team_id.is_some(), false)
        }
    };
    stored_keys::prepare(&mut input, in_team, stored_now)?;
    db::upsert_credential(&conn, id, &input).map_err(|e| e.to_string())
}

/// The public half of a credential's stored key, for authorized_keys
#[tauri::command]
pub fn credential_public_key(db: State<Db>, id: i64) -> Result<String, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    let key = stored_keys::load(&conn, &Keyring, id)?.ok_or("This credential has no stored key")?;
    stored_keys::public_key(&key)
}

#[tauri::command]
pub fn delete_credential(db: State<Db>, id: i64) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    vaults::check_row(&conn, Teamed::Credentials, id)?;
    db::delete_credential(&conn, id).map_err(|e| e.to_string())
}
