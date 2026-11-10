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

use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use zeroize::Zeroizing;

use super::api::{Api, ApiError, PullItem, PushItem, Session, VaultRow};
use super::items::{self, Incoming, Keyring, Payload, Queued};
use super::vault::{self, RecoveryKey, VaultError, VaultKey};
use crate::store::db;
use crate::Db;

const KEYRING_REFRESH: &str = "cloud:refresh-token";
const KEYRING_VAULT_KEY: &str = "cloud:vault-key";

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
        self.inner().key = None;
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
            items::queue_all(&tx)?;
            tx.commit()
        })?;
        let mut inner = self.inner();
        inner.key = Some(Arc::new(key));
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
                let applied = items::apply(&tx, incoming)?;
                put(&tx, S_CURSOR, Some(&after.to_string()))?;
                tx.commit()?;
                Ok(applied)
            })?;
            applied.write_secrets(&Keyring);
            if applied.skipped > 0 {
                crate::applog::write(&format!("cloud sync: {} item(s) had nowhere to go", applied.skipped));
            }
            if !applied.kinds.is_empty() {
                let mut kinds: Vec<&str> = applied.kinds.iter().copied().collect();
                kinds.sort_unstable();
                let _ = app.emit("cloud:changed", kinds);
            }
        }

        // ---- push ----
        let queued = with_db(app, |c| items::queued(c, &Keyring))?;
        let encrypted: Vec<PushItem> = queued.iter().map(|q| encrypt(&key, q)).collect();
        let mut start = 0;
        while start < encrypted.len() {
            let mut end = start;
            let mut bytes = 0;
            while end < encrypted.len() && end - start < PUSH_MAX_ITEMS {
                let size = encrypted[end].data.as_ref().map_or(0, String::len) + 128;
                if end > start && bytes + size > PUSH_MAX_BYTES {
                    break;
                }
                bytes += size;
                end += 1;
            }
            let batch = &encrypted[start..end];
            self.authed(|t| async move { self.api.push(&t, batch).await }).await?;
            with_db(app, |c| items::clear_pushed(c, &queued[start..end]))?;
            start = end;
        }

        with_db(app, |c| put(c, S_LAST_SYNC, Some(&now_ms().to_string())))?;
        Ok(())
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
