//! Account and cloud sync (see cloud/engine.rs). Passwords and the recovery
//! key arrive as plain strings from the webview; they are wrapped for
//! zeroizing at once and never stored.

use std::sync::Arc;

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use crate::cloud::engine::{Cloud, Status, TeamDetail, TeamSummary, TeamsView};

type CloudState<'a> = State<'a, Arc<Cloud>>;

fn text(e: crate::cloud::engine::Error) -> String {
    e.to_string()
}

#[tauri::command]
pub fn cloud_status(app: AppHandle, cloud: CloudState<'_>) -> Result<Status, String> {
    cloud.status(&app).map_err(text)
}

#[tauri::command]
pub async fn cloud_send_code(cloud: CloudState<'_>, email: String) -> Result<(), String> {
    cloud.send_code(&email).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_verify_code(
    app: AppHandle,
    cloud: CloudState<'_>,
    email: String,
    code: String,
) -> Result<Status, String> {
    cloud.verify_code(&app, &email, &code).await.map_err(text)
}

/// Returns the recovery key, to show once
#[tauri::command]
pub async fn cloud_create_vault(
    app: AppHandle,
    cloud: CloudState<'_>,
    password: String,
) -> Result<String, String> {
    let recovery = cloud.create_vault(&app, Zeroizing::new(password)).await.map_err(text)?;
    Ok(recovery.to_string())
}

#[tauri::command]
pub async fn cloud_unlock(app: AppHandle, cloud: CloudState<'_>, password: String) -> Result<Status, String> {
    cloud.unlock(&app, Zeroizing::new(password)).await.map_err(text)
}

/// Returns the new recovery key, to show once
#[tauri::command]
pub async fn cloud_recover(
    app: AppHandle,
    cloud: CloudState<'_>,
    recovery_key: String,
    new_password: String,
) -> Result<String, String> {
    let recovery = cloud
        .recover(&app, Zeroizing::new(recovery_key), Zeroizing::new(new_password))
        .await
        .map_err(text)?;
    Ok(recovery.to_string())
}

#[tauri::command]
pub async fn cloud_change_password(
    app: AppHandle,
    cloud: CloudState<'_>,
    current: String,
    new_password: String,
) -> Result<(), String> {
    cloud
        .change_password(&app, Zeroizing::new(current), Zeroizing::new(new_password))
        .await
        .map_err(text)
}

#[tauri::command]
pub async fn cloud_reset_vault(app: AppHandle, cloud: CloudState<'_>) -> Result<Status, String> {
    cloud.reset_vault(&app).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_sign_out(app: AppHandle, cloud: CloudState<'_>) -> Result<Status, String> {
    cloud.sign_out(&app).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_sync_now(app: AppHandle, cloud: CloudState<'_>) -> Result<Status, String> {
    cloud.sync(&app).await.map_err(text)
}

// ---- team vaults ----

#[tauri::command]
pub async fn cloud_teams(app: AppHandle, cloud: CloudState<'_>) -> Result<TeamsView, String> {
    cloud.teams(&app).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_team_detail(app: AppHandle, cloud: CloudState<'_>, team_id: String) -> Result<TeamDetail, String> {
    cloud.team_detail(&app, &team_id).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_create_team(app: AppHandle, cloud: CloudState<'_>, name: String) -> Result<TeamSummary, String> {
    cloud.create_team(&app, &name).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_rename_team(app: AppHandle, cloud: CloudState<'_>, team_id: String, name: String) -> Result<(), String> {
    cloud.rename_team(&app, &team_id, &name).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_delete_team(app: AppHandle, cloud: CloudState<'_>, team_id: String) -> Result<(), String> {
    cloud.delete_team(&app, &team_id).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_invite(
    app: AppHandle,
    cloud: CloudState<'_>,
    team_id: String,
    email: String,
    role: String,
) -> Result<(), String> {
    cloud.invite(&app, &team_id, &email, &role).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_accept_invite(app: AppHandle, cloud: CloudState<'_>, invite_id: String) -> Result<(), String> {
    cloud.accept_invite(&app, &invite_id).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_cancel_invite(app: AppHandle, cloud: CloudState<'_>, invite_id: String) -> Result<(), String> {
    cloud.cancel_invite(&app, &invite_id).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_admit(
    app: AppHandle,
    cloud: CloudState<'_>,
    invite_id: String,
    shown_code: String,
) -> Result<(), String> {
    cloud.admit(&app, &invite_id, &shown_code).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_set_member_role(
    app: AppHandle,
    cloud: CloudState<'_>,
    team_id: String,
    user_id: String,
    role: String,
) -> Result<(), String> {
    cloud.set_role(&app, &team_id, &user_id, &role).await.map_err(text)
}

/// Returns the team's saved passwords the removed member could read
#[tauri::command]
pub async fn cloud_remove_member(
    app: AppHandle,
    cloud: CloudState<'_>,
    team_id: String,
    user_id: String,
) -> Result<Vec<String>, String> {
    cloud.remove_member(&app, &team_id, &user_id).await.map_err(text)
}

#[tauri::command]
pub async fn cloud_leave_team(app: AppHandle, cloud: CloudState<'_>, team_id: String) -> Result<(), String> {
    cloud.leave_team(&app, &team_id).await.map_err(text)
}
