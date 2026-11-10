//! End-to-end encryption for cloud sync.
//!
//! - The **vault key** is 32 random bytes made once per account. It encrypts
//!   every synced item and never leaves a device unencrypted.
//! - The **master password** goes through Argon2id to a key that wraps
//!   (encrypts) the vault key. The server stores the wrapped vault key and the
//!   Argon2 parameters. Neither the password nor anything derived from it is
//!   ever sent; login is a separate email code.
//! - The **recovery key** is 160 random bits, shown once at setup as 32
//!   characters. It wraps a second copy of the vault key, for a forgotten
//!   master password.
//!
//! Changing the master password re-wraps the same vault key, so nothing synced
//! needs re-encrypting. Losing both the password and the recovery key loses
//! the data: no other copy of the vault key exists for a server to hand out.
//!
//! Every ciphertext is XChaCha20-Poly1305 under a random 24-byte nonce, stored
//! as base64(version || nonce || ciphertext and tag). The associated data
//! binds each one to its purpose, and an item to its sync_id, so the server
//! cannot pass one item's ciphertext off as another's.

use std::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chacha20poly1305::aead::{rand_core::RngCore, Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

/// Shortest master password accepted when one is set
pub const MIN_PASSWORD_CHARS: usize = 10;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SALT_LEN: usize = 16;
/// 160 bits: 32 Crockford base32 characters, no padding
const RECOVERY_LEN: usize = 20;
const RECOVERY_CHARS: usize = RECOVERY_LEN * 8 / 5;
const ENVELOPE_V1: u8 = 1;

/// Argon2id cost for a password set now: 64 MiB, 3 passes, 1 lane
const ARGON2_M_KIB: u32 = 64 * 1024;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 1;
/// Parameters come from the server. Past these, refuse rather than allocate
/// gigabytes or spin for minutes.
const ARGON2_MAX_M_KIB: u32 = 1024 * 1024;
const ARGON2_MAX_T: u32 = 16;
const ARGON2_MAX_P: u32 = 8;

const AAD_WRAP_PASSWORD: &[u8] = b"liteterm:vault-key:password";
const AAD_WRAP_RECOVERY: &[u8] = b"liteterm:vault-key:recovery";
const RECOVERY_HKDF_INFO: &[u8] = b"liteterm:recovery-key:v1";

/// Crockford's base32: no I, L, O or U, so a written-down key reads back
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    PasswordTooShort,
    WrongPassword,
    WrongRecoveryKey,
    MalformedRecoveryKey,
    UnsupportedKdf,
    Corrupt,
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PasswordTooShort => write!(
                f,
                "The master password needs at least {MIN_PASSWORD_CHARS} characters"
            ),
            Self::WrongPassword => f.write_str("Wrong master password"),
            Self::WrongRecoveryKey => f.write_str("That recovery key does not open this vault"),
            Self::MalformedRecoveryKey => write!(
                f,
                "A recovery key is {RECOVERY_CHARS} letters and digits, like XXXX-XXXX-…"
            ),
            Self::UnsupportedKdf => {
                f.write_str("This vault's settings are not supported by this version of LiteTerm")
            }
            Self::Corrupt => f.write_str("Vault data is damaged"),
        }
    }
}

impl std::error::Error for VaultError {}

/// How the master password becomes the key that wraps the vault key. Stored
/// as `vaults.kdf`: `{"alg":"argon2id","m":65536,"t":3,"p":1,"salt":"…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub alg: String,
    /// Memory, KiB
    pub m: u32,
    /// Passes
    pub t: u32,
    /// Lanes
    pub p: u32,
    /// base64
    pub salt: String,
}

impl KdfParams {
    fn fresh(m: u32, t: u32, p: u32) -> Self {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        Self { alg: "argon2id".into(), m, t, p, salt: B64.encode(salt) }
    }

    fn derive(&self, password: &str) -> Result<Zeroizing<[u8; KEY_LEN]>, VaultError> {
        if self.alg != "argon2id"
            || self.m > ARGON2_MAX_M_KIB
            || self.t > ARGON2_MAX_T
            || self.p > ARGON2_MAX_P
        {
            return Err(VaultError::UnsupportedKdf);
        }
        let salt = B64.decode(&self.salt).map_err(|_| VaultError::Corrupt)?;
        let params = Params::new(self.m, self.t, self.p, Some(KEY_LEN))
            .map_err(|_| VaultError::UnsupportedKdf)?;
        // The same password typed on another OS or keyboard layout can arrive
        // as different code points (é as one, or as e + accent). RFC 8265
        // normalizes to NFC; without it such a user is locked out.
        let password = Zeroizing::new(password.nfc().collect::<String>());
        let mut kek = Zeroizing::new([0u8; KEY_LEN]);
        Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
            .hash_password_into(password.as_bytes(), &salt, kek.as_mut())
            .map_err(|_| VaultError::UnsupportedKdf)?;
        Ok(kek)
    }
}

/// The key every synced item is encrypted with
pub struct VaultKey(Zeroizing<[u8; KEY_LEN]>);

impl fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VaultKey(…)")
    }
}

impl VaultKey {
    fn generate() -> Self {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        OsRng.fill_bytes(key.as_mut());
        Self(key)
    }

    fn from_unwrapped(bytes: &[u8]) -> Result<Self, VaultError> {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        if bytes.len() != KEY_LEN {
            return Err(VaultError::Corrupt);
        }
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// The raw key as base64, for this device's OS keyring: an unlocked
    /// device stays unlocked across restarts, like its saved SSH passwords.
    pub fn export(&self) -> Zeroizing<String> {
        // Sized up front: a growing String would leave unzeroed copies behind
        let mut out = Zeroizing::new(String::with_capacity(KEY_LEN.div_ceil(3) * 4));
        B64.encode_string(self.0.as_ref(), &mut out);
        out
    }

    /// Reads back what [`VaultKey::export`] wrote
    pub fn import(text: &str) -> Result<Self, VaultError> {
        let bytes = Zeroizing::new(B64.decode(text.trim()).map_err(|_| VaultError::Corrupt)?);
        Self::from_unwrapped(&bytes)
    }

    /// `data` for vault_items, bound to the item's sync_id
    pub fn encrypt_item(&self, sync_id: &str, plaintext: &[u8]) -> String {
        seal(&self.0, &item_aad(sync_id), plaintext)
    }

    pub fn decrypt_item(&self, sync_id: &str, data: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        open(&self.0, &item_aad(sync_id), data).map_err(|_| VaultError::Corrupt)
    }
}

fn item_aad(sync_id: &str) -> Vec<u8> {
    // Postgres prints uuids in lower case; match whatever case came in
    format!("liteterm:item:{}", sync_id.to_ascii_lowercase()).into_bytes()
}

/// The second way into a vault, for a forgotten master password
pub struct RecoveryKey(Zeroizing<[u8; RECOVERY_LEN]>);

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryKey(…)")
    }
}

impl RecoveryKey {
    fn generate() -> Self {
        let mut key = Zeroizing::new([0u8; RECOVERY_LEN]);
        OsRng.fill_bytes(key.as_mut());
        Self(key)
    }

    /// Reads a key back the way people type it: any case, with or without
    /// dashes and spaces, O for 0 and I or L for 1.
    pub fn parse(text: &str) -> Result<Self, VaultError> {
        let mut key = Zeroizing::new([0u8; RECOVERY_LEN]);
        let (mut acc, mut bits, mut chars, mut len) = (0u32, 0u32, 0usize, 0usize);
        for c in text.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
            let value = crockford_value(c).ok_or(VaultError::MalformedRecoveryKey)?;
            chars += 1;
            if chars > RECOVERY_CHARS {
                return Err(VaultError::MalformedRecoveryKey);
            }
            acc = (acc << 5) | value;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                key[len] = (acc >> bits) as u8;
                len += 1;
                acc &= (1 << bits) - 1;
            }
        }
        if chars != RECOVERY_CHARS {
            return Err(VaultError::MalformedRecoveryKey);
        }
        Ok(Self(key))
    }

    /// Eight groups of four: `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX`
    pub fn to_display_string(&self) -> Zeroizing<String> {
        // Sized up front: a growing String would leave unzeroed copies behind
        let mut out = Zeroizing::new(String::with_capacity(RECOVERY_CHARS + RECOVERY_CHARS / 4));
        let (mut acc, mut bits, mut written) = (0u32, 0u32, 0usize);
        for &byte in self.0.iter() {
            acc = (acc << 8) | u32::from(byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                if written > 0 && written % 4 == 0 {
                    out.push('-');
                }
                out.push(CROCKFORD[((acc >> bits) & 31) as usize] as char);
                written += 1;
            }
            acc &= (1 << bits) - 1;
        }
        out
    }

    fn wrapping_key(&self) -> Zeroizing<[u8; KEY_LEN]> {
        // 160 random bits need no slow KDF, only expanding to a cipher key
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        Hkdf::<Sha256>::new(None, self.0.as_ref())
            .expand(RECOVERY_HKDF_INFO, key.as_mut())
            .expect("32 bytes is a valid HKDF-SHA256 output length");
        key
    }
}

fn crockford_value(c: char) -> Option<u32> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    CROCKFORD.iter().position(|&a| char::from(a) == c).map(|i| i as u32)
}

enum OpenError {
    /// Not an envelope this version can read
    Malformed,
    /// Wrong key, wrong associated data, or tampered with
    Rejected,
}

fn seal(key: &[u8; KEY_LEN], aad: &[u8], plaintext: &[u8]) -> String {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let sealed = XChaCha20Poly1305::new(Key::from_slice(key))
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad })
        .expect("XChaCha20-Poly1305 only fails past 256 GiB of input");
    let mut out = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
    out.push(ENVELOPE_V1);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    B64.encode(out)
}

fn open(key: &[u8; KEY_LEN], aad: &[u8], data: &str) -> Result<Zeroizing<Vec<u8>>, OpenError> {
    let raw = B64.decode(data).map_err(|_| OpenError::Malformed)?;
    if raw.len() < 1 + NONCE_LEN + TAG_LEN || raw[0] != ENVELOPE_V1 {
        return Err(OpenError::Malformed);
    }
    let (nonce, sealed) = raw[1..].split_at(NONCE_LEN);
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), Payload { msg: sealed, aad })
        .map(Zeroizing::new)
        .map_err(|_| OpenError::Rejected)
}

/// What setting up a vault produces: the three values the server stores
/// (`kdf`, `wrapped_key`, `wrapped_key_recovery`), the vault key for this
/// device, and the recovery key to show the user once.
pub struct NewVault {
    pub kdf: KdfParams,
    pub wrapped_key: String,
    pub wrapped_key_recovery: String,
    pub key: VaultKey,
    pub recovery_key: RecoveryKey,
}

pub fn create(password: &str) -> Result<NewVault, VaultError> {
    create_with_cost(password, ARGON2_M_KIB, ARGON2_T, ARGON2_P)
}

fn create_with_cost(password: &str, m: u32, t: u32, p: u32) -> Result<NewVault, VaultError> {
    let key = VaultKey::generate();
    let (kdf, wrapped_key) = wrap_with_password(&key, password, m, t, p)?;
    let (recovery_key, wrapped_key_recovery) = new_recovery_key(&key);
    Ok(NewVault { kdf, wrapped_key, wrapped_key_recovery, key, recovery_key })
}

/// Open the vault with the master password
pub fn unlock(kdf: &KdfParams, wrapped_key: &str, password: &str) -> Result<VaultKey, VaultError> {
    let kek = kdf.derive(password)?;
    match open(&kek, AAD_WRAP_PASSWORD, wrapped_key) {
        Ok(bytes) => VaultKey::from_unwrapped(&bytes),
        Err(OpenError::Rejected) => Err(VaultError::WrongPassword),
        Err(OpenError::Malformed) => Err(VaultError::Corrupt),
    }
}

/// Open the vault with the recovery key. The user should set a new master
/// password ([`change_password`]) and get a new recovery key
/// ([`new_recovery_key`]) right after.
pub fn unlock_with_recovery_key(
    wrapped_key_recovery: &str,
    recovery_key: &RecoveryKey,
) -> Result<VaultKey, VaultError> {
    match open(&recovery_key.wrapping_key(), AAD_WRAP_RECOVERY, wrapped_key_recovery) {
        Ok(bytes) => VaultKey::from_unwrapped(&bytes),
        Err(OpenError::Rejected) => Err(VaultError::WrongRecoveryKey),
        Err(OpenError::Malformed) => Err(VaultError::Corrupt),
    }
}

/// New `kdf` and `wrapped_key` for the same vault key: a fresh salt and
/// today's Argon2 cost. Nothing synced needs re-encrypting.
pub fn change_password(key: &VaultKey, new_password: &str) -> Result<(KdfParams, String), VaultError> {
    wrap_with_password(key, new_password, ARGON2_M_KIB, ARGON2_T, ARGON2_P)
}

/// A new recovery key and `wrapped_key_recovery` for the same vault key, for
/// when the old one was used or may have been seen.
pub fn new_recovery_key(key: &VaultKey) -> (RecoveryKey, String) {
    let recovery_key = RecoveryKey::generate();
    let wrapped = seal(&recovery_key.wrapping_key(), AAD_WRAP_RECOVERY, key.0.as_ref());
    (recovery_key, wrapped)
}

fn wrap_with_password(
    key: &VaultKey,
    password: &str,
    m: u32,
    t: u32,
    p: u32,
) -> Result<(KdfParams, String), VaultError> {
    if password.nfc().count() < MIN_PASSWORD_CHARS {
        return Err(VaultError::PasswordTooShort);
    }
    let kdf = KdfParams::fresh(m, t, p);
    let kek = kdf.derive(password)?;
    let wrapped = seal(&kek, AAD_WRAP_PASSWORD, key.0.as_ref());
    Ok((kdf, wrapped))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWORD: &str = "correct horse battery";
    const ITEM: &str = "0b6f5f53-1111-4222-8333-444455556666";

    /// Argon2 at its minimum cost: the logic under test is the same
    fn quick_vault(password: &str) -> NewVault {
        create_with_cost(password, 64, 1, 1).unwrap()
    }

    fn opens(key: &VaultKey, data: &str) -> bool {
        key.decrypt_item(ITEM, data).is_ok()
    }

    #[test]
    fn a_new_vault_opens_with_its_password_and_with_its_recovery_key() {
        let vault = quick_vault(PASSWORD);
        let data = vault.key.encrypt_item(ITEM, b"{\"kind\":\"host\"}");

        let by_password = unlock(&vault.kdf, &vault.wrapped_key, PASSWORD).unwrap();
        assert_eq!(&*by_password.decrypt_item(ITEM, &data).unwrap(), b"{\"kind\":\"host\"}");

        let typed = RecoveryKey::parse(&vault.recovery_key.to_display_string()).unwrap();
        let by_recovery = unlock_with_recovery_key(&vault.wrapped_key_recovery, &typed).unwrap();
        assert!(opens(&by_recovery, &data));
    }

    #[test]
    fn a_wrong_password_or_recovery_key_is_refused() {
        let vault = quick_vault(PASSWORD);
        assert_eq!(
            unlock(&vault.kdf, &vault.wrapped_key, "correct horse battery!").unwrap_err(),
            VaultError::WrongPassword
        );
        let other = quick_vault(PASSWORD).recovery_key;
        assert_eq!(
            unlock_with_recovery_key(&vault.wrapped_key_recovery, &other).unwrap_err(),
            VaultError::WrongRecoveryKey
        );
        // The two wrapped copies are not interchangeable
        assert_eq!(
            unlock(&vault.kdf, &vault.wrapped_key_recovery, PASSWORD).unwrap_err(),
            VaultError::WrongPassword
        );
    }

    #[test]
    fn the_same_password_typed_as_other_code_points_still_opens() {
        let composed = "caf\u{e9} au lait!"; // é as one code point
        let decomposed = "cafe\u{301} au lait!"; // e + combining accent
        let vault = quick_vault(composed);
        assert!(unlock(&vault.kdf, &vault.wrapped_key, decomposed).is_ok());
    }

    #[test]
    fn a_short_password_is_refused_when_set() {
        let key = VaultKey::generate();
        assert_eq!(
            wrap_with_password(&key, "123456789", 64, 1, 1).unwrap_err(),
            VaultError::PasswordTooShort
        );
        assert!(wrap_with_password(&key, "1234567890", 64, 1, 1).is_ok());
        // Characters, not bytes: nine é are 18 bytes and still too short
        assert_eq!(
            wrap_with_password(&key, &"\u{e9}".repeat(9), 64, 1, 1).unwrap_err(),
            VaultError::PasswordTooShort
        );
        assert_eq!(change_password(&key, "short").unwrap_err(), VaultError::PasswordTooShort);
    }

    #[test]
    fn a_new_password_wraps_the_same_vault_key() {
        let vault = quick_vault(PASSWORD);
        let data = vault.key.encrypt_item(ITEM, b"secret");

        let (kdf, wrapped) = wrap_with_password(&vault.key, "a whole new password", 64, 1, 1).unwrap();
        assert_ne!(kdf.salt, vault.kdf.salt);
        let key = unlock(&kdf, &wrapped, "a whole new password").unwrap();
        assert!(opens(&key, &data));
        assert_eq!(unlock(&kdf, &wrapped, PASSWORD).unwrap_err(), VaultError::WrongPassword);

        // A new recovery key too; the old one no longer opens the new copy
        let (recovery, wrapped_recovery) = new_recovery_key(&key);
        assert!(opens(&unlock_with_recovery_key(&wrapped_recovery, &recovery).unwrap(), &data));
        assert_eq!(
            unlock_with_recovery_key(&wrapped_recovery, &vault.recovery_key).unwrap_err(),
            VaultError::WrongRecoveryKey
        );
    }

    #[test]
    fn an_item_only_opens_as_itself_and_untouched() {
        let key = VaultKey::generate();
        let data = key.encrypt_item(ITEM, b"payload");
        assert!(opens(&key, &data));
        assert!(key.decrypt_item(&ITEM.to_uppercase(), &data).is_ok());

        // Served as a different item
        assert_eq!(
            key.decrypt_item("9d6f5f53-1111-4222-8333-444455556666", &data).unwrap_err(),
            VaultError::Corrupt
        );
        // One bit flipped anywhere after the version byte
        let mut raw = B64.decode(&data).unwrap();
        for i in [1, NONCE_LEN + 1, raw.len() - 1] {
            raw[i] ^= 1;
            assert!(!opens(&key, &B64.encode(&raw)), "byte {i}");
            raw[i] ^= 1;
        }
        // Another vault's key
        assert!(!opens(&VaultKey::generate(), &data));
        // A fresh nonce every time
        assert_ne!(key.encrypt_item(ITEM, b"payload"), data);
    }

    #[test]
    fn recovery_keys_read_back_the_way_people_type_them() {
        let known = |bytes: [u8; RECOVERY_LEN]| RecoveryKey(Zeroizing::new(bytes));
        let mut counting = [0u8; RECOVERY_LEN];
        for (i, b) in counting.iter_mut().enumerate() {
            *b = i as u8;
        }
        // Expected values computed independently (Python, big-endian 5-bit groups)
        assert_eq!(
            known(counting).to_display_string().as_str(),
            "000G-40R4-0M30-E209-185G-R38E-1W81-24GK"
        );
        assert_eq!(
            known([0xFF; RECOVERY_LEN]).to_display_string().as_str(),
            "ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ-ZZZZ"
        );

        for typed in [
            "000G-40R4-0M30-E209-185G-R38E-1W81-24GK",
            "000g40r40m30e209185gr38e1w8124gk",
            "000G 40R4 0M30 E209 185G R38E 1W81 24GK\n",
            "OOOG-4OR4-OM3O-E2O9-I85G-R38E-lW8I-24GK", // O for 0, I and l for 1
        ] {
            let parsed = RecoveryKey::parse(typed).unwrap();
            assert_eq!(*parsed.0, counting, "{typed}");
        }

        for bad in [
            "",
            "000G-40R4-0M30-E209-185G-R38E-1W81-24G",   // one short
            "000G-40R4-0M30-E209-185G-R38E-1W81-24GKK", // one long
            "000G-40R4-0M30-E209-185G-R38E-1W81-24GU",  // U is not in the alphabet
        ] {
            assert_eq!(
                RecoveryKey::parse(bad).unwrap_err(),
                VaultError::MalformedRecoveryKey,
                "{bad}"
            );
        }
    }

    #[test]
    fn kdf_parameters_from_the_server_are_checked_before_use() {
        let vault = quick_vault(PASSWORD);
        let json = serde_json::to_value(&vault.kdf).unwrap();
        let mut fields: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        fields.sort();
        assert_eq!(fields, ["alg", "m", "p", "salt", "t"]);
        assert_eq!(json["alg"], "argon2id");

        let with = |change: fn(&mut KdfParams)| {
            let mut kdf = vault.kdf.clone();
            change(&mut kdf);
            unlock(&kdf, &vault.wrapped_key, PASSWORD).unwrap_err()
        };
        assert_eq!(with(|k| k.alg = "scrypt".into()), VaultError::UnsupportedKdf);
        // Refused up front, without allocating 4 GiB
        assert_eq!(with(|k| k.m = 4 * 1024 * 1024), VaultError::UnsupportedKdf);
        assert_eq!(with(|k| k.t = 1000), VaultError::UnsupportedKdf);
        assert_eq!(with(|k| k.p = 0), VaultError::UnsupportedKdf);
        assert_eq!(with(|k| k.salt = "not base64!".into()), VaultError::Corrupt);
        // A different salt is simply a different key
        assert_eq!(with(|k| k.salt = B64.encode([7u8; SALT_LEN])), VaultError::WrongPassword);
    }

    #[test]
    fn a_damaged_wrapped_key_is_reported_as_damaged() {
        let vault = quick_vault(PASSWORD);
        let mut raw = B64.decode(&vault.wrapped_key).unwrap();
        raw[0] = 2; // a future envelope version
        for wrapped in ["", "%%%", "AAAA", &B64.encode(&raw)] {
            assert_eq!(
                unlock(&vault.kdf, wrapped, PASSWORD).unwrap_err(),
                VaultError::Corrupt,
                "{wrapped}"
            );
        }
    }

    #[test]
    fn an_exported_key_imports_as_the_same_key() {
        let vault = quick_vault(PASSWORD);
        let sealed = vault.key.encrypt_item(ITEM, b"x");
        let text = vault.key.export();
        assert_eq!(text.len(), 44);
        let back = VaultKey::import(&text).unwrap();
        assert!(opens(&back, &sealed));
        assert_eq!(VaultKey::import("AAAA").unwrap_err(), VaultError::Corrupt);
        assert_eq!(VaultKey::import("not base64!").unwrap_err(), VaultError::Corrupt);
    }

    #[test]
    fn keys_never_show_up_in_debug_output() {
        let vault = quick_vault(PASSWORD);
        let shown = format!("{:?} {:?}", vault.key, vault.recovery_key);
        assert_eq!(shown, "VaultKey(…) RecoveryKey(…)");
    }

    #[test]
    fn the_real_cost_settings_work() {
        let vault = create(PASSWORD).unwrap();
        assert_eq!((vault.kdf.m, vault.kdf.t, vault.kdf.p), (65536, 3, 1));
        let key = unlock(&vault.kdf, &vault.wrapped_key, PASSWORD).unwrap();
        assert!(opens(&key, &vault.key.encrypt_item(ITEM, b"x")));
    }
}
