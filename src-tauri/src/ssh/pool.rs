use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::store::{db, models};

const KEEPALIVE_SECS: u64 = 30;
const CONNECT_TIMEOUT_SECS: u64 = 15;
const TRANSFER_CHUNK: usize = 32 * 1024;

/// Answer keyboard-interactive prompts with the password (up to 3 rounds).
async fn authenticate_keyboard_interactive(
    handle: &mut russh::client::Handle<ClientHandler>,
    username: &str,
    password: &str,
) -> bool {
    use russh::client::KeyboardInteractiveAuthResponse as R;

    let mut response = match handle
        .authenticate_keyboard_interactive_start(username, None)
        .await
    {
        Ok(r) => r,
        Err(_) => return false,
    };

    for _ in 0..3 {
        match response {
            R::Success => return true,
            R::Failure => return false,
            R::InfoRequest { prompts, .. } => {
                // One response per prompt; password prompts expect the secret,
                // anything else (e.g. TOTP with empty prompt set) gets the
                // password too — if the server wanted a TOTP it will fail
                // gracefully below.
                let answers = prompts.iter().map(|_| password.to_string()).collect();
                response = match handle.authenticate_keyboard_interactive_respond(answers).await {
                    Ok(r) => r,
                    Err(_) => return false,
                }
            }
        }
    }
    false
}

// ---------- shared app state ----------

pub struct AppState {
    pub app: AppHandle,
    pub db_path: PathBuf,
    pub next_session_id: AtomicU32,
    pub sessions: Mutex<HashMap<u32, Arc<SshSession>>>,
    pub local_sessions: Mutex<HashMap<u32, Arc<crate::local::LocalSession>>>,
    pub pending_hostkey: Mutex<HashMap<u32, mpsc::Sender<HostKeyDecision>>>,
    pub forwards: Mutex<HashMap<i64, ForwardHandle>>,
}

pub struct HostKeyDecision {
    pub accept: bool,
    pub remember: bool,
}

pub struct ForwardHandle {
    pub cancel: CancellationToken,
    pub listener: tokio::task::JoinHandle<()>,
}

type Shared<T> = Arc<tokio::sync::Mutex<T>>;
type ChannelWriter = Box<dyn tokio::io::AsyncWrite + Unpin + Send>;

pub struct SshSession {
    pub id: u32,
    pub host_id: Option<i64>,
    #[allow(dead_code)] // kept for future logging/forward naming
    pub label: String,
    #[allow(dead_code)]
    pub address: String,
    pub handle: Shared<Option<russh::client::Handle<ClientHandler>>>,
    pub writer: Shared<Option<ChannelWriter>>,
    pub resize_tx: Shared<Option<mpsc::Sender<(u32, u32)>>>,
    pub sftp: Arc<tokio::sync::Mutex<Option<russh_sftp::client::SftpSession>>>,
    /// The key-mismatch path reports its own detailed error; this flag
    /// tells run_session not to stack a generic second one
    pub key_mismatch: Arc<std::sync::atomic::AtomicBool>,
    /// Kept in memory only, for auto-filling sudo prompts
    pub sudo_password: Arc<Mutex<Option<String>>>,
    pub closed: CancellationToken,
    pub transfers: Mutex<HashMap<u32, CancellationToken>>,
    pub next_transfer_id: AtomicU32,
}

// ---------- events ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub session_id: u32,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StdoutEvent {
    pub session_id: u32,
    pub data: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransferEvent {
    pub transfer_id: u32,
    pub transferred: u64,
    pub size: u64,
    pub done: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForwardEvent {
    pub id: i64,
    pub active: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SudoFilledEvent {
    pub session_id: u32,
}

pub fn emit_status(app: &AppHandle, session_id: u32, status: &str, error: Option<String>) {
    let _ = app.emit(
        "session:status",
        StatusEvent {
            session_id,
            status: status.into(),
            error,
        },
    );
}

// ---------- ssh client handler ----------

pub struct ClientHandler {
    pub host_tag: String,
    pub session_id: u32,
    /// Trusted (key_type, public_key_b64) pairs for this host
    pub known_keys: Vec<(String, String)>,
    /// Set when this handler already reported a host-key mismatch so
    /// run_session can skip the generic follow-up error
    pub key_mismatch: Arc<std::sync::atomic::AtomicBool>,
    pub state: Arc<AppState>,
}

#[async_trait::async_trait]
impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        use russh::keys::PublicKeyBase64;

        let fingerprint = server_public_key.fingerprint();
        let key_str = server_public_key.public_key_base64();
        let key_type = server_public_key.name().to_string();

        if !self.known_keys.is_empty() {
            let exact = self
                .known_keys
                .iter()
                .any(|(t, k)| t == &key_type && k == &key_str);
            if exact {
                return Ok(true);
            }
            let same_type_differs = self
                .known_keys
                .iter()
                .any(|(t, _)| t == &key_type);

            if same_type_differs {
                // Same key type, different bytes — this is what a MITM or a
                // genuine server rebuild looks like. Loud failure, no prompt.
                self.key_mismatch.store(true, Ordering::SeqCst);
                emit_status(
                    &self.state.app,
                    self.session_id,
                    "error",
                    Some(format!(
                        "HOST KEY CHANGED for {}: the saved {key_type} key no longer matches. \
                         If you did not rebuild this server, do not connect — \
                         use “Reset host key” only when you are sure.",
                        self.host_tag
                    )),
                );
                return Ok(false);
            }
            // Host known under other key types only — servers rotate types;
            // accept and remember the new type (OpenSSH accept-new behavior).
            if let Ok(conn) = Connection::open(&self.state.db_path) {
                let _ = db::remember_host_key(&conn, &self.host_tag, &key_type, &key_str, &fingerprint);
            }
            return Ok(true);
        }

        // Trust on first use — ask the user
        let (tx, mut rx) = mpsc::channel::<HostKeyDecision>(1);
        self.state
            .pending_hostkey
            .lock()
            .unwrap()
            .insert(self.session_id, tx);

        let _ = self.state.app.emit(
            "hostkey:pending",
            models::HostKeyEvent {
                session_id: self.session_id,
                fingerprint,
                key_type,
                address: self.host_tag.clone(),
            },
        );

        match rx.recv().await {
            Some(decision) => {
                if decision.accept {
                    if decision.remember {
                        if let Ok(conn) = Connection::open(&self.state.db_path) {
                            let _ = db::remember_host_key(
                                &conn,
                                &self.host_tag,
                                server_public_key.name(),
                                &key_str,
                                &server_public_key.fingerprint(),
                            );
                        }
                    }
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            None => Ok(false),
        }
    }
}

// ---------- connection lifecycle ----------

pub enum AuthSpec {
    Password(String),
    Key {
        path: String,
        passphrase: Option<String>,
    },
}

pub struct ConnParams {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthSpec,
}

pub fn create_session(
    state: &AppState,
    host_id: Option<i64>,
    label: &str,
    username: &str,
    address: &str,
) -> u32 {
    let id = state.next_session_id.fetch_add(1, Ordering::SeqCst) + 1;
    let session = SshSession {
        id,
        host_id,
        label: label.to_string(),
        address: format!("{username}@{address}"),
        handle: Arc::default(),
        writer: Arc::default(),
        resize_tx: Arc::default(),
        sftp: Arc::default(),
        key_mismatch: Arc::default(),
        sudo_password: Arc::new(Mutex::new(None)),
        closed: CancellationToken::new(),
        transfers: Mutex::new(HashMap::new()),
        next_transfer_id: AtomicU32::new(1),
    };
    state.sessions.lock().unwrap().insert(id, Arc::new(session));
    id
}

pub fn remove_session(state: &AppState, session_id: u32) {
    if let Some(session) = state.sessions.lock().unwrap().remove(&session_id) {
        session.closed.cancel();
    }
}

pub async fn run_session(state: Arc<AppState>, session: Arc<SshSession>, params: ConnParams) {
    let host_tag = format!("{}:{}", params.address, params.port);

    let result = connect_and_stream(&state, &session, &params, &host_tag).await;

    if let Err(msg) = result {
        // A key mismatch already emitted its own detailed error event
        if !session.key_mismatch.load(Ordering::SeqCst) {
            emit_status(&state.app, session.id, "error", Some(msg));
        }
    }
    remove_session(&state, session.id);
}

async fn connect_and_stream(
    state: &Arc<AppState>,
    session: &Arc<SshSession>,
    params: &ConnParams,
    host_tag: &str,
) -> Result<(), String> {
    // Look up all previously trusted keys for this host (per key type)
    let known_keys = Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::get_known_hosts(&conn, host_tag).ok())
        .unwrap_or_default();

    let config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(KEEPALIVE_SECS)),
        ..Default::default()
    });

    let handler = ClientHandler {
        host_tag: host_tag.to_string(),
        session_id: session.id,
        known_keys,
        key_mismatch: session.key_mismatch.clone(),
        state: state.clone(),
    };

    let mut handle = tokio::time::timeout(
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
        russh::client::connect(config, (params.address.as_str(), params.port), handler),
    )
    .await
    .map_err(|_| format!("Connection timed out after {CONNECT_TIMEOUT_SECS}s — check the address and port"))?
    .map_err(|e| {
        let s = e.to_string();
        if s.contains("UnknownKey") || s.contains("unknown key") || s.contains("Unknown server key") {
            "Host key rejected".to_string()
        } else {
            format!("Connection failed: {s}")
        }
    })?;

    // ---- authenticate ----
    let authed = match &params.auth {
        AuthSpec::Password(pw) => {
            let by_password = handle
                .authenticate_password(&params.username, pw)
                .await
                .map_err(|e| format!("Auth error: {e}"))?;
            // Many servers (shared hosts like Hostinger, some cloud images)
            // disable the "password" method but accept the same secret via
            // keyboard-interactive. Answer its prompts with the password.
            if by_password {
                true
            } else {
                authenticate_keyboard_interactive(&mut handle, &params.username, pw).await
            }
        }
        AuthSpec::Key { path, passphrase } => {
            let key = russh::keys::load_secret_key(path, passphrase.as_deref())
                .map_err(|e| format!("Could not load key: {e}"))?;
            handle
                .authenticate_publickey(&params.username, Arc::new(key))
                .await
                .map_err(|e| format!("Auth error: {e}"))?
        }
    };

    if !authed {
        return Err("Authentication failed: check username and credentials".into());
    }

    // ---- open PTY + shell ----
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("Could not open channel: {e}"))?;

    let mut channel = channel;
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .map_err(|e| format!("PTY request failed: {e}"))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| format!("Shell request failed: {e}"))?;

    *session.handle.lock().await = Some(handle);
    let writer = channel.make_writer();
    *session.writer.lock().await = Some(Box::new(writer));

    // Keep the password in memory for sudo auto-fill (password auth only)
    if let AuthSpec::Password(pw) = &params.auth {
        *session.sudo_password.lock().unwrap() = Some(pw.clone());
    }
    let auto_sudo = Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::get_settings(&conn).ok())
        .map(|s| s.auto_sudo_password)
        .unwrap_or(true);

    let (resize_tx, mut resize_rx) = mpsc::channel::<(u32, u32)>(8);
    *session.resize_tx.lock().await = Some(resize_tx);

    emit_status(&state.app, session.id, "connected", None);

    // Auto-start any forwards bound to this host, now that it's reachable
    if session.host_id.is_some() {
        let auto: Vec<models::PortForward> = Connection::open(&state.db_path)
            .ok()
            .map(|conn| {
                db::list_forwards(&conn)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|f| f.auto_start && Some(f.host_id) == session.host_id)
                    .collect()
            })
            .unwrap_or_default();
        for f in auto {
            let st = state.clone();
            tokio::spawn(async move {
                let _ = forward_start(st, f).await;
            });
        }
    }

    // ---- pump output; service resizes; end on EOF/Close ----
    let mut output_tail = String::new();
    let mut sudo_fills: u32 = 0;
    let mut last_sudo_fill = Instant::now() - Duration::from_secs(10);

    loop {
        tokio::select! {
            _ = session.closed.cancelled() => break,
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        if auto_sudo && sudo_fills < MAX_SUDO_FILLS {
                            append_tail(&mut output_tail, &data);
                            if let Some(prompt_user) = detect_sudo_prompt(&output_tail) {
                                if last_sudo_fill.elapsed() >= Duration::from_secs(3)
                                    && prompt_matches(&prompt_user, &params.username)
                                {
                                    let filled = try_fill_sudo(session).await;
                                    if filled {
                                        sudo_fills += 1;
                                        last_sudo_fill = Instant::now();
                                        output_tail.clear();
                                        let _ = state.app.emit(
                                            "sudo:filled",
                                            SudoFilledEvent { session_id: session.id },
                                        );
                                    }
                                }
                            }
                        }
                        let _ = state.app.emit(
                            "ssh:stdout",
                            StdoutEvent { session_id: session.id, data: B64.encode(&data) },
                        );
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = state.app.emit(
                            "ssh:stdout",
                            StdoutEvent { session_id: session.id, data: B64.encode(&data) },
                        );
                    }
                    Some(russh::ChannelMsg::ExitStatus { .. }) => {}
                    Some(russh::ChannelMsg::Eof | russh::ChannelMsg::Close) | None => break,
                    _ => {}
                }
            }
            Some((cols, rows)) = resize_rx.recv() => {
                let _ = channel.window_change(cols, rows, 0, 0).await;
            }
        }
    }

    // Wipe the in-memory password when the session ends
    *session.sudo_password.lock().unwrap() = None;
    emit_status(&state.app, session.id, "closed", None);
    Ok(())
}

const MAX_SUDO_FILLS: u32 = 3;

fn append_tail(tail: &mut String, data: &[u8]) {
    tail.push_str(&String::from_utf8_lossy(data));
    if tail.len() > 400 {
        let cut = tail.len() - 400;
        // Don't split a UTF-8 sequence
        let cut = (cut..tail.len()).find(|i| tail.is_char_boundary(*i)).unwrap_or(tail.len());
        tail.drain(..cut);
    }
}

/// Detect a sudo/password prompt at the end of the terminal output.
/// Returns Some(Some(user)) when the prompt names its user,
/// Some(None) for a generic "Password:" prompt.
fn detect_sudo_prompt(tail: &str) -> Option<Option<String>> {
    let line = tail.rsplit(['\n', '\r']).next()?.trim();
    if !line.ends_with(':') {
        return None;
    }
    let lower = line.to_lowercase();
    if let Some(rest) = lower.strip_prefix("[sudo]") {
        let rest = rest.trim_start();
        if let Some(user) = rest.strip_prefix("password for ") {
            return Some(Some(user.trim_end_matches(':').trim().to_string()));
        }
        if let Some(idx) = rest.find("'s password") {
            return Some(Some(rest[..idx].trim().to_string()));
        }
        return Some(None);
    }
    if lower == "password:" {
        return Some(None);
    }
    None
}

/// Only auto-fill when the prompt's user is the account we logged in as —
/// a "password for root" prompt wants root's password, not ours.
fn prompt_matches(prompt_user: &Option<String>, session_user: &str) -> bool {
    match prompt_user {
        Some(u) => u.eq_ignore_ascii_case(session_user),
        None => true,
    }
}

async fn try_fill_sudo(session: &SshSession) -> bool {
    use tokio::io::AsyncWriteExt;

    let pw = session.sudo_password.lock().unwrap().clone();
    let Some(pw) = pw else { return false };
    let mut guard = session.writer.lock().await;
    let Some(writer) = guard.as_mut() else { return false };
    writer.write_all(format!("{pw}\n").as_bytes()).await.is_ok()
}

pub async fn disconnect_session(state: &AppState, session_id: u32) {
    let session = { state.sessions.lock().unwrap().get(&session_id).cloned() };
    if let Some(session) = session {
        session.closed.cancel();
        let mut guard = session.handle.lock().await;
        if let Some(handle) = guard.take() {
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "user disconnect", "en")
                .await;
        }
    }
    remove_session(state, session_id);
    emit_status(&state.app, session_id, "closed", None);
}

// ---------- SFTP ----------

pub async fn sftp_open(_state: &AppState, session: &SshSession) -> Result<(), String> {
    {
        let existing = session.sftp.lock().await;
        if existing.is_some() {
            return Ok(());
        }
    }
    let handle_guard = session.handle.lock().await;
    let handle = handle_guard
        .as_ref()
        .ok_or_else(|| "Not connected".to_string())?;

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| format!("Could not open SFTP channel: {e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("SFTP subsystem request failed: {e}"))?;

    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP init failed: {e}"))?;

    *session.sftp.lock().await = Some(sftp);
    Ok(())
}

fn entry_kind(ft: russh_sftp::protocol::FileType) -> &'static str {
    match ft {
        russh_sftp::protocol::FileType::Dir => "dir",
        russh_sftp::protocol::FileType::Symlink => "symlink",
        russh_sftp::protocol::FileType::File => "file",
        _ => "other",
    }
}

pub async fn sftp_list(
    session: &SshSession,
    path: &str,
) -> Result<Vec<models::FileEntry>, String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let entries = sftp
        .read_dir(path)
        .await
        .map_err(|e| format!("List failed: {e}"))?;
    Ok(entries
        .map(|e| {
            let meta = e.metadata();
            models::FileEntry {
                name: e.file_name(),
                kind: entry_kind(e.file_type()).to_string(),
                size: meta.len() as i64,
                mtime: meta.mtime.unwrap_or(0) as i64,
                permissions: meta.permissions.map(|p| p as i64),
            }
        })
        .collect())
}

pub async fn sftp_mkdir(session: &SshSession, path: &str) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    sftp.create_dir(path)
        .await
        .map_err(|e| format!("mkdir failed: {e}"))
}

pub async fn sftp_remove(session: &SshSession, path: &str, is_dir: bool) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let result = if is_dir {
        sftp.remove_dir(path).await
    } else {
        sftp.remove_file(path).await
    };
    result.map_err(|e| format!("Delete failed: {e}"))
}

pub async fn sftp_rename(session: &SshSession, from: &str, to: &str) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    sftp.rename(from, to)
        .await
        .map_err(|e| format!("Rename failed: {e}"))
}

#[allow(clippy::too_many_arguments)]
fn emit_transfer(
    app: &AppHandle,
    transfer_id: u32,
    transferred: u64,
    size: u64,
    done: bool,
    error: Option<String>,
    last_emit: &mut Instant,
    force: bool,
) {
    if !force && last_emit.elapsed() < Duration::from_millis(100) {
        return;
    }
    *last_emit = Instant::now();
    let _ = app.emit(
        "transfer:progress",
        TransferEvent {
            transfer_id,
            transferred,
            size,
            done,
            error,
        },
    );
}

pub async fn run_upload(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    transfer_id: u32,
    local: String,
    remote: String,
) {
    let cancel = CancellationToken::new();
    session
        .transfers
        .lock()
        .unwrap()
        .insert(transfer_id, cancel.clone());

    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let result = upload_inner(
        &session,
        &local,
        &remote,
        &cancel,
        transfer_id,
        &state.app,
        &mut last_emit,
    )
    .await;

    match result {
        Ok(size) => emit_transfer(&state.app, transfer_id, size, size, true, None, &mut last_emit, true),
        Err(e) => {
            let cancelled = cancel.is_cancelled();
            emit_transfer(
                &state.app,
                transfer_id,
                0,
                0,
                true,
                Some(if cancelled { "Cancelled".into() } else { e }),
                &mut last_emit,
                true,
            );
        }
    }
    session.transfers.lock().unwrap().remove(&transfer_id);
}

async fn upload_inner(
    session: &SshSession,
    local: &str,
    remote: &str,
    cancel: &CancellationToken,
    transfer_id: u32,
    app: &AppHandle,
    last_emit: &mut Instant,
) -> Result<u64, String> {
    use tokio::io::AsyncReadExt;

    let size = tokio::fs::metadata(local)
        .await
        .map_err(|e| format!("Local file error: {e}"))?
        .len();

    let mut local_file = tokio::fs::File::open(local)
        .await
        .map_err(|e| format!("Could not open local file: {e}"))?;

    {
        let guard = session.sftp.lock().await;
        let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
        let mut remote_file = sftp
            .create(remote)
            .await
            .map_err(|e| format!("Could not create remote file: {e}"))?;

        let mut buf = vec![0u8; TRANSFER_CHUNK];
        let mut transferred: u64 = 0;
        loop {
            if cancel.is_cancelled() {
                return Err("cancelled".into());
            }
            let n = local_file
                .read(&mut buf)
                .await
                .map_err(|e| format!("Local read failed: {e}"))?;
            if n == 0 {
                break;
            }
            remote_file
                .write_all(&buf[..n])
                .await
                .map_err(|e| format!("Remote write failed: {e}"))?;
            transferred += n as u64;
            emit_transfer(app, transfer_id, transferred, size, false, None, last_emit, false);
        }
        let _ = remote_file.shutdown().await;
    }
    Ok(size)
}

pub async fn run_download(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    transfer_id: u32,
    remote: String,
    local: String,
) {
    let cancel = CancellationToken::new();
    session
        .transfers
        .lock()
        .unwrap()
        .insert(transfer_id, cancel.clone());

    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let result = download_inner(
        &session,
        &remote,
        &local,
        &cancel,
        transfer_id,
        &state.app,
        &mut last_emit,
    )
    .await;

    match result {
        Ok(size) => emit_transfer(&state.app, transfer_id, size, size, true, None, &mut last_emit, true),
        Err(e) => {
            let cancelled = cancel.is_cancelled();
            emit_transfer(
                &state.app,
                transfer_id,
                0,
                0,
                true,
                Some(if cancelled { "Cancelled".into() } else { e }),
                &mut last_emit,
                true,
            );
        }
    }
    session.transfers.lock().unwrap().remove(&transfer_id);
}

async fn download_inner(
    session: &SshSession,
    remote: &str,
    local: &str,
    cancel: &CancellationToken,
    transfer_id: u32,
    app: &AppHandle,
    last_emit: &mut Instant,
) -> Result<u64, String> {
    use tokio::io::AsyncReadExt;

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;

    let meta = sftp
        .metadata(remote)
        .await
        .map_err(|e| format!("Remote stat failed: {e}"))?;
    let size = meta.len();

    let mut remote_file = sftp
        .open(remote)
        .await
        .map_err(|e| format!("Could not open remote file: {e}"))?;
    let mut local_file = tokio::fs::File::create(local)
        .await
        .map_err(|e| format!("Could not create local file: {e}"))?;

    let mut buf = vec![0u8; TRANSFER_CHUNK];
    let mut transferred: u64 = 0;
    loop {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let n = remote_file
            .read(&mut buf)
            .await
            .map_err(|e| format!("Remote read failed: {e}"))?;
        if n == 0 {
            break;
        }
        local_file
            .write_all(&buf[..n])
            .await
            .map_err(|e| format!("Local write failed: {e}"))?;
        transferred += n as u64;
        emit_transfer(app, transfer_id, transferred, size, false, None, last_emit, false);
    }
    local_file
        .flush()
        .await
        .map_err(|e| format!("Local flush failed: {e}"))?;
    Ok(transferred)
}

pub fn cancel_transfer(state: &AppState, session_id: u32, transfer_id: u32) -> bool {
    let session = { state.sessions.lock().unwrap().get(&session_id).cloned() };
    match session {
        Some(s) => {
            if let Some(token) = s.transfers.lock().unwrap().get(&transfer_id) {
                token.cancel();
                true
            } else {
                false
            }
        }
        None => false,
    }
}

// ---------- port forwarding ----------

pub async fn forward_start(
    state: Arc<AppState>,
    forward: models::PortForward,
) -> Result<(), String> {
    {
        let fw = state.forwards.lock().unwrap();
        if fw.contains_key(&forward.id) {
            return Ok(()); // already running
        }
    }

    let session = {
        let sessions = state.sessions.lock().unwrap();
        sessions
            .values()
            .find(|s| s.host_id == Some(forward.host_id))
            .cloned()
    };
    let session = session
        .ok_or("Not connected to this host yet — connect first, then start the tunnel")?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", forward.local_port as u16))
        .await
        .map_err(|e| format!("Could not bind local port {}: {e}", forward.local_port))?;

    let cancel = CancellationToken::new();
    let state2 = state.clone();
    let fw_id = forward.id;
    let remote_host = forward.remote_host.clone();
    let remote_port = forward.remote_port as u32;
    let task_cancel = cancel.clone();

    let listener_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = task_cancel.cancelled() => break,
                accepted = listener.accept() => {
                    let Ok((tcp, peer)) = accepted else { break };
                    let res = pump_connection(
                        session.clone(),
                        tcp,
                        peer,
                        remote_host.clone(),
                        remote_port,
                    )
                    .await;
                    if res.is_err() {
                        // The SSH connection is gone — stop the tunnel and tell the UI
                        break;
                    }
                }
            }
        }
        state2.forwards.lock().unwrap().remove(&fw_id);
        let _ = state2
            .app
            .emit("forward:stopped", ForwardEvent { id: fw_id, active: false });
    });

    state.forwards.lock().unwrap().insert(
        forward.id,
        ForwardHandle {
            cancel,
            listener: listener_task,
        },
    );
    let _ = state
        .app
        .emit("forward:started", ForwardEvent { id: forward.id, active: true });
    Ok(())
}

async fn pump_connection(
    session: Arc<SshSession>,
    mut tcp: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    remote_host: String,
    remote_port: u32,
) -> Result<(), String> {
    let handle_guard = session.handle.lock().await;
    let handle = handle_guard
        .as_ref()
        .ok_or_else(|| "Not connected".to_string())?;
    let channel = handle
        .channel_open_direct_tcpip(
            remote_host,
            remote_port,
            peer.ip().to_string(),
            peer.port() as u32,
        )
        .await
        .map_err(|e| format!("Forward channel failed: {e}"))?;
    drop(handle_guard);

    let mut remote_stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut remote_stream).await;
    Ok(())
}

pub async fn forward_stop(state: Arc<AppState>, forward_id: i64) {
    let handle = state.forwards.lock().unwrap().remove(&forward_id);
    if let Some(handle) = handle {
        handle.cancel.cancel();
        handle.listener.abort();
    }
    let _ = state
        .app
        .emit("forward:stopped", ForwardEvent { id: forward_id, active: false });
}

pub fn list_active_forwards(state: &AppState) -> Vec<i64> {
    state.forwards.lock().unwrap().keys().copied().collect()
}
