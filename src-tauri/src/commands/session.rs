use std::sync::Arc;

use rusqlite::Connection;
use tauri::State;

use crate::ssh::pool::{self, AuthSpec, ConnParams, AppState, SshSession};
use crate::store::db;
use crate::Db;

/// Build the auth for one saved host. `password` is a secret the user just
/// typed for this host (password or key passphrase); `None` falls back to
/// the keyring. `missing` decides how a missing secret is reported.
fn resolve_auth(
    conn: &Connection,
    host: &crate::store::models::Host,
    password: Option<String>,
    missing: &dyn Fn(&str, &str) -> String,
) -> Result<(String, AuthSpec), String> {
    let host_id = host.id;
    // A linked credential supplies username + auth; inline fields are the
    // fallback for hosts without one
    if let Some(cred_id) = host.credential_id {
        let cred = db::get_credential(conn, cred_id)
            .map_err(|e| e.to_string())?
            .ok_or("The linked credential no longer exists")?;
        let auth = match cred.auth_method.as_str() {
            "agent" => AuthSpec::Agent,
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
                        return Err(missing("passphrase", &cred.name));
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
                    None => return Err(missing("password", &cred.name)),
                };
                AuthSpec::Password(pw)
            }
        };
        return Ok((cred.username, auth));
    }

    let auth = match host.auth_method.as_str() {
        "agent" => AuthSpec::Agent,
        "key" => {
            let key_path = host.key_path.clone().ok_or("No key file configured")?;
            let passphrase = password
                .clone()
                .or_else(|| db::keyring_get(&db::keypass_entry(host_id)));
            // Fail fast here (in the command, not the session task) on a locked key
            if let Err(e) = russh::keys::load_secret_key(&key_path, passphrase.as_deref()) {
                if matches!(e, russh::keys::Error::KeyIsEncrypted) && passphrase.is_none() {
                    return Err(missing("passphrase", &host.label));
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
                None => return Err(missing("password", &host.label)),
            };
            AuthSpec::Password(pw)
        }
    };
    Ok((host.username.clone(), auth))
}

fn resolve_conn_params(
    conn: &Connection,
    host_id: i64,
    password: Option<String>,
) -> Result<(ConnParams, String), String> {
    let host = db::get_host(conn, host_id)
        .map_err(|e| e.to_string())?
        .ok_or("Host not found")?;

    // The target can prompt: the frontend reopens its password dialog
    let prompt = |kind: &str, name: &str| format!("{kind}_required:{name}");
    let (username, auth) = resolve_auth(conn, &host, password, &prompt)?;

    // Bastions can't prompt (the dialog would answer for the target), so
    // their secret must already be saved — say exactly what to fix
    let mut jump: Option<Box<ConnParams>> = None;
    let mut chain: Vec<crate::store::models::Host> = Vec::new();
    let mut seen = vec![host.id];
    let mut next = host.jump_host_id;
    while let Some(jid) = next {
        if seen.contains(&jid) {
            return Err(format!("Jump hosts for “{}” form a loop — check each host's Jump host", host.label));
        }
        if chain.len() >= pool::MAX_JUMP_DEPTH {
            return Err(format!("Too many jump hosts chained for “{}”", host.label));
        }
        seen.push(jid);
        let bastion = db::get_host(conn, jid)
            .map_err(|e| e.to_string())?
            .ok_or("The jump host no longer exists — edit this host and pick another")?;
        next = bastion.jump_host_id;
        chain.push(bastion);
    }
    // chain = [nearest bastion, …, outermost]; nest from the outside in
    for bastion in chain.into_iter().rev() {
        let saved_only = |kind: &str, name: &str| {
            format!(
                "Jump host “{name}” needs a saved {kind} — edit it and turn on “Remember”, \
                 or switch it to SSH agent / key authentication"
            )
        };
        let (b_user, b_auth) = resolve_auth(conn, &bastion, None, &saved_only)?;
        jump = Some(Box::new(ConnParams {
            address: bastion.address.clone(),
            port: bastion.port as u16,
            username: b_user,
            auth: b_auth,
            label: bastion.label.clone(),
            jump,
        }));
    }

    Ok((
        ConnParams {
            address: host.address.clone(),
            port: host.port as u16,
            username,
            auth,
            label: host.label.clone(),
            jump,
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
        // Recency: the access attempt itself bumps last_used_at, even if the
        // connection later fails
        db::touch_host(&conn, host_id).ok();
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
        label: format!("{username}@{address}"),
        jump: None,
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

/// Skip the remaining backoff of an auto-reconnecting session.
#[tauri::command]
pub async fn reconnect_now(state: State<'_, Arc<AppState>>, session_id: u32) -> Result<(), String> {
    pool::reconnect_now(state.inner(), session_id)
}

#[tauri::command]
pub async fn spawn_local(
    state: State<'_, Arc<AppState>>,
    command: Option<String>,
) -> Result<u32, String> {
    crate::local::spawn_local(state.inner(), command.as_deref())
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
