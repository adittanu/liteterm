//! Local terminal sessions via ConPTY (portable-pty).
//! Reuses the same event channel as SSH sessions: `ssh:stdout`,
//! `session:status` — the frontend terminal is source-agnostic.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::ssh::pool::{AppState, StdoutEvent};

const TERM_COLS: u16 = 80;
const TERM_ROWS: u16 = 24;

pub struct LocalSession {
    pub id: u32,
    pub master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    pub writer: Mutex<Option<Box<dyn Write + Send>>>,
    pub child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    pub closed: CancellationToken,
}

/// Shell candidates in preference order: pwsh 7 from its default install
/// location, pwsh from PATH (scoop/winget), then Windows PowerShell.
fn shell_candidates() -> Vec<CommandBuilder> {
    let home = std::env::var("USERPROFILE").ok();
    let with_home = |mut cmd: CommandBuilder| {
        if let Some(h) = &home {
            cmd.cwd(h);
        }
        cmd
    };

    let mut candidates = Vec::new();
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        let pwsh7 = std::path::Path::new(&program_files)
            .join("PowerShell")
            .join("7")
            .join("pwsh.exe");
        if pwsh7.exists() {
            candidates.push(with_home(CommandBuilder::new(pwsh7)));
        }
    }
    candidates.push(with_home(CommandBuilder::new("pwsh.exe")));
    candidates.push(with_home(CommandBuilder::new("powershell.exe")));
    candidates
}

pub fn spawn_local(state: &AppState) -> Result<u32, String> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TERM_ROWS,
            cols: TERM_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Could not open PTY: {e}"))?;

    // Try each shell candidate until one spawns
    let mut spawn_err = String::from("No shell found");
    let mut child_opt = None;
    for cmd in shell_candidates() {
        match pair.slave.spawn_command(cmd) {
            Ok(c) => {
                child_opt = Some(c);
                break;
            }
            Err(e) => spawn_err = format!("Could not start shell: {e}"),
        }
    }
    let child = child_opt.ok_or(spawn_err)?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("PTY writer error: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("PTY reader error: {e}"))?;

    let id = state.next_session_id.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let session = Arc::new(LocalSession {
        id,
        master: Mutex::new(Some(pair.master)),
        writer: Mutex::new(Some(writer)),
        child: Mutex::new(Some(child)),
        closed: CancellationToken::new(),
    });
    state.local_sessions.lock().unwrap().insert(id, session.clone());

    // Drop our slave handle so the reader sees EOF when the shell exits
    drop(pair.slave);

    // Reader thread: blocking reads, emits to the same channel as SSH
    let app = state.app.clone();
    let session_for_reader = session.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            if session_for_reader.closed.is_cancelled() {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let _ = app.emit(
                        "ssh:stdout",
                        StdoutEvent {
                            session_id: session_for_reader.id,
                            data: B64.encode(&buf[..n]),
                        },
                    );
                }
                Err(_) => break,
            }
        }
        let _ = app.emit(
            "session:status",
            crate::ssh::pool::StatusEvent {
                session_id: session_for_reader.id,
                status: "closed".into(),
                error: None,
            },
        );
        session_for_reader.closed.cancel();
    });

    crate::ssh::pool::emit_status(&state.app, id, "connected", None);
    Ok(id)
}

pub fn local_write(session: &LocalSession, data: &str) -> Result<(), String> {
    let mut guard = session.writer.lock().unwrap();
    let writer = guard.as_mut().ok_or("Local session closed")?;
    writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    writer.flush().map_err(|e| format!("Write failed: {e}"))
}

pub fn local_resize(session: &LocalSession, cols: u16, rows: u16) -> Result<(), String> {
    let guard = session.master.lock().unwrap();
    let master = guard.as_ref().ok_or("Local session closed")?;
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Resize failed: {e}"))
}

pub fn close_local(state: &AppState, session: &LocalSession) {
    session.closed.cancel();
    if let Some(mut child) = session.child.lock().unwrap().take() {
        let _ = child.kill();
    }
    // Dropping master/writer releases the PTY
    *session.master.lock().unwrap() = None;
    *session.writer.lock().unwrap() = None;
    state.local_sessions.lock().unwrap().remove(&session.id);
}
