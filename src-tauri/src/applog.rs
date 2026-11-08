//! A small append-only log next to the database. The release build has no
//! console (`windows_subsystem = "windows"`), so without this a panic or a
//! swallowed error leaves no trace at all.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;

/// One call site is written at most this often: a failing emit inside a hot
/// output loop must not turn into a log flood.
const THROTTLE: Duration = Duration::from_secs(10);

/// `liteterm.log` in the folder that holds hosts.db: `%APPDATA%\com.liteterm.app`
/// on Windows, `~/Library/Application Support/com.liteterm.app` on macOS,
/// `~/.local/share/com.liteterm.app` on Linux. Tauri is not up yet when the
/// panic hook is installed, so Tauri's own `app_data_dir` is mirrored by hand.
pub fn log_path() -> Option<PathBuf> {
    Some(data_base()?.join("com.liteterm.app").join("liteterm.log"))
}

#[cfg(windows)]
fn data_base() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn data_base() -> Option<PathBuf> {
    Some(crate::paths::home_dir()?.join("Library").join("Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn data_base() -> Option<PathBuf> {
    // XDG: an unset, empty or relative XDG_DATA_HOME is ignored
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| Some(crate::paths::home_dir()?.join(".local").join("share")))
}

pub fn write(line: &str) {
    if let Some(path) = log_path() {
        write_to(&path, line);
    }
}

fn write_to(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    // Keep one previous generation instead of growing without bound
    if fs::metadata(path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = fs::rename(path, path.with_extension("log.old"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "[{}] {line}", utc_stamp(unix_now()));
    }
}

fn throttle_allows(
    seen: &mut HashMap<&'static str, Instant>,
    site: &'static str,
    now: Instant,
) -> bool {
    if let Some(prev) = seen.get(site) {
        if now.duration_since(*prev) < THROTTLE {
            return false;
        }
    }
    seen.insert(site, now);
    true
}

/// Like [`write`], but a given `site` logs at most once per [`THROTTLE`].
pub fn write_throttled(site: &'static str, line: &str) {
    static LAST: OnceLock<Mutex<HashMap<&'static str, Instant>>> = OnceLock::new();
    let last = LAST.get_or_init(|| Mutex::new(HashMap::new()));
    let allowed = match last.lock() {
        Ok(mut seen) => throttle_allows(&mut seen, site, Instant::now()),
        Err(_) => false,
    };
    if allowed {
        write(line);
    }
}

/// Record panics (there is no console for them to land on in the release
/// build) and still hand them to the previous hook, so `tauri dev` keeps
/// printing them.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        write(&format!(
            "PANIC on thread {}: {info}",
            thread.name().unwrap_or("<unnamed>")
        ));
        previous(info);
    }));
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Unix seconds to "YYYY-MM-DD HH:MM:SS" (UTC), without a date crate.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // civil-from-days (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_known_instants() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29 00:00:00"); // leap day
        assert_eq!(utc_stamp(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(utc_stamp(4_102_444_799), "2099-12-31 23:59:59");
    }

    #[test]
    fn appends_and_rotates() {
        let dir = std::env::temp_dir().join(format!("liteterm-applog-test-{}", std::process::id()));
        let path = dir.join("liteterm.log");
        let _ = fs::remove_dir_all(&dir);

        write_to(&path, "first");
        write_to(&path, "second");
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(text.lines().next().unwrap().ends_with("] first"));

        // Past the size cap the file is rotated to .log.old and a new one starts
        fs::write(&path, vec![b'x'; (MAX_LOG_BYTES + 1) as usize]).unwrap();
        write_to(&path, "after rotation");
        assert!(path.with_extension("log.old").exists());
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn throttle_lets_one_call_through_per_site_per_window() {
        let mut seen = HashMap::new();
        let t0 = Instant::now();
        assert!(throttle_allows(&mut seen, "a", t0));
        assert!(!throttle_allows(&mut seen, "a", t0 + Duration::from_secs(1)));
        assert!(throttle_allows(&mut seen, "b", t0 + Duration::from_secs(1)));
        assert!(throttle_allows(&mut seen, "a", t0 + THROTTLE));
    }
}
