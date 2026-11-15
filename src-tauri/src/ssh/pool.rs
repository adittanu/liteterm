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
/// How long a dial may sit on the "verify this fingerprint" prompt. The prompt
/// is part of the dial, so the plain 15 s limit turned a slow reader's first
/// connect into a bogus "timed out" error. A person gets minutes; a dead
/// network or a port that never speaks SSH still gets 15 s, because the
/// longer limit only starts once the prompt actually appears.
const HOST_KEY_PROMPT_TIMEOUT_SECS: u64 = 300;
const TRANSFER_CHUNK: usize = 32 * 1024;

/// An `Instant` that is `age` in the past — or just now, if the clock has not
/// run that long yet. (`Instant::now() - age` panics when the machine booted
/// less than `age` ago, and the release build aborts on a panic.)
fn instant_ago(age: Duration) -> Instant {
    Instant::now().checked_sub(age).unwrap_or_else(Instant::now)
}

/// A release build pushes PTY bytes much faster than `tauri dev`. One IPC
/// event per SSH packet starves the WebView frame loop; xterm then drops
/// its paint and a full-screen TUI (opencode, and the same path for other
/// chatty tools) stays blank or half-drawn. Debug builds emit slowly
/// enough that the frame still lands. Coalesce to one event per flush.
const STDOUT_FLUSH: Duration = Duration::from_millis(8);
const STDOUT_FLUSH_BYTES: usize = 32 * 1024;

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
    /// Transfer ids are unique across sessions: the progress event carries
    /// only the id, so per-session counters made two servers' transfers
    /// overwrite each other's rows in the UI
    pub next_transfer_id: AtomicU32,
    pub sessions: Mutex<HashMap<u32, Arc<SshSession>>>,
    pub local_sessions: Mutex<HashMap<u32, Arc<crate::local::LocalSession>>>,
    pub pending_hostkey: Mutex<HashMap<u32, mpsc::Sender<HostKeyDecision>>>,
    pub forwards: Mutex<HashMap<i64, ForwardHandle>>,
    /// Remote forwards (-R) the servers listen for, by (session id, server
    /// port): where a connection the server hands back goes on this side
    pub remote_targets: Mutex<HashMap<(u32, u32), RemoteTarget>>,
    /// One lock per forward id, held while that forward starts
    pub forward_starts: Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
}

pub struct HostKeyDecision {
    pub accept: bool,
    pub remember: bool,
}

pub struct ForwardHandle {
    pub cancel: CancellationToken,
    pub listener: tokio::task::JoinHandle<()>,
    /// The SSH session this tunnel rides on; it ends when that session does
    pub session_id: u32,
    /// Remote forwards: the port the server listens on. A reconnect is a
    /// new transport, so the request is sent again on it.
    pub remote_port: Option<u32>,
}

#[derive(Clone)]
pub struct RemoteTarget {
    pub forward_id: i64,
    pub host: String,
    pub port: u16,
    /// Ends the connections already open when the forward stops
    pub cancel: CancellationToken,
    /// Which start put it here, so a failed or stale one never removes
    /// another start's target
    pub start: u64,
}

/// The address a remote forward asks the server to listen on. Loopback, as
/// `ssh -R port:host:hostport` does without a bind address.
const REMOTE_BIND: &str = "localhost";

type Shared<T> = Arc<tokio::sync::Mutex<T>>;
type ChannelWriter = Box<dyn tokio::io::AsyncWrite + Unpin + Send>;

pub struct SshSession {
    pub id: u32,
    pub host_id: Option<i64>,
    #[allow(dead_code)] // kept for future logging/forward naming
    pub label: String,
    #[allow(dead_code)]
    pub address: String,
    /// A read lock for what russh lets several callers do at once (opening
    /// channels), the write lock for the rest. A slow channel open, such as a
    /// SOCKS request to an unreachable address, then no longer holds up every
    /// other open on the session.
    pub handle: Arc<tokio::sync::RwLock<Option<russh::client::Handle<ClientHandler>>>>,
    pub writer: Shared<Option<ChannelWriter>>,
    /// cols, rows, pixel width, pixel height
    pub resize_tx: Shared<Option<mpsc::Sender<(u32, u32, u32, u32)>>>,
    pub sftp: Arc<tokio::sync::Mutex<Option<russh_sftp::client::SftpSession>>>,
    /// The key-mismatch path reports its own detailed error; this flag
    /// tells run_session not to stack a generic second one
    pub key_mismatch: Arc<std::sync::atomic::AtomicBool>,
    /// Kept in memory only, for auto-filling sudo prompts
    pub sudo_password: Arc<Mutex<Option<String>>>,
    /// session_logs row for this connection (SSH only)
    pub log_id: Mutex<Option<i64>>,
    pub closed: CancellationToken,
    pub transfers: Mutex<HashMap<u32, CancellationToken>>,
    /// Skips the backoff wait while auto-reconnecting ("Reconnect now")
    pub reconnect_now: tokio::sync::Notify,
    /// Cancels the remote-forward retries of the previous reconnect
    pub renewals: Mutex<CancellationToken>,
}

// ---------- events ----------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StatusEvent {
    pub session_id: u32,
    pub status: String,
    pub error: Option<String>,
    /// "reconnecting" only: 1-based attempt number and its cap
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    /// "reconnecting" only: wait before this attempt starts (0 = dialing now)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_in_ms: Option<u64>,
    /// "closed" only: "exit" (shell ended) | "lost" (connection dropped)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
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
    /// Folders only: files copied so far, of how many
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_done: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_total: Option<u32>,
    /// Folders only: entries left out (links to folders, devices)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<u32>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForwardEvent {
    pub id: i64,
    pub active: bool,
    /// Stopped events only: why the app stopped it by itself
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SudoFilledEvent {
    pub session_id: u32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HostOsEvent {
    pub host_id: i64,
    pub os: String,
}

/// Run a quiet detection command on its own exec channel (the interactive
/// PTY is untouched) and parse the distro id.
async fn detect_os(
    handle: &russh::client::Handle<ClientHandler>,
) -> Option<String> {
    let mut channel = tokio::time::timeout(
        Duration::from_secs(3),
        handle.channel_open_session(),
    )
    .await
    .ok()?
    .ok()?;
    channel
        .exec(true, "cat /etc/os-release 2>/dev/null; echo; uname -s")
        .await
        .ok()?;
    let mut out = String::new();
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(3), channel.wait()).await;
        match msg {
            Ok(Some(russh::ChannelMsg::Data { data })) => {
                out.push_str(&String::from_utf8_lossy(&data));
                if out.len() > 1024 {
                    break;
                }
            }
            Ok(Some(russh::ChannelMsg::Eof | russh::ChannelMsg::Close))
            | Ok(None)
            | Err(_) => break,
            Ok(Some(_)) => {}
        }
    }
    Some(parse_os(&out))
}

fn parse_os(output: &str) -> String {
    let lower = output.to_lowercase();
    if lower.contains("darwin") {
        return "macos".into();
    }
    if lower.contains("windows") || lower.contains("mingw") {
        return "windows".into();
    }
    // Prefer the distro ID line: "ID=ubuntu" (also matches ID_LIKE)
    for line in lower.lines() {
        if let Some(rest) = line.strip_prefix("id_like=") {
            let v = rest.trim_matches('"').trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    for line in lower.lines() {
        if let Some(rest) = line.strip_prefix("id=") {
            let v = rest.trim_matches('"').trim();
            if !v.is_empty() {
                return v.to_string();
            }
        }
    }
    if lower.contains("linux") {
        return "linux".into();
    }
    "unknown".into()
}

fn emit_pending_stdout(app: &AppHandle, session_id: u32, pending: &mut Vec<u8>) {
    if pending.is_empty() {
        return;
    }
    crate::trace::note(session_id, "EMIT", || pending.len().to_string());
    // A dropped chunk corrupts the screen, and the release build has no
    // console to say so — leave a (throttled) line in the app log instead
    if let Err(e) = app.emit(
        "ssh:stdout",
        StdoutEvent {
            session_id,
            data: B64.encode(&pending[..]),
        },
    ) {
        crate::applog::write_throttled(
            "ssh stdout emit",
            &format!("could not deliver terminal output of session {session_id}: {e}"),
        );
    }
    pending.clear();
}

/// Append PTY bytes and flush immediately once the buffer is a full frame.
/// The caller arms `flush_at` so a short interactive chunk is not held
/// until the next packet.
fn queue_stdout(
    app: &AppHandle,
    session_id: u32,
    pending: &mut Vec<u8>,
    flush_at: &mut Option<tokio::time::Instant>,
    data: &[u8],
) {
    if data.is_empty() {
        return;
    }
    let was_empty = pending.is_empty();
    pending.extend_from_slice(data);
    if pending.len() >= STDOUT_FLUSH_BYTES
        || flush_at.is_some_and(|at| tokio::time::Instant::now() >= at)
    {
        emit_pending_stdout(app, session_id, pending);
        *flush_at = None;
    } else if was_empty {
        *flush_at = Some(tokio::time::Instant::now() + STDOUT_FLUSH);
    }
}

pub fn emit_status(app: &AppHandle, session_id: u32, status: &str, error: Option<String>) {
    let _ = app.emit(
        "session:status",
        StatusEvent {
            session_id,
            status: status.into(),
            error,
            attempt: None,
            max_attempts: None,
            retry_in_ms: None,
            reason: None,
        },
    );
}

fn emit_closed(app: &AppHandle, session_id: u32, reason: &str, error: Option<String>) {
    let _ = app.emit(
        "session:status",
        StatusEvent {
            session_id,
            status: "closed".into(),
            error,
            attempt: None,
            max_attempts: None,
            retry_in_ms: None,
            reason: Some(reason.into()),
        },
    );
}

fn emit_reconnecting(app: &AppHandle, session_id: u32, attempt: u32, retry_in: Duration) {
    let _ = app.emit(
        "session:status",
        StatusEvent {
            session_id,
            status: "reconnecting".into(),
            error: None,
            attempt: Some(attempt),
            max_attempts: Some(MAX_RECONNECT_ATTEMPTS),
            retry_in_ms: Some(retry_in.as_millis() as u64),
            reason: None,
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
    /// Signalled when the handshake starts waiting for the user to verify a
    /// host key, so the dial's deadline can move out to a human's pace
    pub prompting: Arc<tokio::sync::Notify>,
    /// This hop asked to forward the agent (the target hop of a host with
    /// agent forwarding on); any other refuses agent channels
    pub agent_forward: bool,
    /// Open agent channels: where the server's bytes for each go
    pub agent_channels: HashMap<russh::ChannelId, mpsc::UnboundedSender<Vec<u8>>>,
    /// Cancelled when this handler is dropped, which happens when its
    /// connection ends. Agent channels stop then, before a reconnect could
    /// reuse their channel numbers on the new connection.
    pub alive: CancellationToken,
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        self.alive.cancel();
    }
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

        // Trust on first use — ask the user, unless the session was closed
        // while this dial was in flight (nobody is there to answer)
        if !self.state.sessions.lock().unwrap().contains_key(&self.session_id) {
            return Ok(false);
        }
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
        // The dial is about to wait on a person, not on the network
        self.prompting.notify_one();

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

    /// A connection to a port this client asked the server to listen on
    /// (a remote forward). Look up where it goes on this side and connect
    /// it there; one unknown to us is closed.
    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<russh::client::Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        let target = self
            .state
            .remote_targets
            .lock()
            .unwrap()
            .get(&(self.session_id, connected_port))
            .cloned();
        // Never wait here: this runs inside the SSH session's own loop
        tokio::spawn(async move {
            let Some(target) = target else {
                let _ = channel.close().await;
                return;
            };
            let tcp = tokio::select! {
                _ = target.cancel.cancelled() => None,
                tcp = tokio::net::TcpStream::connect((target.host.as_str(), target.port)) => tcp.ok(),
            };
            let Some(mut tcp) = tcp else {
                crate::applog::write_throttled(
                    "remote forward connect",
                    &format!(
                        "forward {}: could not reach {}:{} for a connection from the server",
                        target.forward_id, target.host, target.port
                    ),
                );
                let _ = channel.close().await;
                return;
            };
            let mut stream = channel.into_stream();
            tokio::select! {
                _ = target.cancel.cancelled() => {}
                _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream) => {}
            }
        });
        Ok(())
    }

    /// A program on the server (ssh, git) wants to use this computer's SSH
    /// agent. Only honoured on a connection that asked for agent forwarding:
    /// a server must not reach the agent just by opening the channel.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::ChannelId,
        session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        if !self.agent_forward {
            session.close(channel);
            return Ok(());
        }
        // A local pipe or socket: connecting is immediate, so it is done
        // here, and a missing agent closes the channel at once instead of
        // leaving the program on the server waiting for a reply
        match open_agent_stream().await {
            Ok(agent) => {
                let (tx, rx) = mpsc::unbounded_channel();
                self.agent_channels.insert(channel, tx);
                tokio::spawn(pump_agent(self.state.clone(), self.session_id, channel, agent, rx, self.alive.clone()));
            }
            Err(e) => {
                crate::applog::write_throttled("agent forward", &format!("agent forwarding: {e}"));
                session.close(channel);
            }
        }
        Ok(())
    }

    /// Bytes on a channel the server opened. Only agent channels are ours to
    /// route; the rest (the shell, SFTP) have their own Channel objects.
    async fn data(
        &mut self,
        channel: russh::ChannelId,
        data: &[u8],
        session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        if let Some(tx) = self.agent_channels.get(&channel) {
            if tx.send(data.to_vec()).is_err() {
                // The local agent went away
                self.agent_channels.remove(&channel);
                session.close(channel);
            }
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.agent_channels.remove(&channel);
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: russh::ChannelId,
        _session: &mut russh::client::Session,
    ) -> Result<(), Self::Error> {
        self.agent_channels.remove(&channel);
        Ok(())
    }
}

type AgentStream = Box<dyn AgentIo>;
trait AgentIo: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AgentIo for T {}

/// The Windows agent pipe: SSH_AUTH_SOCK when it names a pipe (some agent
/// apps set it), otherwise the Windows OpenSSH agent's well-known pipe,
/// which 1Password / Bitwarden / KeePassXC can also serve
#[cfg(windows)]
fn windows_agent_pipe() -> String {
    std::env::var("SSH_AUTH_SOCK")
        .ok()
        .filter(|p| p.starts_with(r"\\.\pipe\") || p.starts_with("//./pipe/"))
        .unwrap_or_else(|| r"\\.\pipe\openssh-ssh-agent".to_string())
}

/// A raw connection to this computer's SSH agent, for a forwarded channel
async fn open_agent_stream() -> Result<AgentStream, String> {
    #[cfg(windows)]
    {
        let path = windows_agent_pipe();
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(&path)
            .map_err(|e| format!("no SSH agent at {path}: {e}"))?;
        Ok(Box::new(pipe))
    }
    #[cfg(unix)]
    {
        let path = std::env::var_os("SSH_AUTH_SOCK").ok_or("SSH_AUTH_SOCK is not set")?;
        let sock = tokio::net::UnixStream::connect(&path)
            .await
            .map_err(|e| format!("no SSH agent at {}: {e}", std::path::Path::new(&path).display()))?;
        Ok(Box::new(sock))
    }
}

/// Carry one forwarded agent channel: what the server sends goes to the
/// agent, and the agent's replies go back on the same channel
async fn pump_agent(
    state: Arc<AppState>,
    session_id: u32,
    channel: russh::ChannelId,
    agent: AgentStream,
    mut from_server: mpsc::UnboundedReceiver<Vec<u8>>,
    alive: CancellationToken,
) {
    use tokio::io::AsyncReadExt;

    let Some(session) = state.sessions.lock().unwrap().get(&session_id).cloned() else { return };
    let (mut agent_rd, mut agent_wr) = tokio::io::split(agent);
    let to_agent = async {
        while let Some(chunk) = from_server.recv().await {
            if agent_wr.write_all(&chunk).await.is_err() {
                break;
            }
        }
    };
    let to_server = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = match agent_rd.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            // The shell stores the handle right after asking for it; a
            // .bashrc that uses the agent may get here first
            let mut sent = false;
            for _ in 0..50 {
                if alive.is_cancelled() {
                    break;
                }
                if let Some(handle) = session.handle.read().await.as_ref() {
                    sent = handle.data(channel, russh::CryptoVec::from_slice(&buf[..n])).await.is_ok();
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            if !sent {
                break;
            }
        }
    };
    tokio::select! {
        _ = to_agent => {}
        _ = to_server => {}
        _ = alive.cancelled() => {}
        _ = session.closed.cancelled() => {}
    }
}

// ---------- connection lifecycle ----------

#[derive(Clone)]
pub enum AuthSpec {
    Password(String),
    Key {
        path: String,
        passphrase: Option<String>,
    },
    /// Keys held by the running SSH agent (Windows OpenSSH agent pipe,
    /// or whatever SSH_AUTH_SOCK names — 1Password, Bitwarden, KeePassXC)
    Agent,
}

pub struct ConnParams {
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthSpec,
    /// Saved-host label, used to name the hop in errors
    pub label: String,
    /// Bastion to tunnel through first (ProxyJump); may itself hop further
    pub jump: Option<Box<ConnParams>>,
}

/// Bastions in a jump chain beyond this are almost certainly a mistake
pub const MAX_JUMP_DEPTH: usize = 4;

const MAX_RECONNECT_ATTEMPTS: u32 = 12;

/// 1s, 2s, 4s, 8s, 15s, then every 30s — about 3½ minutes in total, enough
/// to ride out a Wi-Fi switch or a laptop waking up.
fn reconnect_delay(attempt: u32) -> Duration {
    const STEPS: [u64; 6] = [1, 2, 4, 8, 15, 30];
    let i = (attempt.saturating_sub(1) as usize).min(STEPS.len() - 1);
    Duration::from_secs(STEPS[i])
}

/// How a connected session ended
#[derive(Debug)]
enum StreamEnd {
    /// disconnect_session() cancelled it
    UserClosed,
    /// The remote shell exited (`exit`, logout) — the server closed the channel
    RemoteExit,
    /// The transport died without a channel close: network drop, sleep,
    /// keepalive timeout, server gone
    Lost,
}

/// A failure before the session became interactive. `retryable` marks
/// transport problems that may heal (network down, timeouts) as opposed to
/// auth or host-key failures that never will.
struct ConnectError {
    msg: String,
    retryable: bool,
}

impl ConnectError {
    fn fatal(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), retryable: false }
    }
    fn transient(msg: impl Into<String>) -> Self {
        Self { msg: msg.into(), retryable: true }
    }
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
        log_id: Mutex::new(None),
        closed: CancellationToken::new(),
        transfers: Mutex::new(HashMap::new()),
        reconnect_now: tokio::sync::Notify::new(),
        renewals: Mutex::new(CancellationToken::new()),
    };
    state.sessions.lock().unwrap().insert(id, Arc::new(session));
    id
}

pub fn remove_session(state: &AppState, session_id: u32) {
    if let Some(session) = state.sessions.lock().unwrap().remove(&session_id) {
        session.closed.cancel();
    }
}

/// Wake a session waiting out its reconnect backoff so it dials right away.
pub fn reconnect_now(state: &AppState, session_id: u32) -> Result<(), String> {
    let session = state
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .ok_or("Session vanished")?;
    // Wake only a pending backoff wait — no stored permit, so a click that
    // lands while a dial is running can't skip the next wait
    session.reconnect_now.notify_waiters();
    Ok(())
}

fn auto_reconnect_enabled(state: &AppState) -> bool {
    Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::get_settings(&conn).ok())
        .map(|s| s.auto_reconnect)
        .unwrap_or(true)
}

/// Forget everything tied to the dead transport so writes, SFTP and
/// tunnels fail fast instead of talking to a closed connection.
async fn reset_transport(session: &SshSession) {
    *session.writer.lock().await = None;
    *session.resize_tx.lock().await = None;
    *session.sftp.lock().await = None;
    if let Some(handle) = session.handle.write().await.take() {
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
    *session.sudo_password.lock().unwrap() = None;
}

/// Owns a session from first dial to final close. The same session id —
/// and so the same tab and terminal scrollback — survives reconnects.
pub async fn run_session(state: Arc<AppState>, session: Arc<SshSession>, params: ConnParams) {
    let host_tag = format!("{}:{}", params.address, params.port);
    let mut attempt: u32 = 0;
    let mut ever_connected = false;

    loop {
        let result = connect_and_stream(&state, &session, &params, &host_tag).await;
        reset_transport(&session).await;

        if session.closed.is_cancelled() {
            // disconnect_session already told the UI
            break;
        }

        match result {
            Ok(StreamEnd::UserClosed) => break,
            Ok(StreamEnd::RemoteExit) => {
                emit_closed(&state.app, session.id, "exit", None);
                break;
            }
            Ok(StreamEnd::Lost) => {
                ever_connected = true;
                attempt = 0;
                if !auto_reconnect_enabled(&state) {
                    emit_closed(&state.app, session.id, "lost", Some("Connection lost".into()));
                    break;
                }
            }
            Err(e) => {
                // Only a session that was live before keeps retrying; a first
                // connect that fails reports straight away, and auth/host-key
                // failures never heal by waiting
                if !(ever_connected && e.retryable) {
                    if !session.key_mismatch.load(Ordering::SeqCst) {
                        emit_status(&state.app, session.id, "error", Some(e.msg));
                    }
                    break;
                }
            }
        }

        attempt += 1;
        if attempt > MAX_RECONNECT_ATTEMPTS {
            emit_status(
                &state.app,
                session.id,
                "error",
                Some(format!(
                    "Connection lost — gave up after {MAX_RECONNECT_ATTEMPTS} reconnect attempts"
                )),
            );
            break;
        }

        let delay = reconnect_delay(attempt);
        emit_reconnecting(&state.app, session.id, attempt, delay);
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            _ = session.reconnect_now.notified() => {}
            _ = session.closed.cancelled() => break,
        }
        if session.closed.is_cancelled() {
            break;
        }
        emit_reconnecting(&state.app, session.id, attempt, Duration::ZERO);
    }

    stop_forwards_for_session(&state, session.id);
    remove_session(&state, session.id);
}

/// Answer the server's auth challenge for one hop.
async fn authenticate(
    handle: &mut russh::client::Handle<ClientHandler>,
    hop: &ConnParams,
) -> Result<bool, ConnectError> {
    match &hop.auth {
        AuthSpec::Password(pw) => {
            let by_password = handle
                .authenticate_password(&hop.username, pw)
                .await
                .map_err(|e| ConnectError::transient(format!("Auth error: {e}")))?;
            // Many servers (shared hosts like Hostinger, some cloud images)
            // disable the "password" method but accept the same secret via
            // keyboard-interactive. Answer its prompts with the password.
            if by_password {
                Ok(true)
            } else {
                Ok(authenticate_keyboard_interactive(handle, &hop.username, pw).await)
            }
        }
        AuthSpec::Key { path, passphrase } => {
            let key = russh::keys::load_secret_key(path, passphrase.as_deref())
                .map_err(|e| ConnectError::fatal(format!("Could not load key: {e}")))?;
            handle
                .authenticate_publickey(&hop.username, Arc::new(key))
                .await
                .map_err(|e| ConnectError::transient(format!("Auth error: {e}")))
        }
        AuthSpec::Agent => authenticate_agent(handle, &hop.username).await,
    }
}

/// Offer each identity the agent holds until the server accepts one. The
/// private keys never leave the agent; it only signs the challenge.
/// A missing or empty agent is transient: after sleep, KeePassXC/Bitwarden
/// hold no keys until their vault is unlocked again — worth retrying for a
/// session that was live (a first connect still reports it straight away).
async fn authenticate_agent(
    handle: &mut russh::client::Handle<ClientHandler>,
    username: &str,
) -> Result<bool, ConnectError> {
    let mut agent = connect_agent().await.map_err(ConnectError::transient)?;
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| ConnectError::transient(format!("SSH agent error: {e}")))?;
    if identities.is_empty() {
        return Err(ConnectError::transient(
            "The SSH agent has no keys loaded — add one with ssh-add, or unlock your vault",
        ));
    }
    for key in identities {
        let (returned, result) = handle.authenticate_future(username, key, agent).await;
        agent = returned;
        match result {
            Ok(true) => return Ok(true),
            Ok(false) => continue,
            Err(e) => return Err(ConnectError::transient(format!("SSH agent signing failed: {e}"))),
        }
    }
    Ok(false)
}

#[cfg(windows)]
async fn connect_agent() -> Result<
    russh::keys::agent::client::AgentClient<tokio::net::windows::named_pipe::NamedPipeClient>,
    String,
> {
    let path = windows_agent_pipe();
    let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(&path)
        .map_err(|e| {
            format!(
                "No SSH agent at {path} ({e}). Start the “OpenSSH Authentication Agent” \
                 Windows service, or enable the SSH agent in 1Password / Bitwarden / KeePassXC."
            )
        })?;
    Ok(russh::keys::agent::client::AgentClient::connect(pipe))
}

#[cfg(unix)]
async fn connect_agent() -> Result<russh::keys::agent::client::AgentClient<tokio::net::UnixStream>, String> {
    russh::keys::agent::client::AgentClient::connect_env()
        .await
        .map_err(|e| format!("No SSH agent (SSH_AUTH_SOCK): {e}"))
}

/// Drive `work` to completion, giving up with `None` after `base` — unless
/// `extend` is signalled first, which moves the deadline to `extended` from
/// that moment. (`base` is the network's allowance, `extended` a person's.)
async fn with_extendable_deadline<F: std::future::Future>(
    work: F,
    base: Duration,
    extended: Duration,
    extend: &tokio::sync::Notify,
) -> Option<F::Output> {
    tokio::pin!(work);
    let deadline = tokio::time::sleep(base);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            output = &mut work => return Some(output),
            _ = &mut deadline => return None,
            _ = extend.notified() => {
                deadline.as_mut().reset(tokio::time::Instant::now() + extended);
            }
        }
    }
}

/// Connect and authenticate one hop — over TCP, or through `via` (the
/// previous bastion) as a direct-tcpip tunnel. Each hop verifies its own
/// host key under its own `addr:port` tag.
async fn open_hop(
    state: &Arc<AppState>,
    session: &Arc<SshSession>,
    hop: &ConnParams,
    via: Option<&russh::client::Handle<ClientHandler>>,
    config: Arc<russh::client::Config>,
    agent_forward: bool,
) -> Result<russh::client::Handle<ClientHandler>, ConnectError> {
    let host_tag = format!("{}:{}", hop.address, hop.port);
    let known_keys = Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::get_known_hosts(&conn, &host_tag).ok())
        .unwrap_or_default();

    let prompting = Arc::new(tokio::sync::Notify::new());
    let handler = ClientHandler {
        host_tag,
        session_id: session.id,
        known_keys,
        key_mismatch: session.key_mismatch.clone(),
        state: state.clone(),
        prompting: prompting.clone(),
        agent_forward,
        agent_channels: HashMap::new(),
        alive: CancellationToken::new(),
    };

    let dial = async {
        match via {
            None => russh::client::connect(config, (hop.address.as_str(), hop.port), handler).await,
            Some(bastion) => {
                let channel = bastion
                    .channel_open_direct_tcpip(hop.address.clone(), hop.port as u32, "127.0.0.1", 0)
                    .await?;
                russh::client::connect_stream(config, channel.into_stream(), handler).await
            }
        }
    };

    // A dead network (or a port that never says "SSH-2.0") gets CONNECT_TIMEOUT_SECS.
    // The host-key prompt is part of the dial, though: once it appears the
    // deadline moves out to HOST_KEY_PROMPT_TIMEOUT_SECS, so the time spent
    // reading a fingerprint is not charged to the connection.
    let dialed = with_extendable_deadline(
        dial,
        Duration::from_secs(CONNECT_TIMEOUT_SECS),
        Duration::from_secs(HOST_KEY_PROMPT_TIMEOUT_SECS),
        &prompting,
    )
    .await;

    let mut handle = dialed
        .ok_or_else(|| {
            ConnectError::transient(format!(
                "Connection timed out after {CONNECT_TIMEOUT_SECS}s — check the address and port"
            ))
        })?
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("UnknownKey") || s.contains("unknown key") || s.contains("Unknown server key") {
                ConnectError::fatal("Host key rejected")
            } else if via.is_some() && matches!(e, russh::Error::ChannelOpenFailure(_)) {
                // The bastion refused to open the tunnel to this hop
                ConnectError::transient(format!(
                    "The jump host could not reach {}:{} ({s})",
                    hop.address, hop.port
                ))
            } else {
                ConnectError::transient(format!("Connection failed: {s}"))
            }
        })?;

    if !authenticate(&mut handle, hop).await? {
        return Err(ConnectError::fatal("Authentication failed: check username and credentials"));
    }
    Ok(handle)
}

/// Dial the whole jump chain, outermost bastion first. Returns the target
/// handle plus the bastion handles that carry it — they must outlive it.
async fn open_chain(
    state: &Arc<AppState>,
    session: &Arc<SshSession>,
    params: &ConnParams,
    config: Arc<russh::client::Config>,
    agent_forward: bool,
) -> Result<(russh::client::Handle<ClientHandler>, Vec<russh::client::Handle<ClientHandler>>), ConnectError> {
    let mut hops: Vec<&ConnParams> = Vec::new();
    let mut cur = Some(params);
    while let Some(p) = cur {
        hops.push(p);
        cur = p.jump.as_deref();
    }
    hops.reverse();

    let mut bastions: Vec<russh::client::Handle<ClientHandler>> = Vec::new();
    let last = hops.len() - 1;
    for (i, hop) in hops.iter().enumerate() {
        // Only the target hop forwards the agent: the bastions just carry it
        match open_hop(state, session, hop, bastions.last(), config.clone(), agent_forward && i == last).await {
            Ok(handle) if i == last => return Ok((handle, bastions)),
            Ok(handle) => bastions.push(handle),
            Err(mut e) => {
                if i != last {
                    e.msg = format!("Jump host “{}”: {}", hop.label, e.msg);
                }
                close_handles(bastions).await;
                return Err(e);
            }
        }
    }
    unreachable!("a chain always ends at the target hop")
}

/// Innermost first, so each tunnel closes before the hop carrying it
async fn close_handles(handles: Vec<russh::client::Handle<ClientHandler>>) {
    for handle in handles.into_iter().rev() {
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

async fn connect_and_stream(
    state: &Arc<AppState>,
    session: &Arc<SshSession>,
    params: &ConnParams,
    host_tag: &str,
) -> Result<StreamEnd, ConnectError> {
    let config = Arc::new(russh::client::Config {
        keepalive_interval: Some(Duration::from_secs(KEEPALIVE_SECS)),
        ..Default::default()
    });

    // Read on every attempt, so a reconnect follows an edit made meanwhile
    let agent_forward = session.host_id.is_some_and(|id| {
        Connection::open(&state.db_path)
            .ok()
            .and_then(|conn| db::host_agent_forwarding(&conn, id).ok())
            .unwrap_or(false)
    });
    let (handle, bastions) = open_chain(state, session, params, config, agent_forward).await?;
    let result = stream_session(state, session, params, host_tag, handle, agent_forward).await;
    close_handles(bastions).await;
    result
}

async fn stream_session(
    state: &Arc<AppState>,
    session: &Arc<SshSession>,
    params: &ConnParams,
    host_tag: &str,
    handle: russh::client::Handle<ClientHandler>,
    agent_forward: bool,
) -> Result<StreamEnd, ConnectError> {
    // ---- detect the server OS on a quiet exec channel ----
    if let Some(host_id) = session.host_id {
        let detected = detect_os(&handle).await.unwrap_or_else(|| "unknown".into());
        let _ = state.app.emit(
            "host:os",
            HostOsEvent {
                host_id,
                os: detected.clone(),
            },
        );
        if let Ok(conn) = Connection::open(&state.db_path) {
            let _ = db::set_host_os(&conn, host_id, &detected);
        }
    }

    // ---- open PTY + shell ----
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| ConnectError::transient(format!("Could not open channel: {e}")))?;

    let mut channel = channel;
    if agent_forward {
        // Before the shell starts, so the server sets SSH_AUTH_SOCK in it.
        // No reply is awaited: a server that refuses just leaves it unset.
        let _ = channel.agent_forward(false).await;
    }
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .map_err(|e| ConnectError::transient(format!("PTY request failed: {e}")))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| ConnectError::transient(format!("Shell request failed: {e}")))?;

    *session.handle.write().await = Some(handle);
    let writer = channel.make_writer();
    *session.writer.lock().await = Some(Box::new(writer));

    // The host's startup command (cd somewhere, tmux attach), typed as if by
    // hand. The PTY holds it until the shell reads input, so sending it
    // before the prompt shows is fine. Read on every login: a reconnect runs
    // it again (tmux attach keeps the work), and picks up an edit.
    let startup = session.host_id.and_then(|id| {
        Connection::open(&state.db_path)
            .ok()
            .and_then(|conn| db::host_startup_command(&conn, id).ok().flatten())
    });
    if let Some(cmd) = startup {
        use tokio::io::AsyncWriteExt;
        // Each line is one Enter, the way a terminal sends it
        let typed: String = cmd.lines().map(|l| format!("{l}\r")).collect();
        if let Some(w) = session.writer.lock().await.as_mut() {
            let _ = w.write_all(typed.as_bytes()).await;
            let _ = w.flush().await;
        }
    }

    // Keep the password in memory for sudo auto-fill (password auth only)
    if let AuthSpec::Password(pw) = &params.auth {
        *session.sudo_password.lock().unwrap() = Some(pw.clone());
    }
    let auto_sudo = Connection::open(&state.db_path)
        .ok()
        .and_then(|conn| db::get_settings(&conn).ok())
        .map(|s| s.auto_sudo_password)
        .unwrap_or(true);

    let (resize_tx, mut resize_rx) = mpsc::channel::<(u32, u32, u32, u32)>(8);
    *session.resize_tx.lock().await = Some(resize_tx);

    // Closed while the dial was in flight (tab closed, or "Stop" during a
    // reconnect): never announce a session nobody is waiting for
    if session.closed.is_cancelled() {
        return Ok(StreamEnd::UserClosed);
    }
    emit_status(&state.app, session.id, "connected", None);

    // Session history (SSH only)
    let log_id = Connection::open(&state.db_path).ok().and_then(|conn| {
        db::session_log_start(&conn, session.id, session.host_id, &session.label, &params.username, host_tag).ok()
    });
    *session.log_id.lock().unwrap() = log_id;

    // Remote forwards still running from before a reconnect lived on the old
    // transport; ask this one to listen again
    renew_remote_forwards(state, session);

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

    // ---- pump output; service resizes; end on close or transport loss ----
    let mut output_tail = String::new();
    let mut sudo_fills: u32 = 0;
    let mut last_sudo_fill = instant_ago(Duration::from_secs(10));
    // Held across packets so a TUI frame arrives as one IPC message.
    let mut pending_out: Vec<u8> = Vec::new();
    let mut flush_at: Option<tokio::time::Instant> = None;

    // russh 0.45 never forwards CHANNEL_CLOSE to the channel — a close just
    // ends wait() with None, same as a dead transport. An exit status or
    // signal seen first means the shell ended on purpose.
    let mut shell_exited = false;

    let end = loop {
        let wake = flush_at;
        tokio::select! {
            _ = session.closed.cancelled() => break StreamEnd::UserClosed,
            _ = async {
                if let Some(at) = wake {
                    tokio::time::sleep_until(at).await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                emit_pending_stdout(&state.app, session.id, &mut pending_out);
                flush_at = None;
            }
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { data }) => {
                        crate::trace::record(session.id, "OUT", &data);
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
                        queue_stdout(&state.app, session.id, &mut pending_out, &mut flush_at, &data);
                    }
                    Some(russh::ChannelMsg::ExtendedData { data, .. }) => {
                        crate::trace::record(session.id, "ERR", &data);
                        queue_stdout(&state.app, session.id, &mut pending_out, &mut flush_at, &data);
                    }
                    Some(russh::ChannelMsg::ExitStatus { .. } | russh::ChannelMsg::ExitSignal { .. }) => {
                        shell_exited = true;
                    }
                    // The server ended the channel on purpose: the shell exited
                    Some(russh::ChannelMsg::Eof | russh::ChannelMsg::Close) => break StreamEnd::RemoteExit,
                    // The channel vanished: after an exit status that's the
                    // normal close; without one the transport died (network
                    // drop, sleep, keepalive timeout)
                    None if shell_exited => break StreamEnd::RemoteExit,
                    None => break StreamEnd::Lost,
                    _ => {}
                }
            }
            Some((cols, rows, pix_w, pix_h)) = resize_rx.recv() => {
                crate::trace::note(session.id, "RESIZE", || format!("{cols}x{rows} {pix_w}x{pix_h}px"));
                let _ = channel.window_change(cols, rows, pix_w, pix_h).await;
            }
        }
    };
    emit_pending_stdout(&state.app, session.id, &mut pending_out);
    crate::trace::note(session.id, "END", || format!("{end:?}"));

    // Wipe the in-memory password when the session ends
    *session.sudo_password.lock().unwrap() = None;
    if let Some(log_id) = *session.log_id.lock().unwrap() {
        if let Ok(conn) = Connection::open(&state.db_path) {
            let _ = db::session_log_end(&conn, log_id);
        }
    }
    // run_session reports the outcome — it may reconnect instead of closing
    Ok(end)
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
        let mut guard = session.handle.write().await;
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
    let handle_guard = session.handle.read().await;
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

pub async fn sftp_home(session: &SshSession) -> Result<String, String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    sftp.canonicalize(".")
        .await
        .map_err(|e| format!("Could not resolve home folder: {e}"))
}

/// The last `max` bytes of a file (relative paths start at the login
/// folder), and whether the start was cut off. None when it can't be read.
pub async fn sftp_read_tail(session: &SshSession, path: &str, max: u64) -> Option<(Vec<u8>, bool)> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref()?;
    let mut file = sftp.open(path).await.ok()?;
    let len = file.metadata().await.ok()?.len();
    let cut = len > max;
    if cut {
        file.seek(std::io::SeekFrom::Start(len - max)).await.ok()?;
    }
    let mut buf = Vec::with_capacity(len.min(max) as usize);
    file.take(max).read_to_end(&mut buf).await.ok()?;
    Some((buf, cut))
}

/// Create a folder only the login user can enter (0700) unless it exists.
/// For ~/.ssh when installing a key.
pub async fn sftp_ensure_private_dir(session: &SshSession, path: &str) -> Result<(), String> {
    use russh_sftp::protocol::FileAttributes;

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    if sftp.try_exists(path).await.unwrap_or(false) {
        return Ok(());
    }
    sftp.create_dir(path).await.map_err(|e| format!("Could not create ~/{path}: {e}"))?;
    // sshd's StrictModes refuses keys in a folder others can write to
    let attrs = FileAttributes { permissions: Some(0o700), ..FileAttributes::empty() };
    let _ = sftp.set_metadata(path, attrs).await;
    Ok(())
}

/// Append to a file, creating it readable by the login user only (0600)
pub async fn sftp_append_private(session: &SshSession, path: &str, bytes: &[u8]) -> Result<(), String> {
    use russh_sftp::protocol::{FileAttributes, OpenFlags};
    use tokio::io::AsyncWriteExt;

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() };
    let mut file = sftp
        .open_with_flags_and_attributes(path, OpenFlags::CREATE | OpenFlags::APPEND | OpenFlags::WRITE, attrs)
        .await
        .map_err(|e| format!("Could not open ~/{path}: {e}"))?;
    file.write_all(bytes).await.map_err(|e| format!("Write failed: {e}"))?;
    file.shutdown().await.map_err(|e| format!("Write failed: {e}"))
}

/// Write `bytes` to a new file only the login user can read (0600). Refuses
/// to replace an existing file. For screenshots pasted into the terminal.
pub async fn sftp_put_private(session: &SshSession, path: &str, bytes: &[u8]) -> Result<(), String> {
    use russh_sftp::protocol::{FileAttributes, OpenFlags};
    use tokio::io::AsyncWriteExt;

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let attrs = FileAttributes { permissions: Some(0o600), ..FileAttributes::empty() };
    let mut file = sftp
        .open_with_flags_and_attributes(path, OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE, attrs)
        .await
        .map_err(|e| format!("Could not create {path}: {e}"))?;
    file.write_all(bytes).await.map_err(|e| format!("Write failed: {e}"))?;
    // Queued write errors (disk full, quota) only surface here
    file.shutdown().await.map_err(|e| format!("Write failed: {e}"))
}

/// Delete a file, or a folder with everything inside it. SFTP's rmdir only
/// removes empty folders, so walk the tree: delete files (and symlinks —
/// the link itself, never its target) on the way down, then the emptied
/// folders deepest-first.
pub async fn sftp_remove(session: &SshSession, path: &str, is_dir: bool) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    if !is_dir {
        return sftp
            .remove_file(path)
            .await
            .map_err(|e| format!("Delete failed: {e}"));
    }

    let mut pending = vec![path.to_string()];
    let mut dirs = Vec::new();
    while let Some(dir) = pending.pop() {
        let entries = sftp
            .read_dir(&dir)
            .await
            .map_err(|e| format!("Delete failed: could not list {dir}: {e}"))?;
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let child = if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") };
            match entry.file_type() {
                russh_sftp::protocol::FileType::Dir => pending.push(child),
                _ => sftp
                    .remove_file(&child)
                    .await
                    .map_err(|e| format!("Delete failed on {child}: {e}"))?,
            }
        }
        dirs.push(dir);
    }
    for dir in dirs.iter().rev() {
        sftp.remove_dir(dir)
            .await
            .map_err(|e| format!("Delete failed on {dir}: {e}"))?;
    }
    Ok(())
}

pub async fn sftp_rename(session: &SshSession, from: &str, to: &str) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    sftp.rename(from, to)
        .await
        .map_err(|e| format!("Rename failed: {e}"))
}

// ---------- transfers: files and whole folders ----------

/// One step of a transfer. `rel` is relative to the item the user picked;
/// an empty one is that item itself (a single file, or the top folder).
#[derive(Debug, Clone, PartialEq)]
enum Step {
    Dir(Vec<String>),
    File(Vec<String>, u64),
}

/// Everything one transfer will do, worked out before the first byte moves
/// so progress can be told against the full size
#[derive(Debug, Default)]
struct Plan {
    steps: Vec<Step>,
    /// The picked item is a folder
    folder: bool,
    /// Links to folders (they could loop), sockets, devices, unreadable names
    skipped: u32,
}

impl Plan {
    fn bytes(&self) -> u64 {
        self.steps
            .iter()
            .map(|s| if let Step::File(_, n) = s { *n } else { 0 })
            .sum()
    }

    fn files(&self) -> u32 {
        self.steps.iter().filter(|s| matches!(s, Step::File(..))).count() as u32
    }
}

/// Folders nested deeper than this are almost certainly a loop
const MAX_FOLDER_DEPTH: usize = 64;

fn too_deep(rel: &[String]) -> String {
    format!("{}: folders are nested more than {MAX_FOLDER_DEPTH} deep", rel.join("/"))
}

/// What uploading `root` from this computer involves. A link at the top is
/// followed (the user picked it); links inside are followed to files only.
fn plan_local(root: &std::path::Path) -> Result<Plan, String> {
    let meta = std::fs::metadata(root).map_err(|e| format!("Local file error: {e}"))?;
    if !meta.is_dir() {
        return Ok(Plan { steps: vec![Step::File(Vec::new(), meta.len())], folder: false, skipped: 0 });
    }
    let mut plan = Plan { steps: vec![Step::Dir(Vec::new())], folder: true, skipped: 0 };
    let mut stack = vec![(root.to_path_buf(), Vec::<String>::new())];
    while let Some((dir, rel)) = stack.pop() {
        let listed = std::fs::read_dir(&dir).and_then(|it| it.collect::<Result<Vec<_>, _>>());
        let mut entries = match listed {
            Ok(entries) => entries,
            // A folder inside that can't be read is skipped, not the whole copy
            Err(_) if !rel.is_empty() => {
                plan.skipped += 1;
                continue;
            }
            Err(e) => return Err(format!("Could not list {}: {e}", dir.display())),
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(name) = entry.file_name().into_string() else {
                plan.skipped += 1; // not valid Unicode: no name to give it on the server
                continue;
            };
            let mut child = rel.clone();
            child.push(name);
            let Ok(ft) = entry.file_type() else {
                plan.skipped += 1;
                continue;
            };
            if ft.is_dir() {
                if child.len() > MAX_FOLDER_DEPTH {
                    return Err(too_deep(&child));
                }
                plan.steps.push(Step::Dir(child.clone()));
                stack.push((entry.path(), child));
            } else if ft.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                plan.steps.push(Step::File(child, size));
            } else if ft.is_symlink() {
                match std::fs::metadata(entry.path()) {
                    Ok(m) if m.is_file() => plan.steps.push(Step::File(child, m.len())),
                    _ => plan.skipped += 1,
                }
            } else {
                plan.skipped += 1;
            }
        }
    }
    Ok(plan)
}

/// What copying `root` off a server involves; the same rules as [`plan_local`].
/// The SFTP lock is taken per request, so folder listings and other transfers
/// on the session keep going during a long walk, and Cancel stops it.
async fn plan_remote(session: &SshSession, root: &str, cancel: &CancellationToken) -> Result<Plan, String> {
    use russh_sftp::protocol::FileType;

    let meta = {
        let guard = session.sftp.lock().await;
        let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
        sftp.metadata(root).await
    }
    .map_err(|e| format!("Remote stat failed: {e}"))?;
    if !meta.file_type().is_dir() {
        return Ok(Plan { steps: vec![Step::File(Vec::new(), meta.len())], folder: false, skipped: 0 });
    }
    let mut plan = Plan { steps: vec![Step::Dir(Vec::new())], folder: true, skipped: 0 };
    let mut stack = vec![Vec::<String>::new()];
    while let Some(rel) = stack.pop() {
        if cancel.is_cancelled() {
            return Err("Cancelled".into());
        }
        let dir = remote_join(root, &rel);
        let listed = {
            let guard = session.sftp.lock().await;
            let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
            sftp.read_dir(&dir).await
        };
        let mut entries: Vec<_> = match listed {
            Ok(it) => it.collect(),
            // A folder inside that can't be read is skipped, not the whole copy
            Err(_) if !rel.is_empty() => {
                plan.skipped += 1;
                continue;
            }
            Err(e) => return Err(format!("Could not list {dir}: {e}")),
        };
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            if name == "." || name == ".." || name.contains('/') || name.is_empty() {
                continue;
            }
            let mut child = rel.clone();
            child.push(name);
            match entry.file_type() {
                FileType::Dir => {
                    if child.len() > MAX_FOLDER_DEPTH {
                        return Err(too_deep(&child));
                    }
                    plan.steps.push(Step::Dir(child.clone()));
                    stack.push(child);
                }
                FileType::File => {
                    let size = entry.metadata().len();
                    plan.steps.push(Step::File(child, size));
                }
                FileType::Symlink => {
                    let target = {
                        let guard = session.sftp.lock().await;
                        let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
                        sftp.metadata(remote_join(root, &child)).await
                    };
                    match target {
                        Ok(m) if m.file_type().is_file() => plan.steps.push(Step::File(child, m.len())),
                        _ => plan.skipped += 1,
                    }
                }
                FileType::Other => plan.skipped += 1,
            }
        }
    }
    Ok(plan)
}

/// `base` + `rel` on a server
fn remote_join(base: &str, rel: &[String]) -> String {
    if rel.is_empty() {
        return base.to_string();
    }
    let base = if base == "/" { "" } else { base.trim_end_matches('/') };
    format!("{base}/{}", rel.join("/"))
}

/// `base` + `rel` on this computer. Names that come from a server are made
/// safe here first: Windows refuses `:` or `?`, and none may climb out with `..`.
fn local_join(base: &str, rel: &[String], from_server: bool) -> PathBuf {
    let mut p = PathBuf::from(base);
    for name in rel {
        if from_server {
            p.push(safe_local_name(name, cfg!(windows)));
        } else {
            p.push(name);
        }
    }
    p
}

/// A server's file name as one this computer can save. The same rules as
/// safeLocalName in the frontend (src/components/sftp/fsUtils.ts).
fn safe_local_name(name: &str, windows: bool) -> String {
    if !windows {
        let safe: String = name.chars().map(|c| if c == '/' || c == '\0' { '_' } else { c }).collect();
        return if safe.is_empty() || safe == "." || safe == ".." { "_".into() } else { safe };
    }
    let mapped: String = name
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    // Windows drops trailing dots and spaces, which would change the name
    let mut out = mapped.trim_end_matches(['.', ' ']).to_string();
    if out.is_empty() {
        out = "_".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if reserved {
        out = format!("_{out}");
    }
    out
}

/// Prefix an error with the file it happened on, inside a folder
fn at(rel: &[String], e: String) -> String {
    if rel.is_empty() { e } else { format!("{}: {e}", rel.join("/")) }
}

/// Where each step of a download lands on this computer. Server names are
/// made safe first ([`safe_local_name`]), and that can turn two names into
/// one: `a:b` and `a?b` on Windows, or `README` and `readme` where the disk
/// ignores case (`fold_case`). A later duplicate gets " (2)", " (3)" and so
/// on, so no file from the copy overwrites another from the same copy.
fn download_targets(base: &str, steps: &[Step], windows: bool, fold_case: bool) -> Vec<PathBuf> {
    let key = |n: &str| if fold_case { n.to_lowercase() } else { n.to_string() };
    let mut dirs: HashMap<Vec<String>, PathBuf> = HashMap::new();
    let mut taken: HashMap<PathBuf, std::collections::HashSet<String>> = HashMap::new();
    let mut out = Vec::with_capacity(steps.len());
    for step in steps {
        let (rel, is_file) = match step {
            Step::Dir(rel) => (rel, false),
            Step::File(rel, _) => (rel, true),
        };
        let Some((last, parent_rel)) = rel.split_last() else {
            dirs.insert(Vec::new(), PathBuf::from(base));
            out.push(PathBuf::from(base));
            continue;
        };
        let parent = dirs.get(parent_rel).cloned().unwrap_or_else(|| local_join(base, parent_rel, true));
        let name = safe_local_name(last, windows);
        let used = taken.entry(parent.clone()).or_default();
        let mut chosen = name.clone();
        let mut n = 2;
        while !used.insert(key(&chosen)) {
            chosen = numbered_name(&name, n, is_file);
            n += 1;
        }
        let path = parent.join(&chosen);
        if !is_file {
            dirs.insert(rel.clone(), path.clone());
        }
        out.push(path);
    }
    out
}

/// "report.txt", 2 -> "report (2).txt"; a folder, or a file whose only dot
/// leads its name (".env"), gets the number on the end
fn numbered_name(name: &str, n: u32, is_file: bool) -> String {
    match name.rfind('.').filter(|&i| is_file && i > 0) {
        Some(i) => format!("{} ({n}){}", &name[..i], &name[i..]),
        None => format!("{name} ({n})"),
    }
}

/// An error that ends the whole transfer: Cancel, or the SFTP channel gone.
/// Anything else fails one file, and a folder copy carries on past it.
fn ends_transfer(e: &str, cancel: &CancellationToken) -> bool {
    cancel.is_cancelled() || e.starts_with("SFTP not open")
}

/// What went wrong inside one transfer that kept going
#[derive(Default)]
struct Failures {
    files: u32,
    first: Option<String>,
    /// Folders that could not be made; nothing inside them is tried
    dirs: Vec<Vec<String>>,
}

impl Failures {
    fn inside_failed_dir(&self, rel: &[String]) -> bool {
        self.dirs.iter().any(|d| rel.starts_with(d))
    }

    fn dir(&mut self, rel: &[String], e: String) {
        self.dirs.push(rel.to_vec());
        self.first.get_or_insert_with(|| at(rel, e));
    }

    fn file(&mut self, rel: &[String], e: String) {
        self.files += 1;
        self.first.get_or_insert_with(|| at(rel, e));
    }

    fn into_result(self, folder: bool, files_total: u32) -> Result<(), String> {
        let Some(first) = self.first else { return Ok(()) };
        if !folder || self.files == 0 {
            return Err(first);
        }
        let what = if files_total == 1 { "1 file failed".to_string() } else { format!("{} of {files_total} files failed", self.files) };
        Err(format!("{what}. The first: {first}"))
    }
}

/// Running totals for one transfer, sent to the UI as `transfer:progress`
struct Progress<'a> {
    app: &'a AppHandle,
    id: u32,
    /// Bytes moved; a copy between two servers counts both of its halves
    units: u64,
    halves: bool,
    total: u64,
    files_done: u32,
    files_total: u32,
    skipped: u32,
    folder: bool,
    last_emit: Instant,
}

impl<'a> Progress<'a> {
    fn new(app: &'a AppHandle, id: u32, halves: bool) -> Self {
        Progress {
            app,
            id,
            units: 0,
            halves,
            total: 0,
            files_done: 0,
            files_total: 0,
            skipped: 0,
            folder: false,
            last_emit: instant_ago(Duration::from_secs(1)),
        }
    }

    fn plan(&mut self, plan: &Plan) {
        self.total = plan.bytes();
        self.files_total = plan.files();
        self.skipped = plan.skipped;
        self.folder = plan.folder;
        self.emit(false, None, true);
    }

    fn moved(&mut self, n: u64) {
        self.units += n;
        self.emit(false, None, false);
    }

    fn file_done(&mut self) {
        self.files_done += 1;
        // Small and empty files move no bytes worth an event of their own
        self.emit(false, None, false);
    }

    fn emit(&mut self, done: bool, error: Option<String>, force: bool) {
        if !force && self.last_emit.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_emit = Instant::now();
        let moved = if self.halves { self.units / 2 } else { self.units };
        let transferred = if done && error.is_none() { self.total } else { moved.min(self.total) };
        let (files_done, files_total, skipped) = if self.folder {
            (Some(self.files_done), Some(self.files_total), (self.skipped > 0).then_some(self.skipped))
        } else {
            (None, None, None)
        };
        let _ = self.app.emit(
            "transfer:progress",
            TransferEvent {
                transfer_id: self.id,
                transferred,
                size: self.total,
                done,
                error,
                files_done,
                files_total,
                skipped,
            },
        );
    }

    /// The last event: done, or why not
    fn finish(&mut self, result: Result<(), String>, cancel: &CancellationToken) {
        match result {
            Ok(()) => self.emit(true, None, true),
            Err(e) => {
                let msg = if cancel.is_cancelled() { "Cancelled".to_string() } else { e };
                self.emit(true, Some(msg), true);
            }
        }
    }
}

fn register_transfer(session: &SshSession, transfer_id: u32) -> CancellationToken {
    let cancel = CancellationToken::new();
    session.transfers.lock().unwrap().insert(transfer_id, cancel.clone());
    cancel
}

/// A folder on the server, made if missing. One already there is used as it
/// is: copying into it merges, and files of the same name are replaced.
async fn ensure_remote_dir(session: &SshSession, path: &str) -> Result<(), String> {
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    if let Err(e) = sftp.create_dir(path).await {
        match sftp.metadata(path).await {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => return Err(format!("{path} already exists and is not a folder")),
            Err(_) => return Err(format!("Could not create folder {path}: {e}")),
        }
    }
    Ok(())
}

async fn upload_file(
    session: &SshSession,
    local: &std::path::Path,
    remote: &str,
    cancel: &CancellationToken,
    p: &mut Progress<'_>,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;

    let mut local_file = tokio::fs::File::open(local)
        .await
        .map_err(|e| format!("Could not open local file: {e}"))?;
    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let mut remote_file = sftp
        .create(remote)
        .await
        .map_err(|e| format!("Could not create remote file: {e}"))?;

    let mut buf = vec![0u8; TRANSFER_CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err("Cancelled".into());
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
        p.moved(n as u64);
    }
    // Pending write errors (disk full, quota) only surface here:
    // russh-sftp queues writes without waiting for their acks, so
    // discarding this result reported "Done" for a failed upload
    remote_file
        .shutdown()
        .await
        .map_err(|e| format!("Remote write failed: {e}"))
}

async fn download_file(
    session: &SshSession,
    remote: &str,
    local: &std::path::Path,
    cancel: &CancellationToken,
    p: &mut Progress<'_>,
) -> Result<(), String> {
    use tokio::io::AsyncReadExt;

    let guard = session.sftp.lock().await;
    let sftp = guard.as_ref().ok_or_else(|| "SFTP not open".to_string())?;
    let mut remote_file = sftp
        .open(remote)
        .await
        .map_err(|e| format!("Could not open remote file: {e}"))?;
    let mut local_file = tokio::fs::File::create(local)
        .await
        .map_err(|e| format!("Could not create local file: {e}"))?;

    let mut buf = vec![0u8; TRANSFER_CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err("Cancelled".into());
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
        p.moved(n as u64);
    }
    local_file
        .flush()
        .await
        .map_err(|e| format!("Local write failed: {e}"))
}

/// Upload a file or a whole folder from this computer to `remote`
pub async fn run_upload(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    transfer_id: u32,
    local: String,
    remote: String,
) {
    let cancel = register_transfer(&session, transfer_id);
    let mut p = Progress::new(&state.app, transfer_id, false);
    let result = async {
        let root = PathBuf::from(&local);
        let plan = tokio::task::spawn_blocking(move || plan_local(&root))
            .await
            .map_err(|e| e.to_string())??;
        p.plan(&plan);
        let mut failed = Failures::default();
        for step in &plan.steps {
            if cancel.is_cancelled() {
                return Err("Cancelled".to_string());
            }
            match step {
                Step::Dir(rel) => {
                    if failed.inside_failed_dir(rel) {
                        continue;
                    }
                    if let Err(e) = ensure_remote_dir(&session, &remote_join(&remote, rel)).await {
                        if rel.is_empty() || ends_transfer(&e, &cancel) {
                            return Err(at(rel, e));
                        }
                        failed.dir(rel, e);
                    }
                }
                Step::File(rel, _) => {
                    if failed.inside_failed_dir(rel) {
                        failed.files += 1;
                        continue;
                    }
                    match upload_file(&session, &local_join(&local, rel, false), &remote_join(&remote, rel), &cancel, &mut p).await {
                        Ok(()) => p.file_done(),
                        Err(e) if ends_transfer(&e, &cancel) => return Err(at(rel, e)),
                        Err(e) => failed.file(rel, e),
                    }
                }
            }
        }
        failed.into_result(plan.folder, plan.files())
    }
    .await;
    p.finish(result, &cancel);
    session.transfers.lock().unwrap().remove(&transfer_id);
}

/// Download a file or a whole folder from the server to `local`
pub async fn run_download(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    transfer_id: u32,
    remote: String,
    local: String,
) {
    let cancel = register_transfer(&session, transfer_id);
    let mut p = Progress::new(&state.app, transfer_id, false);
    let result = async {
        let plan = plan_remote(&session, &remote, &cancel).await?;
        p.plan(&plan);
        let targets = download_targets(&local, &plan.steps, cfg!(windows), cfg!(any(windows, target_os = "macos")));
        let mut failed = Failures::default();
        for (step, target) in plan.steps.iter().zip(&targets) {
            if cancel.is_cancelled() {
                return Err("Cancelled".to_string());
            }
            match step {
                Step::Dir(rel) => {
                    if failed.inside_failed_dir(rel) {
                        continue;
                    }
                    if let Err(e) = tokio::fs::create_dir_all(target).await {
                        let e = format!("Could not create local folder: {e}");
                        if rel.is_empty() {
                            return Err(e);
                        }
                        failed.dir(rel, e);
                    }
                }
                Step::File(rel, _) => {
                    if failed.inside_failed_dir(rel) {
                        failed.files += 1;
                        continue;
                    }
                    match download_file(&session, &remote_join(&remote, rel), target, &cancel, &mut p).await {
                        Ok(()) => p.file_done(),
                        Err(e) if ends_transfer(&e, &cancel) => return Err(at(rel, e)),
                        Err(e) => failed.file(rel, e),
                    }
                }
            }
        }
        failed.into_result(plan.folder, plan.files())
    }
    .await;
    p.finish(result, &cancel);
    session.transfers.lock().unwrap().remove(&transfer_id);
}

/// Server-to-server copy between two different SSH sessions, of a file or a
/// whole folder: each file goes through a temp file here. Reported as one
/// transfer whose progress covers both halves.
pub async fn run_cross_transfer(
    state: Arc<AppState>,
    src: Arc<SshSession>,
    dst: Arc<SshSession>,
    transfer_id: u32,
    src_path: String,
    dst_dir: String,
) {
    let cancel = register_transfer(&src, transfer_id);
    let mut p = Progress::new(&state.app, transfer_id, true);
    let name = src_path.trim_end_matches('/').rsplit('/').next().unwrap_or("file").to_string();
    let dst_root = remote_join(&dst_dir, &[name]);

    let result = async {
        let plan = plan_remote(&src, &src_path, &cancel)
            .await
            .map_err(|e| if e.starts_with("SFTP not open") { "SFTP not open (source)".to_string() } else { e })?;
        p.plan(&plan);
        let mut failed = Failures::default();
        for (i, step) in plan.steps.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err("Cancelled".to_string());
            }
            match step {
                Step::Dir(rel) => {
                    if failed.inside_failed_dir(rel) {
                        continue;
                    }
                    if let Err(e) = ensure_remote_dir(&dst, &remote_join(&dst_root, rel)).await {
                        if rel.is_empty() || ends_transfer(&e, &cancel) {
                            return Err(at(rel, e));
                        }
                        failed.dir(rel, e);
                    }
                }
                Step::File(rel, _) => {
                    if failed.inside_failed_dir(rel) {
                        failed.files += 1;
                        continue;
                    }
                    let temp = std::env::temp_dir().join(format!("liteterm_xfer_{transfer_id}_{i}"));
                    let copied = async {
                        download_file(&src, &remote_join(&src_path, rel), &temp, &cancel, &mut p).await?;
                        upload_file(&dst, &temp, &remote_join(&dst_root, rel), &cancel, &mut p).await
                    }
                    .await;
                    let _ = tokio::fs::remove_file(&temp).await;
                    match copied {
                        Ok(()) => p.file_done(),
                        Err(e) if ends_transfer(&e, &cancel) => return Err(at(rel, e)),
                        Err(e) => failed.file(rel, e),
                    }
                }
            }
        }
        failed.into_result(plan.folder, plan.files())
    }
    .await;
    p.finish(result, &cancel);
    src.transfers.lock().unwrap().remove(&transfer_id);
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

/// Start a forward on the open session for its host. Local (-L) and dynamic
/// (-D) forwards listen on 127.0.0.1 here; a remote one (-R) asks the server
/// to listen and connects what it hands back (ClientHandler).
pub async fn forward_start(
    state: Arc<AppState>,
    forward: models::PortForward,
) -> Result<(), String> {
    // One start per forward at a time. Clicking Start while auto-start is
    // still bringing the same forward up waits for it, and then finds it
    // running, instead of racing it for the port.
    let gate = state.forward_starts.lock().unwrap().entry(forward.id).or_default().clone();
    let _starting = gate.lock().await;
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

    match forward.kind.as_str() {
        "remote" => start_remote(state, session, forward).await,
        "dynamic" => start_listener(state, session, forward, Serve::Socks).await,
        _ => start_listener(state, session, forward, Serve::Direct).await,
    }
}

/// What a local listener does with each connection
#[derive(Clone)]
enum Serve {
    /// Local forward: always to the same host and port, seen from the server
    Direct,
    /// Dynamic forward: wherever the SOCKS client asks
    Socks,
}

async fn start_listener(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    forward: models::PortForward,
    serve: Serve,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", forward.local_port as u16))
        .await
        .map_err(|e| format!("Could not bind local port {}: {e}", forward.local_port))?;

    let cancel = CancellationToken::new();
    let state2 = state.clone();
    let fw_id = forward.id;
    let remote_host = forward.remote_host.clone();
    let remote_port = forward.remote_port as u32;
    let task_cancel = cancel.clone();
    let session_id = session.id;

    let listener_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = task_cancel.cancelled() => break,
                accepted = listener.accept() => {
                    let (tcp, peer) = match accepted {
                        Ok(pair) => pair,
                        // A client that resets before accept() returns (browsers do)
                        // is no reason to end the tunnel for everyone else
                        Err(e) if matches!(
                            e.kind(),
                            std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::TimedOut
                        ) => {
                            crate::applog::write_throttled(
                                "forward accept",
                                &format!("tunnel {fw_id}: accept failed, continuing: {e}"),
                            );
                            continue;
                        }
                        Err(_) => break,
                    };
                    // One task per connection: a browser or a database client
                    // opens several at once and they must not queue behind
                    // each other. A connection that cannot be served (the far
                    // side refused it, or the session is reconnecting) is
                    // dropped on its own — the tunnel stays up for the next
                    // one. It ends when the session does
                    // (stop_forwards_for_session) or the user stops it.
                    let session = session.clone();
                    let remote_host = remote_host.clone();
                    let conn_cancel = task_cancel.clone();
                    let serve = serve.clone();
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = conn_cancel.cancelled() => {}
                            _ = async {
                                match serve {
                                    Serve::Direct => {
                                        let _ = pump_connection(session, tcp, peer, remote_host, remote_port).await;
                                    }
                                    Serve::Socks => serve_socks(session, tcp, peer).await,
                                }
                            } => {}
                        }
                    });
                }
            }
        }
        state2.forwards.lock().unwrap().remove(&fw_id);
        let _ = state2
            .app
            .emit("forward:stopped", ForwardEvent { id: fw_id, active: false, error: None });
    });

    state.forwards.lock().unwrap().insert(
        forward.id,
        ForwardHandle {
            cancel,
            listener: listener_task,
            session_id,
            remote_port: None,
        },
    );
    let _ = state
        .app
        .emit("forward:started", ForwardEvent { id: forward.id, active: true, error: None });
    Ok(())
}

/// Ask the server to listen on `port` for this session's current transport
async fn request_remote_listen(session: &SshSession, port: u32) -> Result<(), String> {
    let mut guard = session.handle.write().await;
    let handle = guard.as_mut().ok_or_else(|| "Not connected".to_string())?;
    match tokio::time::timeout(Duration::from_secs(15), handle.tcpip_forward(REMOTE_BIND, port)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(russh::Error::RequestDenied)) => Err(format!(
            "The server refused to listen on port {port}: the port is in use there, \
             or its sshd does not allow forwarding (AllowTcpForwarding)"
        )),
        Ok(Err(e)) => Err(format!("Could not ask the server to listen on port {port}: {e}")),
        Err(_) => {
            // The request already went out. Should the server still say yes,
            // it would listen with nothing behind it and keep the port; the
            // cancel queues behind the request, so it undoes that.
            let _ = tokio::time::timeout(Duration::from_secs(5), handle.cancel_tcpip_forward(REMOTE_BIND, port)).await;
            Err(format!("The server did not answer the request to listen on port {port}"))
        }
    }
}

/// Tells one start's remote target from another's
static NEXT_REMOTE_START: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

async fn start_remote(
    state: Arc<AppState>,
    session: Arc<SshSession>,
    forward: models::PortForward,
) -> Result<(), String> {
    let server_port = forward.remote_port as u32;
    let key = (session.id, server_port);
    let cancel = CancellationToken::new();
    let start = NEXT_REMOTE_START.fetch_add(1, Ordering::Relaxed);
    {
        let mut targets = state.remote_targets.lock().unwrap();
        if let Some(other) = targets.get(&key) {
            if other.forward_id != forward.id {
                return Err(format!("Another forward already listens on port {server_port} of this server"));
            }
        }
        // In place before the request: a connection may arrive right after the reply
        targets.insert(
            key,
            RemoteTarget {
                forward_id: forward.id,
                host: forward.remote_host.clone(),
                port: forward.local_port as u16,
                cancel: cancel.clone(),
                start,
            },
        );
    }
    if let Err(e) = request_remote_listen(&session, server_port).await {
        let mut targets = state.remote_targets.lock().unwrap();
        // Only the target this call put there
        if targets.get(&key).is_some_and(|t| t.start == start) {
            targets.remove(&key);
        }
        return Err(e);
    }

    // Nothing to accept here; the task only marks the forward as running
    // until it is stopped
    let task_cancel = cancel.clone();
    let listener_task = tokio::spawn(async move { task_cancel.cancelled().await });
    state.forwards.lock().unwrap().insert(
        forward.id,
        ForwardHandle {
            cancel,
            listener: listener_task,
            session_id: session.id,
            remote_port: Some(server_port),
        },
    );
    let _ = state
        .app
        .emit("forward:started", ForwardEvent { id: forward.id, active: true, error: None });
    Ok(())
}

/// After a reconnect: the remote forwards riding on this session were
/// registered with the transport that died, so ask the new one again. Right
/// after a network drop the old sshd usually still holds the port, so a
/// refusal is retried for about a minute before the forward is stopped, and
/// the UI is told why. A later reconnect replaces these retries with its own.
fn renew_remote_forwards(state: &Arc<AppState>, session: &Arc<SshSession>) {
    let renewal = {
        let mut current = session.renewals.lock().unwrap();
        current.cancel();
        *current = CancellationToken::new();
        current.clone()
    };
    let renew: Vec<(i64, u32, Option<u64>)> = state
        .forwards
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, h)| h.session_id == session.id)
        .filter_map(|(id, h)| h.remote_port.map(|p| (*id, p)))
        .map(|(id, port)| (id, port, remote_start_of(state, session.id, port)))
        .collect();
    for (id, port, start) in renew {
        let state = state.clone();
        let session = session.clone();
        let renewal = renewal.clone();
        tokio::spawn(async move {
            const RETRY_SECS: [u64; 6] = [0, 2, 4, 8, 16, 30];
            let mut last_error = String::new();
            for wait in RETRY_SECS {
                tokio::select! {
                    _ = renewal.cancelled() => return,
                    _ = session.closed.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(wait)) => {}
                }
                // Stopped (or stopped and started again) meanwhile, or the
                // transport dropped again: the next reconnect renews it, or
                // the session's end stops it
                if remote_start_of(&state, session.id, port) != start || session.handle.read().await.is_none() {
                    return;
                }
                match request_remote_listen(&session, port).await {
                    Ok(()) => return,
                    Err(e) => last_error = e,
                }
            }
            if renewal.is_cancelled() || remote_start_of(&state, session.id, port) != start {
                return;
            }
            crate::applog::write(&format!("remote forward {id} after reconnect: {last_error}"));
            forward_stop_because(state, id, Some(format!("Stopped after the connection came back: {last_error}"))).await;
        });
    }
}

/// Which start of a remote forward owns `port` on a session's server now
fn remote_start_of(state: &AppState, session_id: u32, port: u32) -> Option<u64> {
    state.remote_targets.lock().unwrap().get(&(session_id, port)).map(|t| t.start)
}

/// How long a channel open may wait on the server. For a direct-tcpip channel
/// the server answers only once its own connect to the destination is done,
/// which for an unreachable address can take minutes.
const DIRECT_OPEN_TIMEOUT: Duration = Duration::from_secs(20);

/// Open a direct-tcpip channel from the server to `host:port`
async fn open_direct(
    session: &SshSession,
    host: String,
    port: u32,
    peer: std::net::SocketAddr,
) -> Result<russh::Channel<russh::client::Msg>, russh::Error> {
    let handle_guard = session.handle.read().await;
    let handle = handle_guard.as_ref().ok_or(russh::Error::Disconnect)?;
    let open = handle.channel_open_direct_tcpip(host, port, peer.ip().to_string(), peer.port() as u32);
    tokio::select! {
        // Closing the tab must not wait for a slow open to give up
        _ = session.closed.cancelled() => Err(russh::Error::Disconnect),
        opened = tokio::time::timeout(DIRECT_OPEN_TIMEOUT, open) => opened.unwrap_or(Err(russh::Error::ConnectionTimeout)),
    }
}

async fn pump_connection(
    session: Arc<SshSession>,
    mut tcp: tokio::net::TcpStream,
    peer: std::net::SocketAddr,
    remote_host: String,
    remote_port: u32,
) -> Result<(), String> {
    let channel = open_direct(&session, remote_host, remote_port, peer)
        .await
        .map_err(|e| format!("Forward channel failed: {e}"))?;
    let mut remote_stream = channel.into_stream();
    let _ = tokio::io::copy_bidirectional(&mut tcp, &mut remote_stream).await;
    Ok(())
}

/// One connection to a dynamic forward: the SOCKS handshake names the
/// destination, the server opens it
async fn serve_socks(session: Arc<SshSession>, mut tcp: tokio::net::TcpStream, peer: std::net::SocketAddr) {
    use super::socks;

    // A client that connects and says nothing must not hold a task forever
    let request = match tokio::time::timeout(Duration::from_secs(15), socks::handshake(&mut tcp)).await {
        Ok(Ok(request)) => request,
        _ => return,
    };
    match open_direct(&session, request.host, request.port as u32, peer).await {
        Ok(channel) => {
            if socks::reply(&mut tcp, request.version, Ok(())).await.is_err() {
                let _ = channel.close().await;
                return;
            }
            let mut remote_stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut tcp, &mut remote_stream).await;
        }
        Err(e) => {
            let failure = if matches!(e, russh::Error::ChannelOpenFailure(_)) {
                socks::Failure::Refused
            } else {
                socks::Failure::General
            };
            let _ = socks::reply(&mut tcp, request.version, Err(failure)).await;
        }
    }
}

/// Forget the remote targets a stopped forward left behind
fn remove_remote_targets(state: &AppState, forward_id: i64) -> Vec<(u32, u32)> {
    let mut targets = state.remote_targets.lock().unwrap();
    let keys: Vec<(u32, u32)> = targets
        .iter()
        .filter(|(_, t)| t.forward_id == forward_id)
        .map(|(k, _)| *k)
        .collect();
    for k in &keys {
        if let Some(t) = targets.remove(k) {
            t.cancel.cancel();
        }
    }
    keys
}

pub async fn forward_stop(state: Arc<AppState>, forward_id: i64) {
    forward_stop_because(state, forward_id, None).await
}

/// Stop a forward; `reason` says why when the app stopped it by itself
async fn forward_stop_because(state: Arc<AppState>, forward_id: i64, reason: Option<String>) {
    let handle = state.forwards.lock().unwrap().remove(&forward_id);
    if let Some(handle) = handle {
        handle.cancel.cancel();
        handle.listener.abort();
    }
    // A remote forward: ask the server to stop listening, if it is still there
    for (session_id, port) in remove_remote_targets(&state, forward_id) {
        let session = { state.sessions.lock().unwrap().get(&session_id).cloned() };
        if let Some(session) = session {
            let guard = session.handle.read().await;
            if let Some(handle) = guard.as_ref() {
                let _ = tokio::time::timeout(
                    Duration::from_secs(5),
                    handle.cancel_tcpip_forward(REMOTE_BIND, port),
                )
                .await;
            }
        }
    }
    let _ = state
        .app
        .emit("forward:stopped", ForwardEvent { id: forward_id, active: false, error: reason });
}

/// Stop every tunnel riding on `session_id`. Called when a session is gone
/// for good: its listeners would keep reporting "Listening" while every
/// connection they accept can never be served.
pub fn stop_forwards_for_session(state: &AppState, session_id: u32) {
    let stopped: Vec<(i64, ForwardHandle)> = {
        let mut forwards = state.forwards.lock().unwrap();
        let ids: Vec<i64> = forwards
            .iter()
            .filter(|(_, handle)| handle.session_id == session_id)
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter()
            .filter_map(|id| forwards.remove(&id).map(|handle| (id, handle)))
            .collect()
    };
    // The transport is gone, so the server has already dropped its listeners
    state.remote_targets.lock().unwrap().retain(|(sid, _), t| {
        let keep = *sid != session_id;
        if !keep {
            t.cancel.cancel();
        }
        keep
    });
    for (id, handle) in stopped {
        handle.cancel.cancel();
        handle.listener.abort();
        let _ = state
            .app
            .emit("forward:stopped", ForwardEvent { id, active: false, error: None });
    }
}

pub fn list_active_forwards(state: &AppState) -> Vec<i64> {
    state.forwards.lock().unwrap().keys().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant as StdInstant;

    #[tokio::test]
    async fn a_dial_that_finishes_in_time_is_returned() {
        let extend = tokio::sync::Notify::new();
        let out = with_extendable_deadline(
            async { 7 },
            Duration::from_millis(200),
            Duration::from_secs(5),
            &extend,
        )
        .await;
        assert_eq!(out, Some(7));
    }

    #[tokio::test]
    async fn a_silent_dial_times_out_at_the_base_limit() {
        let extend = tokio::sync::Notify::new();
        let started = StdInstant::now();
        let out = with_extendable_deadline(
            std::future::pending::<u8>(),
            Duration::from_millis(150),
            Duration::from_secs(5),
            &extend,
        )
        .await;
        assert_eq!(out, None);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "a port that never speaks SSH must not get the host-key prompt's allowance"
        );
    }

    #[tokio::test]
    async fn a_prompt_moves_the_deadline_out_to_a_human_pace() {
        let extend = Arc::new(tokio::sync::Notify::new());
        let prompt = extend.clone();
        // The prompt appears at 50 ms and the person answers at 400 ms: past the
        // base limit (150 ms) but inside the extended one
        let work = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            prompt.notify_one();
            tokio::time::sleep(Duration::from_millis(350)).await;
            "accepted"
        };
        let out = with_extendable_deadline(
            work,
            Duration::from_millis(150),
            Duration::from_secs(5),
            &extend,
        )
        .await;
        assert_eq!(out, Some("accepted"));
    }

    #[tokio::test]
    async fn an_unanswered_prompt_still_ends_at_the_extended_limit() {
        let extend = Arc::new(tokio::sync::Notify::new());
        let prompt = extend.clone();
        let work = async move {
            prompt.notify_one();
            std::future::pending::<u8>().await
        };
        let started = StdInstant::now();
        let out = with_extendable_deadline(
            work,
            Duration::from_millis(100),
            Duration::from_millis(300),
            &extend,
        )
        .await;
        assert_eq!(out, None);
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(250) && took < Duration::from_secs(3),
            "took {took:?}"
        );
    }

    #[test]
    fn a_young_clock_does_not_panic() {
        // `Instant::now() - age` panics when the machine booted less than `age`
        // ago; an absurd age stands in for that
        let _ = instant_ago(Duration::from_secs(u64::MAX / 4));
    }

    fn s(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn remote_paths_join_without_doubled_slashes() {
        assert_eq!(remote_join("/var/www", &[]), "/var/www");
        assert_eq!(remote_join("/var/www", &s(&["a", "b.txt"])), "/var/www/a/b.txt");
        assert_eq!(remote_join("/var/www/", &s(&["a"])), "/var/www/a");
        assert_eq!(remote_join("/", &s(&["etc"])), "/etc");
        assert_eq!(remote_join("relative", &s(&["x"])), "relative/x");
    }

    #[test]
    fn server_names_are_made_safe_for_this_disk() {
        assert_eq!(safe_local_name("app-12:30.log", true), "app-12_30.log");
        assert_eq!(safe_local_name("what?.txt", true), "what_.txt");
        assert_eq!(safe_local_name("trailing. ", true), "trailing");
        assert_eq!(safe_local_name("CON", true), "_CON");
        assert_eq!(safe_local_name("com1.txt", true), "_com1.txt");
        assert_eq!(safe_local_name("console", true), "console");
        assert_eq!(safe_local_name("..", true), "_");
        assert_eq!(safe_local_name("..", false), "_");
        assert_eq!(safe_local_name("a:b", false), "a:b");
        assert_eq!(safe_local_name("plain.txt", true), "plain.txt");
        // A name from the server can never climb out of the download folder
        let p = local_join("C:/dl/site", &s(&["..", "..", "x"]), true);
        assert!(!p.components().any(|c| matches!(c, std::path::Component::ParentDir)), "{p:?}");
    }

    #[test]
    fn a_local_folder_plans_parents_before_children() {
        let root = std::env::temp_dir().join(format!("liteterm-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("b/deep")).unwrap();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("top.txt"), b"12345").unwrap();
        std::fs::write(root.join("b/one.bin"), [0u8; 10]).unwrap();
        std::fs::write(root.join("b/deep/two.bin"), [0u8; 7]).unwrap();

        let plan = plan_local(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        assert!(plan.folder);
        assert_eq!(plan.bytes(), 22);
        assert_eq!(plan.files(), 3);
        assert_eq!(plan.steps[0], Step::Dir(vec![]));
        let pos = |want: &Step| plan.steps.iter().position(|st| st == want).unwrap();
        assert!(pos(&Step::Dir(s(&["b"]))) < pos(&Step::Dir(s(&["b", "deep"]))));
        assert!(pos(&Step::Dir(s(&["b", "deep"]))) < pos(&Step::File(s(&["b", "deep", "two.bin"]), 7)));
        assert!(plan.steps.contains(&Step::Dir(s(&["a"]))), "empty folders come along");
        assert!(plan.steps.contains(&Step::File(s(&["top.txt"]), 5)));
    }

    #[test]
    fn names_that_become_one_on_this_disk_get_numbered() {
        let steps = vec![
            Step::Dir(vec![]),
            Step::File(s(&["a:b.txt"]), 1),
            Step::File(s(&["a?b.txt"]), 1),
            Step::File(s(&["README"]), 1),
            Step::File(s(&["readme"]), 1),
            Step::Dir(s(&["x:y"])),
            Step::Dir(s(&["x?y"])),
            Step::File(s(&["x?y", "inner.txt"]), 1),
        ];
        let names: Vec<String> = download_targets("/dl/site", &steps, true, true)
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            names,
            [
                "/dl/site",
                "/dl/site/a_b.txt",
                "/dl/site/a_b (2).txt",
                "/dl/site/README",
                "/dl/site/readme (2)",
                "/dl/site/x_y",
                "/dl/site/x_y (2)",
                "/dl/site/x_y (2)/inner.txt",
            ]
        );
        // A disk that tells case apart keeps both
        let linux = download_targets("/dl", &steps[3..5], false, false);
        assert_eq!(linux[1].file_name().unwrap(), "readme");
        assert_eq!(numbered_name(".env", 2, true), ".env (2)");
        assert_eq!(numbered_name("v1.2", 3, false), "v1.2 (3)");
    }

    #[test]
    fn a_folder_copy_reports_the_files_it_could_not_move() {
        assert_eq!(Failures::default().into_result(true, 10), Ok(()));

        let mut one = Failures::default();
        one.file(&[], "Remote write failed: quota".into());
        assert_eq!(one.into_result(false, 1), Err("Remote write failed: quota".into()));

        let mut some = Failures::default();
        some.file(&s(&["a", "x.bin"]), "Permission denied".into());
        some.dir(&s(&["locked"]), "Could not create folder".into());
        assert!(some.inside_failed_dir(&s(&["locked", "deep", "f"])));
        assert!(!some.inside_failed_dir(&s(&["lockedness"])));
        some.files += 2; // two files inside the folder that failed
        assert_eq!(some.into_result(true, 9), Err("3 of 9 files failed. The first: a/x.bin: Permission denied".into()));
    }

    #[test]
    fn a_single_file_plans_as_itself() {
        let file = std::env::temp_dir().join(format!("liteterm-plan-file-{}", std::process::id()));
        std::fs::write(&file, b"abc").unwrap();
        let plan = plan_local(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        assert!(!plan.folder);
        assert_eq!(plan.steps, vec![Step::File(vec![], 3)]);
        assert!(plan_local(std::path::Path::new("Z:/surely/not/here")).is_err());
    }
}
