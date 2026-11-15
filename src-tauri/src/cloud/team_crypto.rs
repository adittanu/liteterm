//! Keys for shared team vaults.
//!
//! - Every account has an **account key**, an X25519 key pair. The private
//!   half is kept on the server sealed with the account's personal vault key,
//!   so each of its computers can open it once the vault is unlocked.
//! - Every team has a **team key** per epoch: 32 random bytes that encrypt the
//!   team's items. Each member gets a copy sealed to their account key
//!   (ephemeral X25519, HKDF-SHA256, XChaCha20-Poly1305). Removing a member
//!   starts a new epoch with a new key.
//! - A member's **safety code** is derived from their public key. Comparing it
//!   with the member, out of band, is what stops a server from slipping in a
//!   key of its own when someone is admitted.
//!
//! All associated data names the account or team and the item, so a server
//! can't pass a sealed value off as another one.

use std::fmt;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::vault::{self, VaultError, VaultKey};

const KEY_LEN: usize = 32;
const SEAL_HKDF_INFO: &[u8] = b"liteterm:team-key:v1";
/// Digits in a safety code: about 66 bits, so a matching fake key is out of
/// reach of a brute-force search
const SAFETY_DIGITS: usize = 20;

fn account_aad(user_id: &str) -> Vec<u8> {
    format!("liteterm:account-key:{}", user_id.to_ascii_lowercase()).into_bytes()
}

fn wrap_aad(team_id: &str, epoch: i64, user_id: &str) -> Vec<u8> {
    format!("liteterm:team-key:{}:{epoch}:{}", team_id.to_ascii_lowercase(), user_id.to_ascii_lowercase())
        .into_bytes()
}

fn item_aad(team_id: &str, sync_id: &str) -> Vec<u8> {
    format!("liteterm:team-item:{}:{}", team_id.to_ascii_lowercase(), sync_id.to_ascii_lowercase()).into_bytes()
}

fn decode_public(text: &str) -> Result<PublicKey, VaultError> {
    let bytes = B64.decode(text.trim()).map_err(|_| VaultError::Corrupt)?;
    let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| VaultError::Corrupt)?;
    Ok(PublicKey::from(bytes))
}

/// An account's key pair
pub struct AccountKey {
    secret: StaticSecret,
    public: PublicKey,
}

impl fmt::Debug for AccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountKey(…)")
    }
}

impl AccountKey {
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self { secret, public }
    }

    /// The public half, base64, as the server stores and hands it out
    pub fn public_key(&self) -> String {
        B64.encode(self.public.as_bytes())
    }

    /// The private half sealed with the personal vault key, for the server
    pub fn seal(&self, vault: &VaultKey, user_id: &str) -> String {
        vault.seal_other(&account_aad(user_id), self.secret.as_bytes())
    }

    /// Opens what [`AccountKey::seal`] made. Checks it against the public
    /// key the server lists, so a mismatched pair is caught here.
    pub fn open(vault: &VaultKey, user_id: &str, sealed: &str, public_key: &str) -> Result<Self, VaultError> {
        let bytes = vault.open_other(&account_aad(user_id), sealed)?;
        let key = Self::from_secret(&bytes)?;
        if key.public != decode_public(public_key)? {
            return Err(VaultError::Corrupt);
        }
        Ok(key)
    }

    fn from_secret(bytes: &[u8]) -> Result<Self, VaultError> {
        let bytes: Zeroizing<[u8; KEY_LEN]> =
            Zeroizing::new(bytes.try_into().map_err(|_| VaultError::Corrupt)?);
        let secret = StaticSecret::from(*bytes);
        let public = PublicKey::from(&secret);
        Ok(Self { secret, public })
    }

    /// The private key as base64, for this device's OS keyring
    pub fn export(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(KEY_LEN.div_ceil(3) * 4));
        B64.encode_string(self.secret.as_bytes(), &mut out);
        out
    }

    pub fn import(text: &str) -> Result<Self, VaultError> {
        let bytes = Zeroizing::new(B64.decode(text.trim()).map_err(|_| VaultError::Corrupt)?);
        Self::from_secret(&bytes)
    }

    /// This account's safety code
    pub fn safety_code(&self) -> String {
        code_of(&self.public)
    }
}

/// The safety code of a public key: 20 digits in groups of four
pub fn safety_code(public_key: &str) -> Result<String, VaultError> {
    Ok(code_of(&decode_public(public_key)?))
}

fn code_of(public: &PublicKey) -> String {
    let digest = Sha256::digest([b"liteterm:safety-code:v1:".as_slice(), public.as_bytes()].concat());
    let n = u128::from_be_bytes(digest[..16].try_into().expect("16 bytes"));
    let digits = format!("{:0width$}", n % 10u128.pow(SAFETY_DIGITS as u32), width = SAFETY_DIGITS);
    digits.as_bytes().chunks(4).map(|c| std::str::from_utf8(c).expect("digits")).collect::<Vec<_>>().join(" ")
}

/// The key a team's items are encrypted with, for one epoch
pub struct TeamKey(Zeroizing<[u8; KEY_LEN]>);

impl fmt::Debug for TeamKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TeamKey(…)")
    }
}

impl TeamKey {
    pub fn generate() -> Self {
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        OsRng.fill_bytes(key.as_mut());
        Self(key)
    }

    /// A copy for one member, sealed to their public key:
    /// `base64(ephemeral public key) "." envelope`
    pub fn seal_for(&self, team_id: &str, epoch: i64, user_id: &str, public_key: &str) -> Result<String, VaultError> {
        let recipient = decode_public(public_key)?;
        let ephemeral = StaticSecret::random_from_rng(OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral);
        let shared = ephemeral.diffie_hellman(&recipient);
        // A low-order point would make the secret predictable
        if !shared.was_contributory() {
            return Err(VaultError::Corrupt);
        }
        let kek = seal_kek(shared.as_bytes(), &ephemeral_public, &recipient);
        let envelope = vault::seal_with(&kek, &wrap_aad(team_id, epoch, user_id), self.0.as_ref());
        Ok(format!("{}.{envelope}", B64.encode(ephemeral_public.as_bytes())))
    }

    /// Opens this member's copy
    pub fn open(account: &AccountKey, team_id: &str, epoch: i64, user_id: &str, wrapped: &str) -> Result<Self, VaultError> {
        let (ephemeral, envelope) = wrapped.split_once('.').ok_or(VaultError::Corrupt)?;
        let ephemeral = decode_public(ephemeral)?;
        let shared = account.secret.diffie_hellman(&ephemeral);
        if !shared.was_contributory() {
            return Err(VaultError::Corrupt);
        }
        let kek = seal_kek(shared.as_bytes(), &ephemeral, &account.public);
        let bytes = vault::open_with(&kek, &wrap_aad(team_id, epoch, user_id), envelope)?;
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        if bytes.len() != KEY_LEN {
            return Err(VaultError::Corrupt);
        }
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    /// `data` for team_items, bound to the team and the item
    pub fn encrypt_item(&self, team_id: &str, sync_id: &str, plaintext: &[u8]) -> String {
        vault::seal_with(&self.0, &item_aad(team_id, sync_id), plaintext)
    }

    pub fn decrypt_item(&self, team_id: &str, sync_id: &str, data: &str) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        vault::open_with(&self.0, &item_aad(team_id, sync_id), data)
    }
}

/// HKDF over the shared secret, salted with both public keys so the key is
/// tied to this exchange
fn seal_kek(shared: &[u8; KEY_LEN], ephemeral: &PublicKey, recipient: &PublicKey) -> Zeroizing<[u8; KEY_LEN]> {
    let salt = [ephemeral.as_bytes().as_slice(), recipient.as_bytes().as_slice()].concat();
    let mut kek = Zeroizing::new([0u8; KEY_LEN]);
    Hkdf::<Sha256>::new(Some(&salt), shared)
        .expand(SEAL_HKDF_INFO, kek.as_mut())
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    kek
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM: &str = "6f1d4c2a-0000-4000-8000-000000000001";
    const ALICE: &str = "a11ce000-0000-4000-8000-000000000001";
    const BOB: &str = "b0b00000-0000-4000-8000-000000000002";

    fn vault_key() -> VaultKey {
        let mut raw = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut raw);
        VaultKey::import(&B64.encode(raw)).unwrap()
    }

    #[test]
    fn the_account_key_opens_only_with_its_vault_and_user() {
        let (vault, other_vault) = (vault_key(), vault_key());
        let key = AccountKey::generate();
        let sealed = key.seal(&vault, ALICE);
        let opened = AccountKey::open(&vault, ALICE, &sealed, &key.public_key()).unwrap();
        assert_eq!(opened.public_key(), key.public_key());
        assert!(AccountKey::open(&other_vault, ALICE, &sealed, &key.public_key()).is_err());
        assert!(AccountKey::open(&vault, BOB, &sealed, &key.public_key()).is_err(), "bound to its user");
        // A server pairing the sealed key with someone else's public key
        let stranger = AccountKey::generate();
        assert!(AccountKey::open(&vault, ALICE, &sealed, &stranger.public_key()).is_err());
        // The keyring copy round-trips
        assert_eq!(AccountKey::import(&key.export()).unwrap().public_key(), key.public_key());
    }

    #[test]
    fn a_team_key_sealed_to_a_member_opens_only_for_that_member() {
        let (alice, bob) = (AccountKey::generate(), AccountKey::generate());
        let team = TeamKey::generate();
        let for_bob = team.seal_for(TEAM, 1, BOB, &bob.public_key()).unwrap();

        let opened = TeamKey::open(&bob, TEAM, 1, BOB, &for_bob).unwrap();
        let item = team.encrypt_item(TEAM, "item-1", b"host");
        assert_eq!(&*opened.decrypt_item(TEAM, "item-1", &item).unwrap(), b"host");

        assert!(TeamKey::open(&alice, TEAM, 1, BOB, &for_bob).is_err(), "another member's key");
        assert!(TeamKey::open(&bob, TEAM, 2, BOB, &for_bob).is_err(), "another epoch");
        assert!(TeamKey::open(&bob, "6f1d4c2a-0000-4000-8000-000000000009", 1, BOB, &for_bob).is_err());
        assert!(TeamKey::open(&bob, TEAM, 1, ALICE, &for_bob).is_err(), "relabelled for someone else");
        // Two seals of the same key differ (fresh ephemeral key each time)
        assert_ne!(for_bob, team.seal_for(TEAM, 1, BOB, &bob.public_key()).unwrap());
    }

    #[test]
    fn an_item_cannot_be_moved_between_teams_or_items() {
        let team = TeamKey::generate();
        let data = team.encrypt_item(TEAM, "item-1", b"secret");
        assert!(team.decrypt_item("6f1d4c2a-0000-4000-8000-000000000009", "item-1", &data).is_err());
        assert!(team.decrypt_item(TEAM, "item-2", &data).is_err());
        assert!(TeamKey::generate().decrypt_item(TEAM, "item-1", &data).is_err());
        // Ids are compared case-insensitively, as Postgres prints uuids
        assert!(team.decrypt_item(&TEAM.to_uppercase(), "ITEM-1", &data).is_ok());
    }

    #[test]
    fn a_low_order_public_key_is_refused() {
        let team = TeamKey::generate();
        // The identity point: every shared secret with it is zero
        let zero = B64.encode([0u8; KEY_LEN]);
        assert!(team.seal_for(TEAM, 1, BOB, &zero).is_err());
        assert!(team.seal_for(TEAM, 1, BOB, "not base64!").is_err());
    }

    #[test]
    fn safety_codes_are_stable_and_tell_keys_apart() {
        let key = AccountKey::generate();
        let code = key.safety_code();
        assert_eq!(code, safety_code(&key.public_key()).unwrap());
        assert_eq!(code.len(), 24, "20 digits in 5 groups: {code}");
        assert!(code.split(' ').all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_digit())));
        assert_ne!(code, AccountKey::generate().safety_code());
        // Pinned, so the code never changes between versions
        let fixed = B64.encode([9u8; KEY_LEN]);
        assert_eq!(safety_code(&fixed).unwrap(), "0620 1527 2526 4411 3223");
    }
}
