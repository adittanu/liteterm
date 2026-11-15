//! Private keys kept in LiteTerm itself ("Store the key in LiteTerm"). The key
//! travels inside its credential's encrypted payload, so the credential works
//! on every computer of the account and for every member of a team, with no
//! key file to copy around.
//!
//! On each computer it sits in `credential_keys`, sealed with a key of this
//! device that lives in the OS keyring. Not in the keyring itself: Windows
//! Credential Manager caps an entry at 2,560 bytes, which an RSA-4096 key
//! exceeds.
//!
//! A key is stored unlocked: a passphrase is asked for once, when the key is
//! added, so nobody needs it later.

use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rusqlite::{params, Connection, OptionalExtension};
use zeroize::Zeroizing;

use super::models::CredentialInput;
use crate::cloud::items::Secrets;

/// Keyring entry holding this device's sealing key
const DEVICE_KEY: &str = "local:secrets-key";
const AAD: &[u8] = b"liteterm:credential-key";
const NONCE_LEN: usize = 24;

pub const NO_DEVICE_KEY: &str =
    "The key stored for this credential can't be read on this computer: its keyring entry is gone. \
     Choose the key file again.";

/// This device's sealing key; made the first time one is needed
fn device_key(secrets: &dyn Secrets, create: bool) -> Option<Zeroizing<[u8; 32]>> {
    if let Some(text) = secrets.get(DEVICE_KEY).map(Zeroizing::new) {
        let bytes = Zeroizing::new(STANDARD.decode(text.as_bytes()).ok()?);
        return <[u8; 32]>::try_from(bytes.as_slice()).ok().map(Zeroizing::new);
    }
    if !create {
        return None;
    }
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(key.as_mut());
    secrets.set(DEVICE_KEY, &STANDARD.encode(key.as_ref()));
    // Read back: a keyring that silently dropped it would lose every key sealed now
    device_key(secrets, false).filter(|k| **k == *key)
}

fn cipher(key: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(Key::from_slice(key))
}

/// Store `text` (an unlocked OpenSSH private key) for credential `id`
pub fn store(conn: &Connection, secrets: &dyn Secrets, id: i64, text: &str) -> Result<(), String> {
    let key = device_key(secrets, true)
        .ok_or("The OS keyring didn't keep LiteTerm's key, so the private key can't be stored safely.")?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let sealed = cipher(&key)
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: text.as_bytes(), aad: AAD })
        .map_err(|_| "Could not encrypt the key")?;
    let mut blob = nonce.to_vec();
    blob.extend(sealed);
    conn.execute(
        "INSERT INTO credential_keys (credential_id, sealed) VALUES (?1, ?2) \
         ON CONFLICT(credential_id) DO UPDATE SET sealed = excluded.sealed",
        params![id, STANDARD.encode(blob)],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// The key stored for credential `id`: None when it has none
pub fn load(conn: &Connection, secrets: &dyn Secrets, id: i64) -> Result<Option<Zeroizing<String>>, String> {
    let sealed: Option<String> = conn
        .query_row("SELECT sealed FROM credential_keys WHERE credential_id = ?1", [id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(sealed) = sealed else { return Ok(None) };
    let key = device_key(secrets, false).ok_or(NO_DEVICE_KEY)?;
    let blob = STANDARD.decode(sealed.as_bytes()).map_err(|_| NO_DEVICE_KEY)?;
    if blob.len() <= NONCE_LEN {
        return Err(NO_DEVICE_KEY.into());
    }
    let (nonce, body) = blob.split_at(NONCE_LEN);
    let plain = Zeroizing::new(
        cipher(&key)
            .decrypt(XNonce::from_slice(nonce), Payload { msg: body, aad: AAD })
            .map_err(|_| NO_DEVICE_KEY)?,
    );
    String::from_utf8(plain.to_vec()).map(|s| Some(Zeroizing::new(s))).map_err(|_| NO_DEVICE_KEY.into())
}

/// Drop credential `id`'s stored key; true when it had one
pub fn forget(conn: &Connection, id: i64) -> Result<bool, rusqlite::Error> {
    conn.execute("DELETE FROM credential_keys WHERE credential_id = ?1", [id]).map(|n| n > 0)
}

/// A private key as read from a file (OpenSSH, PEM or PuTTY), opened with
/// `passphrase` if it is locked, as unlocked OpenSSH text
pub fn unlock(text: &str, passphrase: Option<&str>) -> Result<Zeroizing<String>, String> {
    let passphrase = passphrase.filter(|p| !p.is_empty());
    let key = russh::keys::decode_secret_key(text, passphrase).map_err(|e| match (e, passphrase) {
        (russh::keys::Error::KeyIsEncrypted, None) => {
            "This key has a passphrase. Type it below: LiteTerm stores the key unlocked, so it's asked only once."
                .to_string()
        }
        (_, Some(_)) => "That passphrase doesn't unlock this key, or the file isn't a private key.".to_string(),
        (e, None) => format!("That file isn't a private key LiteTerm can read: {e}"),
    })?;
    key.to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(|e| format!("Could not read the key: {e}"))
}

/// The OpenSSH public key of a stored private key
pub fn public_key(text: &str) -> Result<String, String> {
    let key = russh::keys::decode_secret_key(text, None).map_err(|e| format!("Could not read the key: {e}"))?;
    key.public_key().to_openssh().map_err(|e| format!("Could not read the key: {e}"))
}

/// Before a credential is saved: read and unlock the key file it should
/// store, into `input.key_text`. A team credential that signs in with a key
/// has to carry it, because no one else has the file.
pub fn prepare(input: &mut CredentialInput, in_team: bool, stored_now: bool) -> Result<(), String> {
    if input.auth_method != "key" {
        return Ok(());
    }
    if input.store_key == Some(true) {
        let path = input
            .key_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or("Choose the private key file to store")?;
        let path = crate::paths::expand_home(path);
        let text = Zeroizing::new(std::fs::read_to_string(&path).map_err(|e| format!("Can't read {path}: {e}"))?);
        input.key_text = Some(unlock(&text, input.key_passphrase.as_deref())?);
    }
    let keeps_stored = stored_now && input.store_key != Some(false);
    if in_team && input.key_text.is_none() && !keeps_stored {
        return Err("A team credential carries its key: choose the key file and keep “Store the key in LiteTerm” on.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct MemSecrets(RefCell<HashMap<String, String>>);

    impl Secrets for MemSecrets {
        fn get(&self, account: &str) -> Option<String> {
            self.0.borrow().get(account).cloned()
        }
        fn set(&self, account: &str, value: &str) {
            self.0.borrow_mut().insert(account.into(), value.into());
        }
        fn delete(&self, account: &str) {
            self.0.borrow_mut().remove(account);
        }
    }

    fn open() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        crate::store::db::migrate(&conn).unwrap();
        conn.execute("INSERT INTO credentials (id, name, username, auth_method) VALUES (1, 'ops', 'deploy', 'key')", [])
            .unwrap();
        conn
    }

    fn new_key(passphrase: Option<&str>) -> String {
        use rand_core::OsRng;
        use ssh_key::{Algorithm, LineEnding, PrivateKey};
        let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519).unwrap();
        let key = match passphrase {
            Some(p) => key.encrypt(&mut OsRng, p).unwrap(),
            None => key,
        };
        key.to_openssh(LineEnding::LF).unwrap().to_string()
    }

    #[test]
    fn a_stored_key_is_sealed_on_disk_and_opens_with_this_devices_key() {
        let conn = open();
        let secrets = MemSecrets::default();
        let key = new_key(None);
        store(&conn, &secrets, 1, &key).unwrap();
        let sealed: String = conn.query_row("SELECT sealed FROM credential_keys", [], |r| r.get(0)).unwrap();
        assert!(!sealed.contains("PRIVATE KEY"));
        assert_eq!(load(&conn, &secrets, 1).unwrap().as_deref().map(String::as_str), Some(key.as_str()));
        assert_eq!(load(&conn, &secrets, 2).unwrap(), None, "a credential without one");

        // Another computer's keyring can't open it
        assert_eq!(load(&conn, &MemSecrets::default(), 1).unwrap_err(), NO_DEVICE_KEY);

        // Deleting the credential takes its key along
        conn.execute("DELETE FROM credentials WHERE id = 1", []).unwrap();
        let left: i64 = conn.query_row("SELECT count(*) FROM credential_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn a_locked_key_is_stored_unlocked_after_one_passphrase() {
        let locked = new_key(Some("hunter22"));
        assert!(unlock(&locked, None).unwrap_err().contains("has a passphrase"));
        assert!(unlock(&locked, Some("wrong")).unwrap_err().contains("doesn't unlock"));
        let open = unlock(&locked, Some("hunter22")).unwrap();
        assert!(russh::keys::decode_secret_key(&open, None).is_ok(), "no passphrase needed any more");
        assert!(public_key(&open).unwrap().starts_with("ssh-ed25519 "));
        assert!(unlock("not a key", None).is_err());
    }

    #[test]
    fn a_team_credential_must_carry_its_key() {
        let input = || CredentialInput {
            name: "ops".into(),
            username: "deploy".into(),
            auth_method: "key".into(),
            password: None,
            save_password: None,
            key_path: Some("/nowhere/id_ed25519".into()),
            key_passphrase: None,
            team_id: None,
            store_key: None,
            key_text: None,
        };
        assert!(prepare(&mut input(), true, false).unwrap_err().contains("team credential"));
        assert_eq!(prepare(&mut input(), true, true), Ok(()), "it has one stored already");
        assert_eq!(prepare(&mut input(), false, false), Ok(()), "a personal one may use a file");
        let mut drop = input();
        drop.store_key = Some(false);
        assert!(prepare(&mut drop, true, true).is_err(), "and can't drop it");

        let dir = std::env::temp_dir().join(format!("liteterm-stored-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("id_ed25519");
        std::fs::write(&path, new_key(Some("pp"))).unwrap();
        let mut from_file = input();
        from_file.key_path = Some(path.to_string_lossy().into_owned());
        from_file.store_key = Some(true);
        from_file.key_passphrase = Some("pp".into());
        prepare(&mut from_file, true, false).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        assert!(from_file.key_text.is_some_and(|t| t.contains("OPENSSH PRIVATE KEY")));
    }
}
