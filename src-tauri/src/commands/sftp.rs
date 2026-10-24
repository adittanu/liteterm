use std::sync::Arc;

use tauri::State;

use crate::ssh::pool::{self, AppState};

#[tauri::command]
pub async fn sftp_open(state: State<'_, Arc<AppState>>, session_id: u32) -> Result<(), String> {
    let session = pool_session(state.inner(), session_id)?;
    pool::sftp_open(state.inner(), &session).await
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    path: String,
) -> Result<Vec<crate::store::models::FileEntry>, String> {
    let session = pool_session(state.inner(), session_id)?;
    pool::sftp_list(&session, &path).await
}

#[tauri::command]
pub async fn sftp_mkdir(state: State<'_, Arc<AppState>>, session_id: u32, path: String) -> Result<(), String> {
    let session = pool_session(state.inner(), session_id)?;
    pool::sftp_mkdir(&session, &path).await
}

#[tauri::command]
pub async fn sftp_remove(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    path: String,
    is_dir: bool,
) -> Result<(), String> {
    let session = pool_session(state.inner(), session_id)?;
    pool::sftp_remove(&session, &path, is_dir).await
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    from: String,
    to: String,
) -> Result<(), String> {
    let session = pool_session(state.inner(), session_id)?;
    pool::sftp_rename(&session, &from, &to).await
}

#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    local_path: String,
    remote_path: String,
) -> Result<u32, String> {
    let app_state = state.inner().clone();
    let session = pool_session(state.inner(), session_id)?;
    let transfer_id = session.next_transfer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        pool::run_upload(app_state, session, transfer_id, local_path, remote_path).await;
    });
    Ok(transfer_id)
}

#[tauri::command]
pub async fn sftp_download(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    remote_path: String,
    local_path: String,
) -> Result<u32, String> {
    let app_state = state.inner().clone();
    let session = pool_session(state.inner(), session_id)?;
    let transfer_id = session.next_transfer_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        pool::run_download(app_state, session, transfer_id, remote_path, local_path).await;
    });
    Ok(transfer_id)
}

#[tauri::command]
pub fn sftp_cancel_transfer(state: State<'_, Arc<AppState>>, session_id: u32, transfer_id: u32) -> Result<(), String> {
    pool::cancel_transfer(state.inner(), session_id, transfer_id);
    Ok(())
}

fn pool_session(state: &AppState, session_id: u32) -> Result<std::sync::Arc<pool::SshSession>, String> {
    state
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .ok_or_else(|| "Session not found".into())
}
