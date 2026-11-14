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

/// `~/.ssh/id_ed25519` → that file under the user's home, as a pasted
/// `ssh -i` command or a synced host from another computer writes it.
/// Anything not starting with `~/` (or `~\`) comes back unchanged.
pub fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) else {
        return path.to_string();
    };
    match home_dir() {
        Some(home) => home.join(rest).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_only_touches_a_leading_tilde() {
        let home = home_dir().expect("tests run with a home directory");
        assert_eq!(expand_home("~/.ssh/id"), home.join(".ssh/id").to_string_lossy());
        assert_eq!(expand_home("/etc/key"), "/etc/key");
        assert_eq!(expand_home("C:\\keys\\id"), "C:\\keys\\id");
        assert_eq!(expand_home("~user/key"), "~user/key");
    }
}