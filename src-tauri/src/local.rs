//! Local terminal sessions over a PTY (portable-pty: ConPTY on Windows,
//! openpty on macOS/Linux).
//! Reuses the same event channel as SSH sessions: `ssh:stdout`,
//! `session:status` — the frontend terminal is source-agnostic.

use std::io::{Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use portable_pty::{CommandBuilder, MasterPty, PtySize};
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

use crate::ssh::pool::{AppState, StdoutEvent};

const TERM_COLS: u16 = 80;
const TERM_ROWS: u16 = 24;
const STDOUT_FLUSH: Duration = Duration::from_millis(8);
const STDOUT_FLUSH_BYTES: usize = 32 * 1024;

pub struct LocalSession {
    pub id: u32,
    pub master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    pub writer: Mutex<Option<Box<dyn Write + Send>>>,
    pub child: Mutex<Option<Box<dyn portable_pty::Child + Send + Sync>>>,
    pub closed: CancellationToken,
}

/// Start in the user's home directory, like a freshly opened terminal
fn in_home(mut cmd: CommandBuilder) -> CommandBuilder {
    if let Some(home) = crate::paths::home_dir() {
        cmd.cwd(home);
    }
    // A GUI process has no TERM. Windows Terminal advertises these; TUIs
    // such as opencode fall back to a slower, broken renderer without them.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}

/// Shell candidates in preference order: pwsh 7 from its default install
/// location, pwsh from PATH (scoop/winget), then Windows PowerShell.
#[cfg(windows)]
fn shell_candidates() -> Vec<CommandBuilder> {
    let mut candidates = Vec::new();
    if let Ok(program_files) = std::env::var("ProgramFiles") {
        let pwsh7 = std::path::Path::new(&program_files)
            .join("PowerShell")
            .join("7")
            .join("pwsh.exe");
        if pwsh7.exists() {
            candidates.push(in_home(CommandBuilder::new(pwsh7)));
        }
    }
    candidates.push(in_home(CommandBuilder::new("pwsh.exe")));
    candidates.push(in_home(CommandBuilder::new("powershell.exe")));
    candidates
}

/// The user's own shell ($SHELL, else the passwd entry), started as a login
/// shell — portable-pty's default program. A GUI app launched from Finder has
/// only the bare system PATH; a login shell reads the user's profile and gets
/// Homebrew and friends back, as Terminal.app does.
#[cfg(not(windows))]
fn shell_candidates() -> Vec<CommandBuilder> {
    vec![in_home(CommandBuilder::new_default_prog())]
}

/// What `command` (e.g. the herdr TUI) is started as.
#[cfg(windows)]
fn command_candidate(command: &str) -> CommandBuilder {
    in_home(CommandBuilder::new(command))
}

/// Through the login shell, so the program is found where a terminal would
/// find it (see `shell_candidates`: the app's own PATH is the bare one).
#[cfg(not(windows))]
fn command_candidate(command: &str) -> CommandBuilder {
    let shell = CommandBuilder::new_default_prog().get_shell();
    let script = format!("exec {}", strip_exe(command));
    let mut cmd = CommandBuilder::new(shell);
    cmd.args(["-l", "-c", script.as_str()]);
    in_home(cmd)
}

/// The frontend asks for `herdr.exe`, the Windows spelling; elsewhere the
/// program is plain `herdr`.
#[cfg(any(not(windows), test))]
fn strip_exe(command: &str) -> &str {
    match command.len().checked_sub(4) {
        Some(at)
            if command.is_char_boundary(at) && command[at..].eq_ignore_ascii_case(".exe") =>
        {
            &command[..at]
        }
        _ => command,
    }
}

/// `command` runs in place of the default shell (e.g. the herdr TUI);
/// reader, event, and write/resize wiring is identical either way.
pub fn spawn_local(state: &AppState, command: Option<&str>) -> Result<u32, String> {
    let pty_system = portable_pty::native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: TERM_ROWS,
            cols: TERM_COLS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Could not open PTY: {e}"))?;

    // Try each spawn candidate until one starts
    let candidates: Vec<CommandBuilder> = match command {
        Some(cmd) => vec![command_candidate(cmd)],
        None => shell_candidates(),
    };
    let mut spawn_err = String::from("No shell found");
    let mut child_opt = None;
    for cmd in candidates {
        match pair.slave.spawn_command(cmd) {
            Ok(c) => {
                child_opt = Some(c);
                break;
            }
            Err(e) => spawn_err = format!("Could not start program: {e}"),
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

    // Announce "connected" before the reader threads exist: a shell that dies
    // at once would otherwise let its "closed" overtake this and leave the
    // tab showing a dead terminal as connected
    crate::ssh::pool::emit_status(&state.app, id, "connected", None);

    // The reader only pushes bytes. A second thread coalesces them: a
    // release build fills this pipe much faster than `tauri dev`, and one
    // IPC event per read sticks xterm's painter (opencode draws halfway).
    // Short output still leaves within STDOUT_FLUSH; it is not held until
    // the next keystroke.
    let app = state.app.clone();
    let session_for_reader = session.clone();
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let app_for_flush = app.clone();
    let flush_id = session.id;
    std::thread::spawn(move || {
        flush_loop(rx, |bytes| emit_stdout(&app_for_flush, flush_id, bytes));
        crate::ssh::pool::emit_status(&app_for_flush, flush_id, "closed", None);
    });
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 32 * 1024];
        loop {
            if session_for_reader.closed.is_cancelled() {
                break;
            }
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        session_for_reader.closed.cancel();
        drop(tx);
    });

    Ok(id)
}

fn emit_stdout(app: &tauri::AppHandle, session_id: u32, bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    // A dropped chunk corrupts the screen, and the release build has no
    // console to say so — leave a (throttled) line in the app log instead
    if let Err(e) = app.emit(
        "ssh:stdout",
        StdoutEvent {
            session_id,
            data: B64.encode(bytes),
        },
    ) {
        crate::applog::write_throttled(
            "local stdout emit",
            &format!("could not deliver terminal output of session {session_id}: {e}"),
        );
    }
}

/// Coalesce PTY output into as few IPC events as possible without adding
/// echo lag: bytes wait at most `STDOUT_FLUSH` after the first byte of a
/// batch arrives, and go out at once when a full frame is buffered. The wait
/// is a deadline, not an idle timeout — a steady trickle of small chunks must
/// not keep postponing the flush (an idle timeout never fires while chunks
/// keep arriving faster than it).
fn flush_loop(rx: mpsc::Receiver<Vec<u8>>, mut emit: impl FnMut(&[u8])) {
    let mut pending: Vec<u8> = Vec::new();
    let mut deadline: Option<Instant> = None;
    loop {
        let received = match deadline {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(chunk) => {
                if pending.is_empty() {
                    deadline = Some(Instant::now() + STDOUT_FLUSH);
                }
                pending.extend_from_slice(&chunk);
                // Whatever is already queued rides along, up to one frame
                while pending.len() < STDOUT_FLUSH_BYTES {
                    match rx.try_recv() {
                        Ok(more) => pending.extend_from_slice(&more),
                        Err(_) => break,
                    }
                }
                if pending.len() >= STDOUT_FLUSH_BYTES
                    || deadline.is_some_and(|at| Instant::now() >= at)
                {
                    emit(&pending);
                    pending.clear();
                    deadline = None;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !pending.is_empty() {
                    emit(&pending);
                    pending.clear();
                }
                deadline = None;
            }
            Err(RecvTimeoutError::Disconnected) => {
                if !pending.is_empty() {
                    emit(&pending);
                }
                return;
            }
        }
    }
}

pub fn local_write(session: &LocalSession, data: &str) -> Result<(), String> {
    let mut guard = session.writer.lock().unwrap();
    let writer = guard.as_mut().ok_or("Local session closed")?;
    writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;
    writer.flush().map_err(|e| format!("Write failed: {e}"))
}

pub fn local_resize(
    session: &LocalSession,
    cols: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
) -> Result<(), String> {
    let guard = session.master.lock().unwrap();
    let master = guard.as_ref().ok_or("Local session closed")?;
    master
        .resize(PtySize {
            rows,
            cols,
            pixel_width,
            pixel_height,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    type Log = Arc<Mutex<Vec<(Instant, Vec<u8>)>>>;

    /// Runs `flush_loop` on its own thread, recording when each batch left.
    fn spawn_flusher(rx: mpsc::Receiver<Vec<u8>>) -> (thread::JoinHandle<()>, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        let handle = thread::spawn(move || {
            flush_loop(rx, |bytes| {
                sink.lock().unwrap().push((Instant::now(), bytes.to_vec()));
            });
        });
        (handle, log)
    }

    #[test]
    fn every_byte_arrives_in_order() {
        let (tx, rx) = mpsc::channel();
        let (handle, log) = spawn_flusher(rx);
        let mut expected = Vec::new();
        for i in 0..5000u32 {
            let chunk = i.to_le_bytes().to_vec();
            expected.extend_from_slice(&chunk);
            tx.send(chunk).unwrap();
        }
        drop(tx);
        handle.join().unwrap();
        let got: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn a_steady_trickle_still_flushes_on_the_deadline() {
        let (tx, rx) = mpsc::channel();
        let (handle, log) = spawn_flusher(rx);
        // 10 bytes every ~2 ms for 300 ms: far below one frame, and faster
        // than STDOUT_FLUSH, so an idle timeout would never fire. (Spin
        // instead of sleeping — Windows sleeps are too coarse for 2 ms.)
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(300) {
            tx.send(vec![b'x'; 10]).unwrap();
            let gap = Instant::now();
            while gap.elapsed() < Duration::from_millis(2) {
                std::hint::spin_loop();
            }
        }
        let sender_done = Instant::now();
        let flushed_while_streaming = log
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| *at < sender_done)
            .count();
        drop(tx);
        handle.join().unwrap();
        // 300 ms / 8 ms is roughly 37 batches; stay generous for slow machines
        assert!(
            flushed_while_streaming >= 5,
            "flushed only {flushed_while_streaming} times while output was streaming"
        );
    }

    #[test]
    fn exe_suffix_is_dropped_only_when_it_is_the_extension() {
        assert_eq!(strip_exe("herdr.exe"), "herdr");
        assert_eq!(strip_exe("HERDR.EXE"), "HERDR");
        assert_eq!(strip_exe("herdr"), "herdr");
        assert_eq!(strip_exe("exe"), "exe");
        assert_eq!(strip_exe("éabc"), "éabc"); // 4 bytes back lands mid-character
    }

    #[test]
    fn closing_flushes_what_is_left() {
        let (tx, rx) = mpsc::channel();
        let (handle, log) = spawn_flusher(rx);
        tx.send(b"tail".to_vec()).unwrap();
        drop(tx);
        handle.join().unwrap();
        let got: Vec<u8> = log
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        assert_eq!(got, b"tail");
    }
}
