//! Per-user locations that are spelled differently on Windows and Unix.

use std::path::PathBuf;

/// The user's home directory: `USERPROFILE` on Windows (with `HOME` as a
/// fallback, which Git Bash and friends set), `HOME` everywhere else.
pub fn home_dir() -> Option<PathBuf> {
    let vars: &[&str] = if cfg!(windows) { &["USERPROFILE", "HOME"] } else { &["HOME"] };
    vars.iter()
        .find_map(|name| std::env::var_os(name).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}
