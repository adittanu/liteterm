use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use tauri::State;

use crate::ssh::pool::{self, AppState};

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// An image pasted into a terminal tab: saved where that tab's programs can
/// open it, and its path returned so the terminal can type it in (opencode
/// and other agent CLIs take an image by path). An SSH tab gets it in the
/// server's /tmp (its home when there is no /tmp), readable only by the
/// login user; a local tab in this computer's temp folder.
#[tauri::command]
pub async fn paste_image(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    data_base64: String,
    ext: String,
) -> Result<String, String> {
    let ext = ext.to_ascii_lowercase();
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err("Unsupported image type".into());
    }
    let bytes = STANDARD.decode(data_base64.as_bytes()).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err("The image is larger than 25 MB".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis();
    let name = format!("liteterm-paste-{stamp}.{ext}");

    let ssh = state.sessions.lock().unwrap().get(&session_id).cloned();
    if let Some(session) = ssh {
        pool::sftp_open(state.inner(), &session).await?;
        let tmp = format!("/tmp/{name}");
        match pool::sftp_put_private(&session, &tmp, &bytes).await {
            Ok(()) => return Ok(tmp),
            Err(tmp_err) => {
                // No writable /tmp (a Windows server, a locked-down box): try home
                let home = pool::sftp_home(&session).await.map_err(|_| tmp_err.clone())?;
                let path = format!("{}/{name}", home.trim_end_matches('/'));
                pool::sftp_put_private(&session, &path, &bytes).await.map_err(|_| tmp_err)?;
                return Ok(path);
            }
        }
    }

    if state.local_sessions.lock().unwrap().contains_key(&session_id) {
        let dir = std::env::temp_dir().join("liteterm-paste");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let path = dir.join(name);
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        return Ok(path.to_string_lossy().into_owned());
    }

    Err("Session not found".into())
}
