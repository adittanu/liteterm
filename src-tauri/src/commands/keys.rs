//! SSH keys: make a new Ed25519 key in ~/.ssh, read the public half of a
//! key file, and add a public key to a server's authorized_keys (what
//! `ssh-keygen` and `ssh-copy-id` do).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use ssh_key::rand_core::OsRng;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use tauri::State;

use crate::ssh::pool::{self, AppState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyDefaults {
    /// A file name in ~/.ssh not taken yet
    pub name: String,
    /// user@computer, as ssh-keygen writes it
    pub comment: String,
    pub dir: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedKey {
    pub path: String,
    pub public_key: String,
    pub fingerprint: String,
}

fn ssh_dir() -> Result<PathBuf, String> {
    crate::paths::home_dir()
        .map(|h| h.join(".ssh"))
        .ok_or_else(|| "Could not find your home folder".into())
}

fn computer_name() -> String {
    let from_env = ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()));
    from_env
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "liteterm".into())
}

fn free_name(dir: &Path) -> String {
    let taken = |n: &str| dir.join(n).exists() || dir.join(format!("{n}.pub")).exists();
    if !taken("id_ed25519") {
        return "id_ed25519".into();
    }
    (2..)
        .map(|i| format!("id_ed25519_{i}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| "id_ed25519_liteterm".into())
}

/// Letters, digits, `.`, `_` and `-` only: a plain file name inside ~/.ssh
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[tauri::command]
pub fn keygen_defaults() -> Result<KeyDefaults, String> {
    let dir = ssh_dir()?;
    let user = ["USERNAME", "USER"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "me".into());
    Ok(KeyDefaults {
        name: free_name(&dir),
        comment: format!("{user}@{}", computer_name()),
        dir: dir.to_string_lossy().into_owned(),
    })
}

/// A new Ed25519 key pair in ~/.ssh/<name> and <name>.pub, in OpenSSH's own
/// format (encrypted with the passphrase when one is given). Never replaces
/// an existing file.
#[tauri::command]
pub fn keygen(name: String, comment: String, passphrase: Option<String>) -> Result<GeneratedKey, String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err("Use letters, digits, dots, dashes or underscores for the file name".into());
    }
    let dir = ssh_dir()?;
    let path = dir.join(name);
    let pub_path = dir.join(format!("{name}.pub"));
    if path.exists() || pub_path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;

    let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).map_err(|e| e.to_string())?;
    key.set_comment(comment.trim());
    let public_key = key.public_key().to_openssh().map_err(|e| e.to_string())?;
    let fingerprint = key.public_key().fingerprint(HashAlg::Sha256).to_string();
    let key = match passphrase.as_deref().filter(|p| !p.is_empty()) {
        Some(pass) => key.encrypt(&mut OsRng, pass).map_err(|e| e.to_string())?,
        None => key,
    };
    let pem = key.to_openssh(LineEnding::LF).map_err(|e| e.to_string())?;

    write_private(&path, pem.as_bytes())?;
    if let Err(e) = std::fs::write(&pub_path, format!("{public_key}\n")) {
        let _ = std::fs::remove_file(&path);
        return Err(format!("Could not write {}: {e}", pub_path.display()));
    }
    Ok(GeneratedKey { path: path.to_string_lossy().into_owned(), public_key, fingerprint })
}

/// Create the private key file readable by its owner only
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path).map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    file.write_all(bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// The public key (one authorized_keys line) of a private key file: its
/// `.pub` neighbour, else read from the key itself. OpenSSH-format keys keep
/// the public half unencrypted, so no passphrase is needed for them.
#[tauri::command]
pub fn public_key_of(path: String) -> Result<String, String> {
    let path = PathBuf::from(crate::paths::expand_home(path.trim()));
    let pub_path = PathBuf::from(format!("{}.pub", path.display()));
    if let Ok(text) = std::fs::read_to_string(&pub_path) {
        if let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) {
            if PublicKey::from_openssh(line).is_ok() {
                return Ok(line.to_string());
            }
        }
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {}: {e}", path.display()))?;
    if let Ok(key) = PrivateKey::from_openssh(&text) {
        return key.public_key().to_openssh().map_err(|e| e.to_string());
    }
    // PEM / PKCS#8 keys (older RSA ones): only readable unlocked
    let pair = russh::keys::load_secret_key(&path, None).map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => {
            format!("This key is locked. Put its public key next to it as {}", pub_path.display())
        }
        other => format!("Could not read the key: {other}"),
    })?;
    use russh::keys::PublicKeyBase64;
    let public = pair.clone_public_key().map_err(|e| e.to_string())?;
    let algo = match public.name() {
        n if n.starts_with("rsa-sha2") => "ssh-rsa",
        n => n,
    };
    Ok(format!("{algo} {}", public.public_key_base64()))
}

/// Add `public_key` to ~/.ssh/authorized_keys on the server behind this
/// session. Returns false when the key was already there. Creates ~/.ssh
/// (0700) and the file (0600) when missing.
#[tauri::command]
pub async fn install_public_key(
    state: State<'_, Arc<AppState>>,
    session_id: u32,
    public_key: String,
) -> Result<bool, String> {
    let parsed = PublicKey::from_openssh(public_key.trim()).map_err(|_| "That is not an SSH public key")?;
    let line = parsed.to_openssh().map_err(|e| e.to_string())?;
    let data = key_data(&line).ok_or("That is not an SSH public key")?.to_string();

    let session = state
        .sessions
        .lock()
        .unwrap()
        .get(&session_id)
        .cloned()
        .ok_or("Session not found")?;
    pool::sftp_open(state.inner(), &session).await?;
    pool::sftp_ensure_private_dir(&session, ".ssh").await?;

    let existing = pool::sftp_read_tail(&session, ".ssh/authorized_keys", 4 * 1024 * 1024)
        .await
        .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    if existing.lines().any(|l| key_data(l) == Some(data.as_str())) {
        return Ok(false);
    }
    let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
    pool::sftp_append_private(&session, ".ssh/authorized_keys", format!("{sep}{line}\n").as_bytes()).await?;
    Ok(true)
}

/// The base64 key field of an authorized_keys line, after any options
/// (`from="…",no-pty ssh-ed25519 AAAA… comment`)
fn key_data(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let fields: Vec<&str> = line.split_whitespace().collect();
    let algo = fields.iter().position(|f| f.starts_with("ssh-") || f.starts_with("ecdsa-") || f.starts_with("sk-"))?;
    fields.get(algo + 1).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_data_skips_options_and_comments() {
        assert_eq!(key_data("ssh-ed25519 AAAAC3 me@pc"), Some("AAAAC3"));
        assert_eq!(key_data(r#"from="10.0.0.1",no-pty ssh-rsa AAAAB3 x"#), Some("AAAAB3"));
        assert_eq!(key_data("# ssh-ed25519 AAAAC3"), None);
        assert_eq!(key_data(""), None);
    }

    #[test]
    fn names_stay_inside_the_ssh_folder() {
        assert!(valid_name("id_ed25519_work"));
        assert!(!valid_name("../id"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(".hidden"));
        assert!(!valid_name(""));
    }

    #[test]
    fn a_generated_key_loads_with_and_without_passphrase() {
        for pass in [None, Some("correct horse")] {
            let mut key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
            key.set_comment("t@pc");
            let public = key.public_key().to_openssh().unwrap();
            let key = match pass {
                Some(p) => key.encrypt(&mut OsRng, p).unwrap(),
                None => key,
            };
            let pem = key.to_openssh(LineEnding::LF).unwrap();
            // The SSH client (russh) must accept what we write
            let loaded = russh::keys::decode_secret_key(&pem, pass).unwrap();
            use russh::keys::PublicKeyBase64;
            assert_eq!(key_data(&public), Some(loaded.clone_public_key().unwrap().public_key_base64().as_str()));
            // and the public half stays readable without the passphrase (the
            // comment sits in the encrypted part, so only the key is compared)
            let read_back = PrivateKey::from_openssh(&*pem).unwrap().public_key().to_openssh().unwrap();
            assert_eq!(key_data(&read_back), key_data(&public));
        }
    }
}
