//! Byte-level trace of SSH sessions, compiled in only with `--features diag`.
//!
//! Each session gets `%APPDATA%\com.liteterm.app\trace\session-<pid>-<id>.log`
//! with one line per event: milliseconds since the first event, a tag
//! (OUT = server → terminal, IN = terminal → server, …), the byte count and
//! the bytes with control characters escaped. Comparing a `tauri dev` trace
//! with an installer trace shows where two runs of the same TUI diverge.
//! Without the feature every call is an empty inline function.

#[cfg(feature = "diag")]
mod imp {
    use std::collections::HashMap;
    use std::fs::{self, File, OpenOptions};
    use std::io::Write;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    /// Stop writing a session's file past this size; a TUI repaints forever
    const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

    struct Sink {
        file: File,
        written: u64,
    }

    struct Tracer {
        start: Instant,
        sinks: HashMap<u32, Sink>,
    }

    fn tracer() -> &'static Mutex<Tracer> {
        static T: OnceLock<Mutex<Tracer>> = OnceLock::new();
        T.get_or_init(|| {
            Mutex::new(Tracer {
                start: Instant::now(),
                sinks: HashMap::new(),
            })
        })
    }

    fn open(session_id: u32) -> Option<Sink> {
        let dir = crate::applog::log_path()?.parent()?.join("trace");
        fs::create_dir_all(&dir).ok()?;
        let path = dir.join(format!("session-{}-{session_id}.log", std::process::id()));
        let mut file = OpenOptions::new().create(true).append(true).open(path).ok()?;
        let mode = if cfg!(debug_assertions) { "debug" } else { "release" };
        let header = format!("# liteterm {} {mode} build\n", env!("CARGO_PKG_VERSION"));
        file.write_all(header.as_bytes()).ok()?;
        Some(Sink {
            file,
            written: header.len() as u64,
        })
    }

    fn escape(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len() + 16);
        for &b in data {
            match b {
                0x1b => out.push_str("\\e"),
                b'\r' => out.push_str("\\r"),
                b'\n' => out.push_str("\\n"),
                b'\t' => out.push_str("\\t"),
                b'\\' => out.push_str("\\\\"),
                0x20..=0x7e => out.push(b as char),
                _ => out.push_str(&format!("\\x{b:02x}")),
            }
        }
        out
    }

    pub fn record(session_id: u32, tag: &str, data: &[u8]) {
        let Ok(mut t) = tracer().lock() else { return };
        let ms = t.start.elapsed().as_secs_f64() * 1000.0;
        let sink = match t.sinks.entry(session_id) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let Some(sink) = open(session_id) else { return };
                e.insert(sink)
            }
        };
        if sink.written > MAX_FILE_BYTES {
            return;
        }
        let line = format!("{ms:>11.3} {tag:<6} {:>6} {}\n", data.len(), escape(data));
        if sink.file.write_all(line.as_bytes()).is_ok() {
            sink.written += line.len() as u64;
        }
    }

    pub fn note(session_id: u32, tag: &str, text: impl FnOnce() -> String) {
        record(session_id, tag, text().as_bytes());
    }
}

#[cfg(feature = "diag")]
pub use imp::{note, record};

#[cfg(not(feature = "diag"))]
#[inline(always)]
pub fn record(_session_id: u32, _tag: &str, _data: &[u8]) {}

/// Takes a closure so a normal build never formats the text at all
#[cfg(not(feature = "diag"))]
#[inline(always)]
pub fn note(_session_id: u32, _tag: &str, _text: impl FnOnce() -> String) {}
