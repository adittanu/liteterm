//! The account and the sync loop.
//!
//! What this device keeps:
//! - settings table: `cloud.email`, `cloud.userId`, `cloud.vaultId` (the vault
//!   it synced with), `cloud.cursor` (the last rev it pulled) and
//!   `cloud.lastSyncAt`
//! - OS keyring: the refresh token and the vault key, so a restart neither
//!   signs out nor locks. The access token lives in memory only.
//!
//! States, as the UI sees them: signed out → signed in without a vault
//! ("noVault": set a master password) or with a locked one ("locked": enter
//! it) → "ready", syncing in the background.
//!
//! A sync pulls first, then pushes. Pulling everything since the cursor and
//! applying it in one transaction keeps relations whole even when a target
//! arrives after the row that points at it.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use zeroize::Zeroizing;

use super::api::{Api, ApiError, MemberRow, PullItem, PushItem, RewriteItem, Session, TeamPullItem, VaultRow, WrappedFor};
use super::items::{self, Incoming, Keyring, Payload, Queued, Vault};
use super::team_crypto::{safety_code, AccountKey, TeamKey};
use super::vault::{self, RecoveryKey, VaultError, VaultKey};
use crate::store::db;
use crate::Db;

const KEYRING_REFRESH: &str = "cloud:refresh-token";
const KEYRING_VAULT_KEY: &str = "cloud:vault-key";
const KEYRING_ACCOUNT_KEY: &str = "cloud:account-key";

const S_EMAIL: &str = "cloud.email";
const S_USER: &str = "cloud.userId";
const S_VAULT: &str = "cloud.vaultId";
const S_CURSOR: &str = "cloud.cursor";
const S_LAST_SYNC: &str = "cloud.lastSyncAt";

const PULL_PAGE: usize = 500;
const PUSH_MAX_ITEMS: usize = 200;
const PUSH_MAX_BYTES: usize = 1 << 20;
/// How often the loop looks for local changes
const TICK: Duration = Duration::from_secs(5);
/// How often it pulls when nothing changed here
const PULL_EVERY_MS: i64 = 2 * 60 * 1000;
/// Retry waits after failures: 30 s, 1, 2, 4, 8 min, then every 10 min
const BACKOFF_MIN_MS: i64 = 30_000;
const BACKOFF_MAX_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// "signedOut" | "noVault" | "locked" | "ready"
    pub state: &'static str,
    pub email: Option<String>,
    pub syncing: bool,
    pub last_sync_at: Option<i64>,
    pub error: Option<String>,
    /// Local changes not on the server yet
    pub pending: i64,
}

#[derive(Debug)]
pub enum Error {
    Api(ApiError),
    Vault(VaultError),
    Db(rusqlite::Error),
    Msg(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api(e) => e.fmt(f),
            Self::Vault(e) => e.fmt(f),
            Self::Db(e) => write!(f, "Local database error: {e}"),
            Self::Msg(m) => f.write_str(m),
        }
    }
}

impl From<ApiError> for Error {
    fn from(e: ApiError) -> Self {
        Self::Api(e)
    }
}
impl From<VaultError> for Error {
    fn from(e: VaultError) -> Self {
        Self::Vault(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e)
    }
}

fn msg(text: &str) -> Error {
    Error::Msg(text.into())
}

#[derive(Default)]
struct Inner {
    refresh: Option<Zeroizing<String>>,
    /// Token and when it expires, ms since epoch
    access: Option<(Zeroizing<String>, i64)>,
    key: Option<Arc<VaultKey>>,
    /// This account's team key pair, once opened
    account: Option<Arc<AccountKey>>,
    /// Checked against the server since the last start or vault change
    account_checked: bool,
    /// Team keys by (team id, epoch)
    team_keys: HashMap<(String, i64), Arc<TeamKey>>,
    syncing: bool,
    error: Option<String>,
    failures: u32,
    last_attempt: i64,
}

pub struct Cloud {
    api: Api,
    inner: Mutex<Inner>,
    /// One sync or account change at a time
    run: tokio::sync::Mutex<()>,
    /// One token refresh at a time: a refresh token works once
    auth: tokio::sync::Mutex<()>,
    wake: tokio::sync::Notify,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

fn put(conn: &Connection, key: &str, value: Option<&str>) -> rusqlite::Result<()> {
    match value {
        Some(v) => conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, v],
        ),
        None => conn.execute("DELETE FROM settings WHERE key = ?1", [key]),
    }?;
    Ok(())
}

fn with_db<T>(app: &AppHandle, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, Error> {
    let db = app.state::<Db>();
    let conn = db.0.lock().map_err(|_| msg("database busy"))?;
    Ok(f(&conn)?)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, Error> + Send + 'static) -> Result<T, Error> {
    tokio::task::spawn_blocking(f).await.map_err(|_| msg("The vault operation was interrupted"))?
}

impl Cloud {
    /// Picks up the session and vault key this device kept
    pub fn new(conn: &Connection) -> Arc<Self> {
        let mut inner = Inner::default();
        if setting(conn, S_USER).ok().flatten().is_some() {
            inner.refresh = db::keyring_get(KEYRING_REFRESH).map(Zeroizing::new);
            inner.key = db::keyring_get(KEYRING_VAULT_KEY)
                .and_then(|text| VaultKey::import(&Zeroizing::new(text)).ok())
                .map(Arc::new);
            inner.account = db::keyring_get(KEYRING_ACCOUNT_KEY)
                .and_then(|text| AccountKey::import(&Zeroizing::new(text)).ok())
                .map(Arc::new);
        }
        Arc::new(Self {
            api: Api::new(),
            inner: Mutex::new(inner),
            run: tokio::sync::Mutex::new(()),
            auth: tokio::sync::Mutex::new(()),
            wake: tokio::sync::Notify::new(),
        })
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- status ----

    pub fn status(&self, app: &AppHandle) -> Result<Status, Error> {
        let (user, email, vault_id, last_sync, pending) = with_db(app, |c| {
            Ok((
                setting(c, S_USER)?,
                setting(c, S_EMAIL)?,
                setting(c, S_VAULT)?,
                setting(c, S_LAST_SYNC)?,
                c.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0))?,
            ))
        })?;
        let inner = self.inner();
        let state = if user.is_none() || inner.refresh.is_none() {
            "signedOut"
        } else if vault_id.is_none() {
            "noVault"
        } else if inner.key.is_none() {
            "locked"
        } else {
            "ready"
        };
        Ok(Status {
            state,
            email,
            syncing: inner.syncing,
            last_sync_at: last_sync.and_then(|s| s.parse().ok()),
            error: inner.error.clone(),
            pending,
        })
    }

    fn emit_status(&self, app: &AppHandle) {
        if let Ok(status) = self.status(app) {
            let _ = app.emit("cloud:status", status);
        }
    }

    // ---- session ----

    fn adopt_session(&self, s: &Session) {
        db::keyring_set(KEYRING_REFRESH, &s.refresh_token);
        let mut inner = self.inner();
        inner.refresh = Some(Zeroizing::new(s.refresh_token.clone()));
        inner.access = Some((Zeroizing::new(s.access_token.clone()), now_ms() + s.expires_in * 1000));
    }

    /// The server ended the session: back to signed out, keeping the email
    /// for the sign-in form and the vault key for when the same account
    /// signs in again
    fn end_session(&self) {
        db::keyring_delete(KEYRING_REFRESH);
        let mut inner = self.inner();
        inner.refresh = None;
        inner.access = None;
    }

    async fn token(&self, force_refresh: bool) -> Result<Zeroizing<String>, Error> {
        let _auth = self.auth.lock().await;
        let refresh = {
            let inner = self.inner();
            if !force_refresh {
                if let Some((token, expires_at)) = &inner.access {
                    if *expires_at - 60_000 > now_ms() {
                        return Ok(token.clone());
                    }
                }
            }
            inner.refresh.clone().ok_or(Error::Api(ApiError::SessionEnded))?
        };
        match self.api.refresh(&refresh).await {
            Ok(session) => {
                self.adopt_session(&session);
                Ok(Zeroizing::new(session.access_token))
            }
            Err(ApiError::SessionEnded) => {
                self.end_session();
                Err(ApiError::SessionEnded.into())
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Call the server with the access token, refreshing it once if the
    /// server says it has expired
    async fn authed<T, F, Fut>(&self, f: F) -> Result<T, Error>
    where
        F: Fn(Zeroizing<String>) -> Fut,
        Fut: Future<Output = Result<T, ApiError>>,
    {
        let token = self.token(false).await?;
        match f(token).await {
            Err(ApiError::Unauthorized) => {
                let token = self.token(true).await?;
                Ok(f(token).await?)
            }
            result => Ok(result?),
        }
    }

    // ---- the vault on this device ----

    fn forget_key(&self) {
        db::keyring_delete(KEYRING_VAULT_KEY);
        let mut inner = self.inner();
        inner.key = None;
        // Whether the account key still opens is checked again under the next key
        inner.account_checked = false;
    }

    /// Another account, or none: this computer keeps nothing of the old
    /// account's teams, not even their saved passwords
    fn forget_teams(&self, app: &AppHandle) -> Result<(), Error> {
        db::keyring_delete(KEYRING_ACCOUNT_KEY);
        {
            let mut inner = self.inner();
            inner.account = None;
            inner.account_checked = false;
            inner.team_keys.clear();
        }
        let applied = with_db(app, |c| {
            let tx = c.unchecked_transaction()?;
            let teams: Vec<String> =
                tx.prepare("SELECT id FROM teams")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            let mut all = items::Applied::default();
            for team in teams {
                let applied = items::forget_team(&tx, &team)?;
                all.kinds.extend(applied.kinds);
                all.secrets.extend(applied.secrets);
            }
            tx.commit()?;
            Ok(all)
        })?;
        applied.write_secrets(&Keyring);
        emit_changed(app, &applied);
        Ok(())
    }

    /// Match what the server has: a vault this device does not know (or none
    /// at all) means its key and cursor no longer apply
    fn adopt_vault_row(&self, app: &AppHandle, vault: Option<&VaultRow>) -> Result<(), Error> {
        let stored = with_db(app, |c| setting(c, S_VAULT))?;
        let vault_id = vault.map(|v| v.vault_id.as_str());
        if stored.as_deref() != vault_id {
            self.forget_key();
            with_db(app, |c| {
                put(c, S_VAULT, vault_id)?;
                put(c, S_CURSOR, None)
            })?;
        }
        Ok(())
    }

    /// Start syncing with a vault: keep its key, pull it all, push all of ours
    fn link(&self, app: &AppHandle, key: VaultKey, vault_id: &str) -> Result<(), Error> {
        db::keyring_set(KEYRING_VAULT_KEY, &key.export());
        with_db(app, |c| {
            let tx = c.unchecked_transaction()?;
            put(&tx, S_VAULT, Some(vault_id))?;
            put(&tx, S_CURSOR, Some("0"))?;
            items::queue_all(&tx, &Vault::Personal)?;
            tx.commit()
        })?;
        let mut inner = self.inner();
        inner.key = Some(Arc::new(key));
        inner.account_checked = false;
        inner.error = None;
        inner.failures = 0;
        drop(inner);
        self.wake.notify_one();
        Ok(())
    }

    fn user_id(&self, app: &AppHandle) -> Result<String, Error> {
        with_db(app, |c| setting(c, S_USER))?.ok_or(Error::Api(ApiError::SessionEnded))
    }

    async fn vault_row(&self, app: &AppHandle) -> Result<VaultRow, Error> {
        let vault = self.authed(|t| async move { self.api.get_vault(&t).await }).await?;
        match vault {
            Some(v) => Ok(v),
            None => {
                self.adopt_vault_row(app, None)?;
                Err(msg("This account has no cloud vault yet."))
            }
        }
    }

    // ---- account actions ----

    pub async fn send_code(&self, email: &str) -> Result<(), Error> {
        let email = email.trim();
        if !email.contains('@') {
            return Err(msg("That email address doesn't look right."));
        }
        Ok(self.api.send_code(email).await?)
    }

    pub async fn verify_code(&self, app: &AppHandle, email: &str, code: &str) -> Result<Status, Error> {
        let _run = self.run.lock().await;
        let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let session = self.api.verify_code(email.trim(), &code).await?;
        let user_id = session.user.id.clone();
        let email = session.user.email.clone().unwrap_or_else(|| email.trim().to_string());

        let previous = with_db(app, |c| setting(c, S_USER))?;
        if previous.as_deref() != Some(user_id.as_str()) {
            // Another account: nothing this device knew about a vault applies
            self.forget_key();
            self.forget_teams(app)?;
            with_db(app, |c| {
                put(c, S_VAULT, None)?;
                put(c, S_CURSOR, None)?;
                put(c, S_LAST_SYNC, None)
            })?;
        }
        self.adopt_session(&session);
        with_db(app, |c| {
            put(c, S_USER, Some(&user_id))?;
            put(c, S_EMAIL, Some(&email))
        })?;
        {
            let mut inner = self.inner();
            inner.error = None;
            inner.failures = 0;
        }

        let vault = self.authed(|t| async move { self.api.get_vault(&t).await }).await?;
        self.adopt_vault_row(app, vault.as_ref())?;
        if self.inner().key.is_some() {
            self.wake.notify_one();
        }
        self.status(app)
    }

    /// Set up the vault with its master password. Returns the recovery key,
    /// to show once.
    pub async fn create_vault(&self, app: &AppHandle, password: Zeroizing<String>) -> Result<Zeroizing<String>, Error> {
        let _run = self.run.lock().await;
        let new = blocking(move || Ok(vault::create(&password)?)).await?;
        let (kdf, wrapped, wrapped_recovery) = (&new.kdf, &new.wrapped_key, &new.wrapped_key_recovery);
        let created = self
            .authed(|t| async move { self.api.create_vault(&t, kdf, wrapped, wrapped_recovery).await })
            .await;
        let row = match created {
            Ok(row) => row,
            Err(Error::Api(ApiError::Refused { code, .. })) if code == "23505" => {
                // Set up from another device in the meantime
                let vault = self.authed(|t| async move { self.api.get_vault(&t).await }).await?;
                self.adopt_vault_row(app, vault.as_ref())?;
                return Err(msg(
                    "This account already has a cloud vault. Unlock it with its master password.",
                ));
            }
            Err(e) => return Err(e),
        };
        let recovery = new.recovery_key.to_display_string();
        self.link(app, new.key, &row.vault_id)?;
        Ok(recovery)
    }

    pub async fn unlock(&self, app: &AppHandle, password: Zeroizing<String>) -> Result<Status, Error> {
        let _run = self.run.lock().await;
        let v = self.vault_row(app).await?;
        let (kdf, wrapped) = (v.kdf.clone(), v.wrapped_key.clone());
        let key = blocking(move || Ok(vault::unlock(&kdf, &wrapped, &password)?)).await?;
        self.link(app, key, &v.vault_id)?;
        self.status(app)
    }

    /// Open the vault with the recovery key and set a new master password.
    /// The used recovery key is replaced; returns the new one, to show once.
    pub async fn recover(
        &self,
        app: &AppHandle,
        recovery_key: Zeroizing<String>,
        new_password: Zeroizing<String>,
    ) -> Result<Zeroizing<String>, Error> {
        let _run = self.run.lock().await;
        let v = self.vault_row(app).await?;
        let recovery_key = RecoveryKey::parse(&recovery_key)?;
        let key = vault::unlock_with_recovery_key(&v.wrapped_key_recovery, &recovery_key)?;
        let (key, kdf, wrapped, new_recovery, wrapped_recovery) = blocking(move || {
            let (kdf, wrapped) = vault::change_password(&key, &new_password)?;
            let (new_recovery, wrapped_recovery) = vault::new_recovery_key(&key);
            Ok((key, kdf, wrapped, new_recovery, wrapped_recovery))
        })
        .await?;
        let user_id = self.user_id(app)?;
        let fields = json!({ "kdf": kdf, "wrapped_key": wrapped, "wrapped_key_recovery": wrapped_recovery });
        let (user_id, fields) = (&user_id, &fields);
        self.authed(|t| async move { self.api.update_vault(&t, user_id, fields.clone()).await })
            .await?;
        self.link(app, key, &v.vault_id)?;
        Ok(new_recovery.to_display_string())
    }

    pub async fn change_password(
        &self,
        app: &AppHandle,
        current: Zeroizing<String>,
        new_password: Zeroizing<String>,
    ) -> Result<(), Error> {
        let _run = self.run.lock().await;
        let v = self.vault_row(app).await?;
        let (kdf, wrapped) = (v.kdf.clone(), v.wrapped_key.clone());
        let (kdf, wrapped) = blocking(move || {
            let key = vault::unlock(&kdf, &wrapped, &current)?;
            Ok(vault::change_password(&key, &new_password)?)
        })
        .await?;
        let user_id = self.user_id(app)?;
        let fields = json!({ "kdf": kdf, "wrapped_key": wrapped });
        let (user_id, fields) = (&user_id, &fields);
        self.authed(|t| async move { self.api.update_vault(&t, user_id, fields.clone()).await })
            .await?;
        Ok(())
    }

    /// Delete the cloud vault and everything in it. Local data stays.
    pub async fn reset_vault(&self, app: &AppHandle) -> Result<Status, Error> {
        let _run = self.run.lock().await;
        let user_id = self.user_id(app)?;
        let user_id = &user_id;
        self.authed(|t| async move { self.api.delete_vault(&t, user_id).await }).await?;
        self.forget_key();
        with_db(app, |c| {
            put(c, S_VAULT, None)?;
            put(c, S_CURSOR, None)?;
            put(c, S_LAST_SYNC, None)
        })?;
        self.inner().error = None;
        self.status(app)
    }

    /// Sign this device out. Its hosts, notes and the rest stay on it.
    pub async fn sign_out(&self, app: &AppHandle) -> Result<Status, Error> {
        let _run = self.run.lock().await;
        // Best effort: an offline device still signs out locally
        if let Ok(token) = self.token(false).await {
            let _ = self.api.sign_out(&token).await;
        }
        self.end_session();
        self.forget_key();
        self.forget_teams(app)?;
        with_db(app, |c| {
            put(c, S_USER, None)?;
            put(c, S_VAULT, None)?;
            put(c, S_CURSOR, None)?;
            put(c, S_LAST_SYNC, None)
        })?;
        let mut inner = self.inner();
        inner.error = None;
        inner.failures = 0;
        drop(inner);
        self.status(app)
    }

    // ---- team management ----

    /// The personal vault key and this account's id, for team actions
    fn ready(&self, app: &AppHandle) -> Result<(Arc<VaultKey>, String), Error> {
        if self.status(app)?.state != "ready" {
            return Err(msg("Teams need sync with a master password. Set it up under Account & sync first."));
        }
        let key = self.inner().key.clone().ok_or_else(|| msg("The vault is locked."))?;
        Ok((key, self.user_id(app)?))
    }

    /// The teams this computer knows, invites for this account, and its
    /// safety code
    pub async fn teams(&self, app: &AppHandle) -> Result<TeamsView, Error> {
        let (vault_key, uid) = self.ready(app)?;
        let account = self.account_key(&vault_key, &uid).await?;
        let email = with_db(app, |c| setting(c, S_EMAIL))?.unwrap_or_default().to_lowercase();
        let invites = self.authed(|t| async move { self.api.invites(&t).await }).await?;
        let teams = with_db(app, |c| {
            c.prepare("SELECT id, name, role, epoch FROM teams ORDER BY name COLLATE NOCASE")?
                .query_map([], |r| Ok(TeamSummary { id: r.get(0)?, name: r.get(1)?, role: r.get(2)?, epoch: r.get(3)? }))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })?;
        let invites = invites
            .into_iter()
            .filter(|i| i.email == email)
            .map(|i| InviteForMe {
                accepted: i.accepted_by.is_some(),
                id: i.id,
                team_name: i.team_name,
                invited_by: i.invited_by_email,
                role: i.role,
                expires_at: i.expires_at,
            })
            .collect();
        Ok(TeamsView { safety_code: account.safety_code(), teams, invites })
    }

    /// Members and pending invites of one team
    pub async fn team_detail(&self, app: &AppHandle, team_id: &str) -> Result<TeamDetail, Error> {
        let (_, uid) = self.ready(app)?;
        let team_id = uuid(team_id)?;
        let tid = team_id.as_str();
        let members = self.authed(|t| async move { self.api.team_members(&t, tid).await }).await?;
        let invites = self.authed(|t| async move { self.api.invites(&t).await }).await?;
        Ok(TeamDetail {
            members: members
                .into_iter()
                .map(|m| MemberView { is_me: m.user_id == uid, user_id: m.user_id, email: m.email, role: m.role })
                .collect(),
            pending: invites
                .into_iter()
                .filter(|i| i.team_id == team_id)
                .map(|i| PendingView {
                    safety_code: i.accepted_public_key.as_deref().and_then(|k| safety_code(k).ok()),
                    accepted: i.accepted_by.is_some(),
                    id: i.id,
                    email: i.email,
                    role: i.role,
                    expires_at: i.expires_at,
                })
                .collect(),
        })
    }

    pub async fn create_team(&self, app: &AppHandle, name: &str) -> Result<TeamSummary, Error> {
        let _run = self.run.lock().await;
        let (vault_key, uid) = self.ready(app)?;
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 80 {
            return Err(msg("Give the team a name of up to 80 characters."));
        }
        let account = self.account_key(&vault_key, &uid).await?;
        let team_id = new_uuid();
        let key = TeamKey::generate();
        let wrapped = key.seal_for(&team_id, 1, &uid, &account.public_key())?;
        let (tid, wrapped) = (team_id.as_str(), wrapped.as_str());
        self.authed(|t| async move { self.api.create_team(&t, tid, name, wrapped).await }).await?;
        self.inner().team_keys.insert((team_id.clone(), 1), Arc::new(key));
        with_db(app, |c| {
            c.execute(
                "INSERT INTO teams (id, name, role, epoch) VALUES (?1, ?2, 'owner', 1) ON CONFLICT(id) DO NOTHING",
                params![team_id, name],
            )?;
            pin_peer(c, &team_id, &uid, &account.public_key()).map(drop)
        })?;
        let _ = app.emit("cloud:teams", ());
        Ok(TeamSummary { id: team_id, name: name.to_string(), role: "owner".into(), epoch: 1 })
    }

    pub async fn rename_team(&self, app: &AppHandle, team_id: &str, name: &str) -> Result<(), Error> {
        self.ready(app)?;
        let team_id = uuid(team_id)?;
        let (tid, name) = (team_id.as_str(), name.trim());
        if name.is_empty() || name.chars().count() > 80 {
            return Err(msg("Give the team a name of up to 80 characters."));
        }
        self.authed(|t| async move { self.api.rename_team(&t, tid, name).await }).await?;
        with_db(app, |c| c.execute("UPDATE teams SET name = ?1 WHERE id = ?2", params![name, tid]).map(drop))?;
        let _ = app.emit("cloud:teams", ());
        Ok(())
    }

    pub async fn invite(&self, app: &AppHandle, team_id: &str, email: &str, role: &str) -> Result<(), Error> {
        self.ready(app)?;
        let team_id = uuid(team_id)?;
        let email = email.trim();
        if !email.contains('@') || email.len() > 254 {
            return Err(msg("That email address doesn't look right."));
        }
        if !matches!(role, "admin" | "editor" | "viewer") {
            return Err(msg("Unknown role"));
        }
        let tid = team_id.as_str();
        self.authed(|t| async move { self.api.invite_member(&t, tid, email, role).await }).await?;
        Ok(())
    }

    /// Say yes to an invite. A team owner or admin then admits this account,
    /// after comparing its safety code.
    pub async fn accept_invite(&self, app: &AppHandle, invite_id: &str) -> Result<(), Error> {
        let _run = self.run.lock().await;
        let (vault_key, uid) = self.ready(app)?;
        let invite_id = uuid(invite_id)?;
        // The invite carries the account key, so there must be one
        self.account_key(&vault_key, &uid).await?;
        let iid = invite_id.as_str();
        self.authed(|t| async move { self.api.accept_invite(&t, iid).await }).await?;
        Ok(())
    }

    /// Decline an invite, or withdraw one
    pub async fn cancel_invite(&self, app: &AppHandle, invite_id: &str) -> Result<(), Error> {
        self.ready(app)?;
        let invite_id = uuid(invite_id)?;
        let iid = invite_id.as_str();
        self.authed(|t| async move { self.api.cancel_invite(&t, iid).await }).await?;
        Ok(())
    }

    /// Admit someone who accepted. `shown_code` is the safety code the owner
    /// compared with the person: the team key is sealed only to the key with
    /// that code, whatever the server hands out meanwhile.
    pub async fn admit(&self, app: &AppHandle, invite_id: &str, shown_code: &str) -> Result<(), Error> {
        let _run = self.run.lock().await;
        self.ready(app)?;
        let invite_id = uuid(invite_id)?;
        let invites = self.authed(|t| async move { self.api.invites(&t).await }).await?;
        let invite = invites
            .into_iter()
            .find(|i| i.id == invite_id)
            .ok_or_else(|| msg("That invite is gone. It was withdrawn, or has expired."))?;
        let (Some(member), Some(public_key)) = (invite.accepted_by.clone(), invite.accepted_public_key.clone()) else {
            return Err(msg("They haven't accepted the invite yet."));
        };
        let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
        if digits(&safety_code(&public_key)?) != digits(shown_code) {
            return Err(msg(
                "Their key no longer has the safety code you compared. Don't admit them: ask them to accept the invite again.",
            ));
        }
        let epoch = with_db(app, |c| {
            c.query_row("SELECT epoch FROM teams WHERE id = ?1", [&invite.team_id], |r| r.get::<_, i64>(0))
                .optional()
        })?
        .ok_or_else(|| msg("This computer hasn't synced that team yet."))?;
        let key = self
            .team_key(&invite.team_id, epoch)
            .ok_or_else(|| msg("This computer doesn't have the team's current key yet. Sync, then try again."))?;
        let wrapped = key.seal_for(&invite.team_id, epoch, &member, &public_key)?;
        let (iid, wrapped_ref) = (invite_id.as_str(), wrapped.as_str());
        self.authed(|t| async move { self.api.admit_member(&t, iid, epoch, wrapped_ref).await }).await?;
        // Checked by code just now: the strongest pin there is
        with_db(app, |c| {
            c.execute(
                "INSERT INTO team_peers (team_id, user_id, public_key) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(team_id, user_id) DO UPDATE SET public_key = excluded.public_key",
                params![invite.team_id, member, public_key],
            )
            .map(drop)
        })?;
        Ok(())
    }

    pub async fn set_role(&self, app: &AppHandle, team_id: &str, user_id: &str, role: &str) -> Result<(), Error> {
        self.ready(app)?;
        let (team_id, user_id) = (uuid(team_id)?, uuid(user_id)?);
        if !matches!(role, "owner" | "admin" | "editor" | "viewer") {
            return Err(msg("Unknown role"));
        }
        let (tid, uid) = (team_id.as_str(), user_id.as_str());
        self.authed(|t| async move { self.api.set_member_role(&t, tid, uid, role).await }).await?;
        Ok(())
    }

    /// Remove a member and change the team key. Returns what they could read
    /// and should be changed on the servers: the team's saved passwords.
    pub async fn remove_member(&self, app: &AppHandle, team_id: &str, user_id: &str) -> Result<Vec<String>, Error> {
        let _run = self.run.lock().await;
        let (_, me) = self.ready(app)?;
        let (team_id, user_id) = (uuid(team_id)?, uuid(user_id)?);
        let (epoch, key, wrapped) = self.next_epoch(app, &me, &team_id, &user_id).await?;
        let (tid, uid, wrapped) = (team_id.as_str(), user_id.as_str(), &wrapped);
        self.authed(|t| async move { self.api.remove_member(&t, tid, uid, epoch, wrapped).await }).await?;
        self.inner().team_keys.insert((team_id.clone(), epoch), Arc::new(key));
        let exposed = with_db(app, |c| {
            c.execute("UPDATE teams SET epoch = ?1 WHERE id = ?2", params![epoch, tid])?;
            c.execute("DELETE FROM team_peers WHERE team_id = ?1 AND user_id = ?2", params![tid, uid])?;
            exposed_secrets(c, tid)
        })?;
        // The next sync moves the items to the new key
        self.wake.notify_one();
        Ok(exposed)
    }

    /// Leave a team. The key changes for the others, and the team's data
    /// leaves this computer.
    pub async fn leave_team(&self, app: &AppHandle, team_id: &str) -> Result<(), Error> {
        let _run = self.run.lock().await;
        let (_, me) = self.ready(app)?;
        let team_id = uuid(team_id)?;
        let (epoch, _, wrapped) = self.next_epoch(app, &me, &team_id, &me).await?;
        let (tid, wrapped) = (team_id.as_str(), &wrapped);
        self.authed(|t| async move { self.api.leave_team(&t, tid, epoch, wrapped).await }).await?;
        self.drop_team_here(app, &team_id)
    }

    pub async fn delete_team(&self, app: &AppHandle, team_id: &str) -> Result<(), Error> {
        let _run = self.run.lock().await;
        self.ready(app)?;
        let team_id = uuid(team_id)?;
        let tid = team_id.as_str();
        self.authed(|t| async move { self.api.delete_team(&t, tid).await }).await?;
        self.drop_team_here(app, &team_id)
    }

    fn drop_team_here(&self, app: &AppHandle, team_id: &str) -> Result<(), Error> {
        let applied = with_db(app, |c| {
            let tx = c.unchecked_transaction()?;
            let applied = items::forget_team(&tx, team_id)?;
            tx.commit()?;
            Ok(applied)
        })?;
        applied.write_secrets(&Keyring);
        self.inner().team_keys.retain(|(team, _), _| team != team_id);
        emit_changed(app, &applied);
        let _ = app.emit("cloud:teams", ());
        Ok(())
    }

    /// A new team key for everyone in the team except `leaving`, sealed to
    /// each of them. Every member's key must match the one this computer
    /// pinned for them; one seen for the first time is pinned now.
    async fn next_epoch(
        &self,
        app: &AppHandle,
        me: &str,
        team_id: &str,
        leaving: &str,
    ) -> Result<(i64, TeamKey, Vec<WrappedFor>), Error> {
        let memberships = self.authed(|t| async move { self.api.memberships(&t, me).await }).await?;
        let current = memberships
            .iter()
            .find(|m| m.team_id == team_id)
            .ok_or_else(|| msg("You're not in that team any more."))?
            .teams
            .epoch;
        let members = self.authed(|t| async move { self.api.team_members(&t, team_id).await }).await?;
        let staying: Vec<MemberRow> = members.into_iter().filter(|m| m.user_id != leaving).collect();
        let ids: Vec<String> = staying.iter().map(|m| m.user_id.clone()).collect();
        let ids_ref = &ids;
        let keys = self.authed(|t| async move { self.api.public_keys(&t, ids_ref).await }).await?;
        let epoch = current + 1;
        let key = TeamKey::generate();
        let mut wrapped = Vec::with_capacity(staying.len());
        for m in &staying {
            let public_key = keys
                .iter()
                .find(|k| k.user_id == m.user_id)
                .map(|k| k.public_key.clone())
                .ok_or_else(|| Error::Msg(format!("The server didn't give {}'s key.", m.email)))?;
            let pinned = with_db(app, |c| pin_peer(c, team_id, &m.user_id, &public_key))?;
            if !pinned {
                return Err(Error::Msg(format!(
                    "{}'s key is not the one this computer saw before, so the new team key wasn't sealed to it. \
                     Remove them and invite them again, checking their safety code.",
                    m.email
                )));
            }
            wrapped.push(WrappedFor { user_id: m.user_id.clone(), wrapped_key: key.seal_for(team_id, epoch, &m.user_id, &public_key)? });
        }
        Ok((epoch, key, wrapped))
    }

    // ---- syncing ----

    /// Sync now, if this device is ready to
    pub async fn sync(&self, app: &AppHandle) -> Result<Status, Error> {
        let _run = self.run.lock().await;
        if self.status(app)?.state != "ready" {
            return self.status(app);
        }
        self.inner().syncing = true;
        self.emit_status(app);

        let result = self.sync_once(app).await;
        {
            let mut inner = self.inner();
            inner.syncing = false;
            inner.last_attempt = now_ms();
            match &result {
                Ok(()) => {
                    inner.error = None;
                    inner.failures = 0;
                }
                Err(e) => {
                    inner.error = Some(e.to_string());
                    inner.failures += 1;
                }
            }
        }
        if let Err(e) = &result {
            crate::applog::write_throttled("cloud-sync", &format!("cloud sync failed: {e}"));
        }
        self.emit_status(app);
        result.and_then(|()| self.status(app))
    }

    async fn sync_once(&self, app: &AppHandle) -> Result<(), Error> {
        let key = self.inner().key.clone().ok_or_else(|| msg("The vault is locked."))?;

        // Reset from another device since the last sync?
        let vault = self.authed(|t| async move { self.api.get_vault(&t).await }).await?;
        let stored = with_db(app, |c| setting(c, S_VAULT))?;
        let v = match vault {
            Some(v) if stored.as_deref() == Some(v.vault_id.as_str()) => v,
            other => {
                self.adopt_vault_row(app, other.as_ref())?;
                return Err(msg(if other.is_some() {
                    "The cloud vault was reset from another computer. Unlock it with its new master password."
                } else {
                    "The cloud vault was deleted from another computer. Set a master password to start a new one."
                }));
            }
        };

        // ---- pull ----
        let cursor: i64 = with_db(app, |c| setting(c, S_CURSOR))?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if v.rev > cursor {
            let mut after = cursor;
            let mut pulled = Vec::new();
            while after < v.rev {
                let page = self.authed(|t| async move { self.api.pull(&t, after, PULL_PAGE).await }).await?;
                let Some(last) = page.last() else { break };
                after = last.rev;
                pulled.extend(page);
            }
            let (incoming, unreadable) = decrypt_all(&key, pulled);
            if unreadable > 0 {
                crate::applog::write(&format!("cloud sync: {unreadable} item(s) could not be read and were skipped"));
            }
            let applied = with_db(app, |c| {
                let tx = c.unchecked_transaction()?;
                let applied = items::apply(&tx, &Vault::Personal, incoming)?;
                put(&tx, S_CURSOR, Some(&after.to_string()))?;
                tx.commit()?;
                Ok(applied)
            })?;
            applied.write_secrets(&Keyring);
            if applied.skipped > 0 {
                crate::applog::write(&format!("cloud sync: {} item(s) had nowhere to go", applied.skipped));
            }
            emit_changed(app, &applied);
        }

        // ---- push ----
        let queued = with_db(app, |c| items::queued(c, &Keyring, &Vault::Personal))?;
        let encrypted: Vec<PushItem> = queued.iter().map(|q| encrypt(&key, q)).collect();
        for (start, end) in batches(&encrypted) {
            let batch = &encrypted[start..end];
            self.authed(|t| async move { self.api.push(&t, batch).await }).await?;
            with_db(app, |c| items::clear_pushed(c, &queued[start..end]))?;
        }

        // ---- teams ----
        self.sync_teams(app, &key).await?;

        with_db(app, |c| put(c, S_LAST_SYNC, Some(&now_ms().to_string())))?;
        Ok(())
    }

    // ---- team vaults ----

    /// This account's team key pair: from memory, or from the server opened
    /// with the vault key, or made now if the account has none. After a vault
    /// reset the server's copy is sealed under the old vault key; a computer
    /// that still holds the key seals it again under the new one.
    async fn account_key(&self, vault_key: &VaultKey, user_id: &str) -> Result<Arc<AccountKey>, Error> {
        let cached = {
            let inner = self.inner();
            if inner.account_checked {
                if let Some(k) = &inner.account {
                    return Ok(k.clone());
                }
            }
            inner.account.clone()
        };
        let row = self.authed(|t| async move { self.api.account_key(&t, user_id).await }).await?;
        let key = match row {
            Some(row) => match AccountKey::open(vault_key, user_id, &row.private_key_sealed, &row.public_key) {
                Ok(key) => Arc::new(key),
                Err(_) => match cached.filter(|c| c.public_key() == row.public_key) {
                    Some(key) => {
                        let sealed = key.seal(vault_key, user_id);
                        let sealed = &sealed;
                        self.authed(|t| async move { self.api.reseal_account_key(&t, sealed).await }).await?;
                        key
                    }
                    None => {
                        return Err(msg(
                            "Your team key was sealed with a cloud vault that has been reset since, and no \
                             computer of yours still holds it. Team vaults can't be opened on this account.",
                        ))
                    }
                },
            },
            None => {
                let fresh = cached.unwrap_or_else(|| Arc::new(AccountKey::generate()));
                let (public, sealed) = (fresh.public_key(), fresh.seal(vault_key, user_id));
                let (public, sealed) = (&public, &sealed);
                self.authed(|t| async move { self.api.put_account_key(&t, public, sealed).await }).await?;
                // Another computer of this account may have set one first: use that
                let row = self
                    .authed(|t| async move { self.api.account_key(&t, user_id).await })
                    .await?
                    .ok_or_else(|| msg("The server did not keep the team key"))?;
                Arc::new(AccountKey::open(vault_key, user_id, &row.private_key_sealed, &row.public_key)?)
            }
        };
        db::keyring_set(KEYRING_ACCOUNT_KEY, &key.export());
        let mut inner = self.inner();
        inner.account = Some(key.clone());
        inner.account_checked = true;
        Ok(key)
    }

    /// Bring every team vault this account is in up to date, and drop the
    /// ones it left
    async fn sync_teams(&self, app: &AppHandle, vault_key: &VaultKey) -> Result<(), Error> {
        let user_id = self.user_id(app)?;
        let uid = user_id.as_str();
        let memberships = self.authed(|t| async move { self.api.memberships(&t, uid).await }).await?;
        let local: Vec<String> = with_db(app, |c| {
            c.prepare("SELECT id FROM teams")?.query_map([], |r| r.get(0))?.collect()
        })?;

        // Teams this account is no longer in
        for gone in local.iter().filter(|id| !memberships.iter().any(|m| &m.team_id == *id)) {
            let applied = with_db(app, |c| {
                let tx = c.unchecked_transaction()?;
                let applied = items::forget_team(&tx, gone)?;
                tx.commit()?;
                Ok(applied)
            })?;
            applied.write_secrets(&Keyring);
            self.inner().team_keys.retain(|(team, _), _| team != gone);
            emit_changed(app, &applied);
            let _ = app.emit("cloud:teams", ());
        }
        if memberships.is_empty() {
            return Ok(());
        }

        let account = self.account_key(vault_key, uid).await?;
        let wrapped = self.authed(|t| async move { self.api.team_keys(&t, uid).await }).await?;
        {
            let mut inner = self.inner();
            for w in wrapped {
                let id = (w.team_id.clone(), w.epoch);
                if inner.team_keys.contains_key(&id) {
                    continue;
                }
                match TeamKey::open(&account, &w.team_id, w.epoch, uid, &w.wrapped_key) {
                    Ok(k) => {
                        inner.team_keys.insert(id, Arc::new(k));
                    }
                    Err(_) => crate::applog::write(&format!(
                        "team {}: the key of epoch {} could not be opened",
                        w.team_id, w.epoch
                    )),
                }
            }
        }

        let mut first_error = None;
        let before = local_teams(app)?;
        for m in &memberships {
            with_db(app, |c| {
                c.execute(
                    "INSERT INTO teams (id, name, role, epoch) VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name, role = excluded.role, epoch = excluded.epoch",
                    params![m.team_id, m.teams.name, m.role, m.teams.epoch],
                )
                .map(drop)
            })?;
            // One team's trouble doesn't hold up the others
            if let Err(e) = self.sync_team(app, &m.team_id, &m.role, m.teams.rev, m.teams.epoch).await {
                crate::applog::write(&format!("team {}: {e}", m.team_id));
                first_error.get_or_insert(e);
            }
        }
        if local_teams(app)? != before {
            let _ = app.emit("cloud:teams", ());
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn team_key(&self, team_id: &str, epoch: i64) -> Option<Arc<TeamKey>> {
        self.inner().team_keys.get(&(team_id.to_string(), epoch)).cloned()
    }

    async fn sync_team(&self, app: &AppHandle, team_id: &str, role: &str, rev: i64, epoch: i64) -> Result<(), Error> {
        let vault = Vault::Team(team_id.to_string());

        // ---- pull ----
        let cursor: i64 = with_db(app, |c| {
            c.query_row("SELECT cursor FROM teams WHERE id = ?1", [team_id], |r| r.get(0))
        })?;
        if rev > cursor {
            let mut after = cursor;
            let mut pulled = Vec::new();
            while after < rev {
                let page = self
                    .authed(|t| async move { self.api.pull_team(&t, team_id, after, PULL_PAGE).await })
                    .await?;
                let Some(last) = page.last() else { break };
                after = last.rev;
                pulled.extend(page);
            }
            let (incoming, unreadable) = self.decrypt_team(team_id, pulled);
            if unreadable > 0 {
                crate::applog::write(&format!("team {team_id}: {unreadable} item(s) could not be read and were skipped"));
            }
            let applied = with_db(app, |c| {
                let tx = c.unchecked_transaction()?;
                let applied = items::apply(&tx, &vault, incoming)?;
                tx.execute("UPDATE teams SET cursor = ?1 WHERE id = ?2", params![after, team_id])?;
                tx.commit()?;
                Ok(applied)
            })?;
            applied.write_secrets(&Keyring);
            emit_changed(app, &applied);
        }

        // ---- push ----
        if role == "viewer" {
            // The server would refuse; the app doesn't offer edits to viewers
            let dropped = with_db(app, |c| c.execute("DELETE FROM sync_outbox WHERE team_id = ?1", [team_id]))?;
            if dropped > 0 {
                crate::applog::write(&format!("team {team_id}: {dropped} change(s) by a viewer were not sent"));
            }
            return Ok(());
        }
        let key = self
            .team_key(team_id, epoch)
            .ok_or_else(|| msg("This computer doesn't have the team's current key yet. Try again in a moment."))?;
        let queued = with_db(app, |c| items::queued(c, &Keyring, &vault))?;
        let encrypted: Vec<PushItem> = queued.iter().map(|q| encrypt_team(&key, team_id, q)).collect();
        for (start, end) in batches(&encrypted) {
            let batch = &encrypted[start..end];
            self.authed(|t| async move { self.api.push_team(&t, team_id, epoch, batch).await }).await?;
            with_db(app, |c| items::clear_pushed(c, &queued[start..end]))?;
        }

        // ---- finish a key change someone started ----
        if role == "owner" || role == "admin" {
            self.rewrite_stale(team_id, epoch, &key).await?;
        }
        Ok(())
    }

    /// Re-encrypt items still on an older team key with the current one, a
    /// page at a time. Whoever administers the team and syncs next carries
    /// on where an interrupted change stopped.
    async fn rewrite_stale(&self, team_id: &str, epoch: i64, key: &TeamKey) -> Result<(), Error> {
        const PAGE: usize = 200;
        let mut left = i64::MAX;
        for _ in 0..50 {
            let stale = self
                .authed(|t| async move { self.api.stale_team_items(&t, team_id, epoch, PAGE).await })
                .await?;
            if stale.is_empty() {
                return Ok(());
            }
            let rewritten: Vec<RewriteItem> = stale
                .iter()
                .filter(|s| !s.deleted)
                .filter_map(|s| {
                    let old = self.team_key(team_id, s.epoch)?;
                    let plain = old.decrypt_item(team_id, &s.id, s.data.as_deref()?).ok()?;
                    Some(RewriteItem { id: s.id.clone(), data: key.encrypt_item(team_id, &s.id, &plain), modified_at: s.modified_at })
                })
                .collect();
            let rewritten = &rewritten;
            let now_left = self
                .authed(|t| async move { self.api.rewrite_team(&t, team_id, epoch, rewritten).await })
                .await?;
            // Nothing moved: items this computer can't read; leave them for another
            if now_left == 0 || now_left >= left {
                return Ok(());
            }
            left = now_left;
        }
        Ok(())
    }

    /// Like [`decrypt_all`], with the key of each item's epoch
    fn decrypt_team(&self, team_id: &str, pulled: Vec<TeamPullItem>) -> (Vec<Incoming>, usize) {
        let mut out = Vec::with_capacity(pulled.len());
        let mut unreadable = 0;
        for p in pulled {
            if p.deleted {
                out.push(Incoming { sync_id: p.id, payload: None, modified_at: p.modified_at });
                continue;
            }
            let payload = self.team_key(team_id, p.epoch).and_then(|key| {
                let plain = key.decrypt_item(team_id, &p.id, p.data.as_deref()?).ok()?;
                serde_json::from_slice::<Payload>(&plain).ok()
            });
            match payload {
                Some(payload) => out.push(Incoming { sync_id: p.id, payload: Some(payload), modified_at: p.modified_at }),
                None => unreadable += 1,
            }
        }
        (out, unreadable)
    }

    /// Run the background loop: sync on start, a few seconds after local
    /// changes, and every couple of minutes to pick up other devices' edits
    pub fn start(self: Arc<Self>, app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            let mut first = true;
            loop {
                self.tick(&app, first).await;
                first = false;
                tokio::select! {
                    _ = tokio::time::sleep(TICK) => {}
                    _ = self.wake.notified() => {}
                }
            }
        });
    }

    async fn tick(&self, app: &AppHandle, first: bool) {
        let Ok(status) = self.status(app) else { return };
        if status.state != "ready" || status.syncing {
            return;
        }
        let now = now_ms();
        let (failures, last_attempt) = {
            let inner = self.inner();
            (inner.failures, inner.last_attempt)
        };
        if failures > 0 {
            let wait = (BACKOFF_MIN_MS << (failures - 1).min(5)).min(BACKOFF_MAX_MS);
            if now - last_attempt < wait {
                return;
            }
        }
        let due = first
            || failures > 0
            || status.pending > 0
            || now - status.last_sync_at.unwrap_or(0) >= PULL_EVERY_MS;
        if due {
            let _ = self.sync(app).await;
        }
    }
}

fn encrypt(key: &VaultKey, q: &Queued) -> PushItem {
    let data = q.payload.as_ref().map(|payload| {
        let json = Zeroizing::new(serde_json::to_vec(payload).expect("items serialize"));
        key.encrypt_item(&q.sync_id, &json)
    });
    PushItem { id: q.sync_id.clone(), deleted: data.is_none(), data, modified_at: q.modified_at }
}

fn encrypt_team(key: &TeamKey, team_id: &str, q: &Queued) -> PushItem {
    let data = q.payload.as_ref().map(|payload| {
        let json = Zeroizing::new(serde_json::to_vec(payload).expect("items serialize"));
        key.encrypt_item(team_id, &q.sync_id, &json)
    });
    PushItem { id: q.sync_id.clone(), deleted: data.is_none(), data, modified_at: q.modified_at }
}

/// (start, end) ranges of at most PUSH_MAX_ITEMS items and about
/// PUSH_MAX_BYTES each
fn batches(items: &[PushItem]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < items.len() {
        let mut end = start;
        let mut bytes = 0;
        while end < items.len() && end - start < PUSH_MAX_ITEMS {
            let size = items[end].data.as_ref().map_or(0, String::len) + 128;
            if end > start && bytes + size > PUSH_MAX_BYTES {
                break;
            }
            bytes += size;
            end += 1;
        }
        out.push((start, end));
        start = end;
    }
    out
}

/// Tell the UI which kinds of rows changed, so it reloads them
fn emit_changed(app: &AppHandle, applied: &items::Applied) {
    if !applied.kinds.is_empty() {
        let mut kinds: Vec<&str> = applied.kinds.iter().copied().collect();
        kinds.sort_unstable();
        let _ = app.emit("cloud:changed", kinds);
    }
}

/// The teams table as this computer has it, to tell when it changed
fn local_teams(app: &AppHandle) -> Result<Vec<TeamSummary>, Error> {
    with_db(app, |c| {
        c.prepare("SELECT id, name, role, epoch FROM teams ORDER BY id")?
            .query_map([], |r| Ok(TeamSummary { id: r.get(0)?, name: r.get(1)?, role: r.get(2)?, epoch: r.get(3)? }))?
            .collect()
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TeamSummary {
    pub id: String,
    pub name: String,
    pub role: String,
    pub epoch: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteForMe {
    pub id: String,
    pub team_name: String,
    pub invited_by: String,
    pub role: String,
    pub expires_at: String,
    /// Accepted, waiting for an owner or admin to admit
    pub accepted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamsView {
    /// This account's, to read out to whoever admits it
    pub safety_code: String,
    pub teams: Vec<TeamSummary>,
    pub invites: Vec<InviteForMe>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub user_id: String,
    pub email: String,
    pub role: String,
    pub is_me: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingView {
    pub id: String,
    pub email: String,
    pub role: String,
    pub expires_at: String,
    pub accepted: bool,
    /// The code of the key they accepted with, to compare before admitting
    pub safety_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamDetail {
    pub members: Vec<MemberView>,
    pub pending: Vec<PendingView>,
}

/// Ids go into request paths: only a uuid's characters may
fn uuid(text: &str) -> Result<String, Error> {
    let t = text.trim().to_ascii_lowercase();
    let ok = t.len() == 36
        && t.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() });
    if ok { Ok(t) } else { Err(msg("That isn't a valid id.")) }
}

/// A random (version 4) uuid
fn new_uuid() -> String {
    use rand_core::{OsRng, RngCore};
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Remember a teammate's public key, or check it against the one already
/// remembered. False when it differs.
fn pin_peer(conn: &Connection, team_id: &str, user_id: &str, public_key: &str) -> rusqlite::Result<bool> {
    let known: Option<String> = conn
        .query_row(
            "SELECT public_key FROM team_peers WHERE team_id = ?1 AND user_id = ?2",
            params![team_id, user_id],
            |r| r.get(0),
        )
        .optional()?;
    match known {
        Some(k) => Ok(k == public_key),
        None => {
            conn.execute(
                "INSERT INTO team_peers (team_id, user_id, public_key) VALUES (?1, ?2, ?3)",
                params![team_id, user_id, public_key],
            )?;
            Ok(true)
        }
    }
}

/// The team's saved passwords, which every member could read: the hosts and
/// credentials to change them on after someone leaves
fn exposed_secrets(conn: &Connection, team_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut out: Vec<String> = conn
        .prepare("SELECT label FROM hosts WHERE team_id = ?1 AND password_saved = 1 ORDER BY label COLLATE NOCASE")?
        .query_map([team_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    out.extend(
        conn.prepare(
            "SELECT name FROM credentials WHERE team_id = ?1 AND password_saved = 1 ORDER BY name COLLATE NOCASE",
        )?
        .query_map([team_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_checked_before_they_reach_a_url() {
        let id = new_uuid();
        assert_eq!(uuid(&id).unwrap(), id);
        assert_eq!(&id[14..15], "4", "version 4: {id}");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"), "variant: {id}");
        assert_eq!(uuid(" 7EA70000-0000-4000-8000-000000000001 ").unwrap(), "7ea70000-0000-4000-8000-000000000001");
        assert!(uuid("7ea70000-0000-4000-8000-00000000000&").is_err());
        assert!(uuid("x&team_id=eq.other").is_err());
        assert!(uuid("").is_err());
    }

    #[test]
    fn a_teammates_key_is_pinned_the_first_time() {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        assert!(pin_peer(&conn, "t", "u", "KEY-1").unwrap());
        assert!(pin_peer(&conn, "t", "u", "KEY-1").unwrap());
        assert!(!pin_peer(&conn, "t", "u", "KEY-2").unwrap(), "a swapped key is caught");
        assert!(pin_peer(&conn, "other-team", "u", "KEY-2").unwrap(), "pins are per team");
    }
}

/// Items this version cannot read (damaged, or a kind from a newer version)
/// are counted and left out
fn decrypt_all(key: &VaultKey, pulled: Vec<PullItem>) -> (Vec<Incoming>, usize) {
    let mut out = Vec::with_capacity(pulled.len());
    let mut unreadable = 0;
    for p in pulled {
        if p.deleted {
            out.push(Incoming { sync_id: p.id, payload: None, modified_at: p.modified_at });
            continue;
        }
        let payload = p
            .data
            .as_deref()
            .and_then(|data| key.decrypt_item(&p.id, data).ok())
            .and_then(|plain| serde_json::from_slice::<Payload>(&plain).ok());
        match payload {
            Some(payload) => {
                out.push(Incoming { sync_id: p.id, payload: Some(payload), modified_at: p.modified_at })
            }
            None => unreadable += 1,
        }
    }
    (out, unreadable)
}
