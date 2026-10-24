use std::sync::Arc;

use rusqlite::Connection;
use tauri::State;

use crate::ssh::pool::{self, AuthSpec, ConnParams, AppState, SshSession};
use crate::store::db;
use crate::Db;

fn resolve_conn_params(
    conn: &Connection,
    host_id: i64,
    password: Option<String>,
) -> Result<(ConnParams, String), String> {
    let host = db::get_host(conn, host_id)
        .map_err(|e| e.to_string())?
        .ok_or("Host not found")?;

    // A linked credential supplies username + auth; inline fields are the
    // fallback for hosts without one
    let (username, auth) = if let Some(cred_id) = host.credential_id {
        let cred = db::get_credential(conn, cred_id)
            .map_err(|e| e.to_string())?
            .ok_or("The linked credential no longer exists")?;
        let auth = match cred.auth_method.as_str() {
            "key" => {
                let key_path = cred
                    .key_path
                    .clone()
                    .ok_or("Credential has no key file configured")?;
                let passphrase = password
                    .clone()
                    .or_else(|| db::keyring_get(&db::cred_keypass_entry(cred_id)));
                if let Err(e) = russh::keys::load_secret_key(&key_path, passphrase.as_deref()) {
                    if matches!(e, russh::keys::Error::KeyIsEncrypted) && passphrase.is_none() {
                        return Err(format!("passphrase_required:{}", cred.name));
                    }
                    return Err(format!("Could not load key: {e}"));
                }
                AuthSpec::Key { path: key_path, passphrase }
            }
            _ => {
                let pw = match password
                    .clone()
                    .or_else(|| db::keyring_get(&db::cred_password_entry(cred_id)))
                {
                    Some(pw) => pw,
                    None => return Err(format!("password_required:{}", cred.name)),
                };
                AuthSpec::Password(pw)
            }
        };
        (cred.username, auth)
    } else {
        let auth = match host.auth_method.as_str() {
            "key" => {
                let key_path = host.key_path.clone().ok_or("No key file configured")?;
                let passphrase = password
                    .clone()
                    .or_else(|| db::keyring_get(&db::keypass_entry(host_id)));
                // Fail fast here (in the command, not the session task) on a locked key
                if let Err(e) = russh::keys::load_secret_key(&key_path, passphrase.as_deref()) {
                    if matches!(e, russh::keys::Error::KeyIsEncrypted) && passphrase.is_none() {
                        return Err(format!("passphrase_required:{}", host.label));
                    }
                    return Err(format!("Could not load key: {e}"));
                }
                AuthSpec::Key { path: key_path, passphrase }
            }
            _ => {
                let pw = match password
                    .clone()
                    .or_else(|| db::keyring_get(&db::password_entry(host_id)))
                {
                    Some(pw) => pw,
                    None => return Err(format!("password_required:{}", host.label)),
                };
                AuthSpec::Password(pw)
            }
        };
        (host.username.clone(), auth)
    };

    Ok((
        ConnParams {
            address: host.address.clone(),
            port: host.port as u16,
            username,
            auth,
        },
        host.label,
    ))
}

fn registered_session(state: &Arc<AppState>, id: u32) -> Result<Arc<SshSession>, String> {
    state
        .sessions
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .ok_or_else(|| "Session vanished".into())
}

#[tauri::command]
pub async fn connect_host(
    state: State<'_, Arc<AppState>>,
    db: State<'_, Db>,
    host_id: i64,
    password: Option<String>,
) -> Result<u32, String> {
    let (params, label) = {
        let conn = db.0.lock().map_err(|_| "database busy")?;
        resolve_conn_params(&conn, host_id, password)?
    };

    let app_state = state.inner().clone();
    let session_id = pool::create_session(
        &app_state,
        Some(host_id),
        &label,
        &params.username,
        &params.address,
    );
    let session = registered_session(&app_state, session_id)?;

    tauri::async_runtime::spawn(async move {
        pool::run_session(app_state, session, params).await;
    });

    Ok(session_id)
}

#[tauri::command]
pub async fn connect_adhoc(
    state: State<'_, Arc<AppState>>,
    address: String,
    port: u16,
    username: String,
    password: Option<String>,
) -> Result<u32, String> {
    let app_state = state.inner().clone();
    let params = ConnParams {
        address: address.clone(),
        port,
        username: username.clone(),
        auth: AuthSpec::Password(password.unwrap_or_default()),
    };
    let session_id = pool::create_session(
        &app_state,
        None,
        &format!("{username}@{address}"),
        &username,
        &address,
    );
    let session = registered_session(&app_state, session_id)?;

    tauri::async_runtime::spawn(async move {
        pool::run_session(app_state, session, params).await;
    });

    Ok(session_id)
}

#[tauri::command]
pub async fn confirm_host_key(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    accept: bool,
    remember: bool,
) -> Result<(), String> {
    let tx = state
        .pending_hostkey
        .lock()
        .unwrap()
        .remove(&session_id)
        .ok_or("No pending host key check for this session")?;
    tx.send(pool::HostKeyDecision { accept, remember })
        .await
        .map_err(|_| "Connection already ended".to_string())
}

#[tauri::command]
pub async fn spawn_local(state: State<'_, Arc<AppState>>) -> Result<u32, String> {
    crate::local::spawn_local(state.inner())
}

#[tauri::command]
pub async fn disconnect_session(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
) -> Result<(), String> {
    // Local sessions take priority in lookup; SSH disconnect is a no-op for them
    let local = state.local_sessions.lock().unwrap().get(&session_id).cloned();
    if let Some(session) = local {
        crate::local::close_local(state.inner(), &session);
        return Ok(());
    }
    pool::disconnect_session(state.inner(), session_id).await;
    Ok(())
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    data: String,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    let local = state.local_sessions.lock().unwrap().get(&session_id).cloned();
    if let Some(session) = local {
        return crate::local::local_write(&session, &data);
    }

    let session = registered_session(state.inner(), session_id)?;
    let mut guard = session.writer.lock().await;
    let writer = guard.as_mut().ok_or("Not connected")?;
    writer
        .write_all(data.as_bytes())
        .await
        .map_err(|e| format!("Write failed: {e}"))
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let local = state.local_sessions.lock().unwrap().get(&session_id).cloned();
    if let Some(session) = local {
        let c = u16::try_from(cols).unwrap_or(u16::MAX);
        let r = u16::try_from(rows).unwrap_or(u16::MAX);
        return crate::local::local_resize(&session, c, r);
    }

    let session = registered_session(state.inner(), session_id)?;
    let guard = session.resize_tx.lock().await;
    let tx = guard.as_ref().ok_or("Not connected")?;
    tx.send((cols, rows))
        .await
        .map_err(|_| "Session ended".to_string())
}
