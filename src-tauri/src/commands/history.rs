use std::sync::Arc;

use tauri::State;

use crate::history_import::{drop_partial_first_line, parse_bash, parse_fish, parse_zsh, MAX_COMMAND_LEN};
use crate::ssh::pool::{self, AppState};
use crate::store::db;
use crate::Db;

/// How much of each history file is read: the newest few thousand commands
const TAIL_BYTES: u64 = 256 * 1024;

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Commands typed on this host (None = local terminals), newest first
#[tauri::command]
pub fn history_list(db: State<Db>, host_id: Option<i64>) -> Result<Vec<String>, String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::history_list(&conn, &db::history_key(host_id)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_add(db: State<Db>, host_id: Option<i64>, command: String) -> Result<(), String> {
    let command = command.trim_end().to_string();
    if command.is_empty() || command.starts_with(' ') || command.len() > MAX_COMMAND_LEN || command.contains(['\n', '\r']) {
        return Ok(());
    }
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::history_add(&conn, &db::history_key(host_id), &[command], now_ms()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn history_clear(db: State<Db>) -> Result<(), String> {
    let conn = db.0.lock().map_err(|_| "database busy")?;
    db::history_clear(&conn).map_err(|e| e.to_string())
}

/// Read the server's own bash, zsh and fish history into this host's
/// suggestions, once per host. Returns how many commands came in (0 when
/// it was done before or the server has none).
#[tauri::command]
pub async fn history_import(
    state: State<'_, Arc<AppState>>,
    db: State<'_, Db>,
    session_id: u32,
) -> Result<usize, String> {
    let session = state
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .ok_or("Session not found")?;
    let Some(host_id) = session.host_id else { return Ok(0) };
    let key = db::history_key(Some(host_id));
    {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        if db::history_imported(&conn, &key).map_err(|e| e.to_string())? {
            return Ok(0);
        }
    }

    pool::sftp_open(state.inner(), &session).await?;
    // fish last: it is the shell most likely in use when its file exists,
    // so its commands rank above the other two
    let files: [(&str, fn(&str) -> Vec<String>); 3] = [
        (".bash_history", parse_bash),
        (".zsh_history", parse_zsh),
        (".local/share/fish/fish_history", parse_fish),
    ];
    let mut commands = Vec::new();
    for (path, parse) in files {
        if let Some((bytes, cut)) = pool::sftp_read_tail(&session, path, TAIL_BYTES).await {
            let text = String::from_utf8_lossy(&bytes);
            commands.extend(parse(drop_partial_first_line(&text, cut)));
        }
    }
    let keep_from = commands.len().saturating_sub(db::HISTORY_LIMIT as usize);
    let commands = &commands[keep_from..];

    let conn = db.0.lock().map_err(|_| "database busy")?;
    // Stamped from 1 up: older than anything typed in LiteTerm, in file order
    db::history_add(&conn, &key, commands, 1).map_err(|e| e.to_string())?;
    db::history_mark_imported(&conn, &key).map_err(|e| e.to_string())?;
    Ok(commands.len())
}
