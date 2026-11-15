//! The Supabase calls sync makes: email-code sign-in, the vault row, items.
//!
//! Nothing secret leaves through here except the account's session tokens:
//! item data is already encrypted, and the vault row holds only the wrapped
//! vault key and the Argon2 parameters.

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::vault::KdfParams;

pub const PROJECT_URL: &str = "https://jqulqbwotdzxieafosxw.supabase.co";
/// Publishable by design: it only names the project. Row-level security
/// decides what a signed-in user can reach (see supabase/migrations).
pub const PUBLISHABLE_KEY: &str = "sb_publishable_HusMJVmEyLbKyoqDQDiojQ_z1_Q8X0i";

#[derive(Debug)]
pub enum ApiError {
    /// No connection, DNS, timeout
    Offline,
    /// The access token expired or was revoked: refresh it and try again
    Unauthorized,
    /// The refresh token is no longer valid: sign in again
    SessionEnded,
    /// The server refused, with its reason
    Refused { status: u16, code: String, message: String },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Offline => f.write_str("Can't reach the sync server. Check the connection."),
            Self::Unauthorized | Self::SessionEnded => {
                f.write_str("Your session has ended. Sign in again.")
            }
            Self::Refused { message, .. } => f.write_str(message),
        }
    }
}

#[derive(Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds
    pub expires_in: i64,
    pub user: User,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session").field("user", &self.user).finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VaultRow {
    pub vault_id: String,
    pub kdf: KdfParams,
    pub wrapped_key: String,
    pub wrapped_key_recovery: String,
    pub rev: i64,
}

#[derive(Debug, Serialize)]
pub struct PushItem {
    pub id: String,
    pub data: Option<String>,
    pub deleted: bool,
    pub modified_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct PullItem {
    pub id: String,
    pub data: Option<String>,
    pub deleted: bool,
    pub modified_at: i64,
    pub rev: i64,
}

const VAULT_COLS: &str = "vault_id,kdf,wrapped_key,wrapped_key_recovery,rev";

#[derive(Debug, Deserialize)]
pub struct AccountKeyRow {
    pub public_key: String,
    pub private_key_sealed: String,
}

#[derive(Debug, Deserialize)]
pub struct PublicKeyRow {
    pub user_id: String,
    pub public_key: String,
}

#[derive(Debug, Deserialize)]
pub struct TeamInfo {
    pub name: String,
    pub rev: i64,
    pub epoch: i64,
}

#[derive(Debug, Deserialize)]
pub struct Membership {
    pub team_id: String,
    pub role: String,
    pub teams: TeamInfo,
}

#[derive(Debug, Deserialize)]
pub struct TeamKeyRow {
    pub team_id: String,
    pub epoch: i64,
    pub wrapped_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberRow {
    pub user_id: String,
    pub role: String,
    pub email: String,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InviteRow {
    pub id: String,
    pub team_id: String,
    pub email: String,
    pub role: String,
    pub team_name: String,
    pub invited_by_email: String,
    pub expires_at: String,
    #[serde(default)]
    pub accepted_by: Option<String>,
    #[serde(default)]
    pub accepted_public_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TeamPullItem {
    pub id: String,
    pub data: Option<String>,
    pub deleted: bool,
    pub modified_at: i64,
    pub rev: i64,
    pub epoch: i64,
}

#[derive(Debug, Serialize)]
pub struct RewriteItem {
    pub id: String,
    pub data: String,
    pub modified_at: i64,
}

#[derive(Debug, Serialize)]
pub struct WrappedFor {
    pub user_id: String,
    pub wrapped_key: String,
}

pub struct Api {
    http: reqwest::Client,
}

impl Api {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("LiteTerm/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("the TLS backend is compiled in");
        Self { http }
    }

    // ---- sign-in ----

    /// Email a sign-in code. Signs the address up on first use.
    pub async fn send_code(&self, email: &str) -> Result<(), ApiError> {
        self.call(reqwest::Method::POST, "/auth/v1/otp", None, Some(json!({ "email": email, "create_user": true })))
            .await
            .map(drop)
    }

    pub async fn verify_code(&self, email: &str, code: &str) -> Result<Session, ApiError> {
        let body = json!({ "type": "email", "email": email, "token": code });
        let value = self.call(reqwest::Method::POST, "/auth/v1/verify", None, Some(body)).await?;
        parse(value)
    }

    /// A new session for the refresh token. Each refresh token works once:
    /// keep the one this returns.
    pub async fn refresh(&self, refresh_token: &str) -> Result<Session, ApiError> {
        let body = json!({ "refresh_token": refresh_token });
        match self
            .call(reqwest::Method::POST, "/auth/v1/token?grant_type=refresh_token", None, Some(body))
            .await
        {
            Ok(value) => parse(value),
            Err(ApiError::Refused { status: 400 | 401 | 403, .. }) | Err(ApiError::Unauthorized) => {
                Err(ApiError::SessionEnded)
            }
            Err(e) => Err(e),
        }
    }

    /// End this device's session on the server (others stay signed in)
    pub async fn sign_out(&self, access: &str) -> Result<(), ApiError> {
        self.call(reqwest::Method::POST, "/auth/v1/logout?scope=local", Some(access), None)
            .await
            .map(drop)
    }

    // ---- the vault row ----

    pub async fn get_vault(&self, access: &str) -> Result<Option<VaultRow>, ApiError> {
        let value = self
            .call(reqwest::Method::GET, &format!("/rest/v1/vaults?select={VAULT_COLS}"), Some(access), None)
            .await?;
        let mut rows: Vec<VaultRow> = parse(value)?;
        Ok(rows.pop())
    }

    pub async fn create_vault(
        &self,
        access: &str,
        kdf: &KdfParams,
        wrapped_key: &str,
        wrapped_key_recovery: &str,
    ) -> Result<VaultRow, ApiError> {
        let body = json!({
            "kdf": kdf,
            "wrapped_key": wrapped_key,
            "wrapped_key_recovery": wrapped_key_recovery,
        });
        let value = self
            .call_with(
                reqwest::Method::POST,
                &format!("/rest/v1/vaults?select={VAULT_COLS}"),
                Some(access),
                Some(body),
                Some("return=representation"),
            )
            .await?;
        let mut rows: Vec<VaultRow> = parse(value)?;
        rows.pop().ok_or_else(|| refused("The server did not return the new vault"))
    }

    /// Change some of kdf / wrapped_key / wrapped_key_recovery
    pub async fn update_vault(&self, access: &str, user_id: &str, fields: Value) -> Result<(), ApiError> {
        let value = self
            .call_with(
                reqwest::Method::PATCH,
                &format!("/rest/v1/vaults?user_id=eq.{user_id}&select=vault_id"),
                Some(access),
                Some(fields),
                Some("return=representation"),
            )
            .await?;
        match value {
            Value::Array(rows) if !rows.is_empty() => Ok(()),
            _ => Err(refused("The cloud vault no longer exists")),
        }
    }

    /// Delete the vault and every item in it
    pub async fn delete_vault(&self, access: &str, user_id: &str) -> Result<(), ApiError> {
        self.call(reqwest::Method::DELETE, &format!("/rest/v1/vaults?user_id=eq.{user_id}"), Some(access), None)
            .await
            .map(drop)
    }

    // ---- items ----

    /// Returns the vault's rev after the push
    pub async fn push(&self, access: &str, items: &[PushItem]) -> Result<i64, ApiError> {
        let value = self
            .call(reqwest::Method::POST, "/rest/v1/rpc/push_items", Some(access), Some(json!({ "items": items })))
            .await?;
        parse(value)
    }

    /// Items changed after `after_rev`, oldest first
    pub async fn pull(&self, access: &str, after_rev: i64, limit: usize) -> Result<Vec<PullItem>, ApiError> {
        let path = format!(
            "/rest/v1/vault_items?select=id,data,deleted,modified_at,rev&rev=gt.{after_rev}&order=rev.asc&limit={limit}"
        );
        let value = self.call(reqwest::Method::GET, &path, Some(access), None).await?;
        parse(value)
    }

    // ---- team vaults ----

    /// Call one of the team functions (`/rest/v1/rpc/<name>`)
    async fn rpc(&self, access: &str, name: &str, args: Value) -> Result<Value, ApiError> {
        self.call(reqwest::Method::POST, &format!("/rest/v1/rpc/{name}"), Some(access), Some(args)).await
    }

    async fn get(&self, access: &str, path: &str) -> Result<Value, ApiError> {
        self.call(reqwest::Method::GET, path, Some(access), None).await
    }

    pub async fn account_key(&self, access: &str, user_id: &str) -> Result<Option<AccountKeyRow>, ApiError> {
        let value = self
            .get(access, &format!("/rest/v1/account_keys?user_id=eq.{user_id}&select=public_key,private_key_sealed"))
            .await?;
        let mut rows: Vec<AccountKeyRow> = parse(value)?;
        Ok(rows.pop())
    }

    /// Public keys of teammates (RLS shows only those)
    pub async fn public_keys(&self, access: &str, user_ids: &[String]) -> Result<Vec<PublicKeyRow>, ApiError> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = user_ids.join(",");
        parse(self.get(access, &format!("/rest/v1/account_keys?user_id=in.({list})&select=user_id,public_key")).await?)
    }

    pub async fn put_account_key(&self, access: &str, public_key: &str, private_key_sealed: &str) -> Result<(), ApiError> {
        self.rpc(access, "put_account_key", json!({ "public_key": public_key, "private_key_sealed": private_key_sealed }))
            .await
            .map(drop)
    }

    /// Re-seal the private half under a new personal vault key (after a vault
    /// reset). The public half can't change.
    pub async fn reseal_account_key(&self, access: &str, private_key_sealed: &str) -> Result<(), ApiError> {
        self.rpc(access, "reseal_account_key", json!({ "private_key_sealed": private_key_sealed })).await.map(drop)
    }

    /// The teams this account is in
    pub async fn memberships(&self, access: &str, user_id: &str) -> Result<Vec<Membership>, ApiError> {
        parse(
            self.get(access, &format!("/rest/v1/team_members?user_id=eq.{user_id}&select=team_id,role,teams(name,rev,epoch)"))
                .await?,
        )
    }

    /// This account's sealed copies of team keys, every team and epoch
    pub async fn team_keys(&self, access: &str, user_id: &str) -> Result<Vec<TeamKeyRow>, ApiError> {
        parse(self.get(access, &format!("/rest/v1/team_keys?user_id=eq.{user_id}&select=team_id,epoch,wrapped_key")).await?)
    }

    pub async fn team_members(&self, access: &str, team_id: &str) -> Result<Vec<MemberRow>, ApiError> {
        parse(
            self.get(access, &format!("/rest/v1/team_members?team_id=eq.{team_id}&select=user_id,role,email,added_at&order=added_at"))
                .await?,
        )
    }

    /// Invites addressed to this account, and those of teams it administers
    pub async fn invites(&self, access: &str) -> Result<Vec<InviteRow>, ApiError> {
        parse(
            self.get(
                access,
                "/rest/v1/team_invites?select=id,team_id,email,role,team_name,invited_by_email,expires_at,accepted_by,accepted_public_key&order=created_at",
            )
            .await?,
        )
    }

    /// `team_id` is chosen by the app: the creator's key copy is sealed to it
    pub async fn create_team(&self, access: &str, team_id: &str, name: &str, wrapped_key: &str) -> Result<String, ApiError> {
        parse(self.rpc(access, "create_team", json!({ "team_id": team_id, "name": name, "wrapped_key": wrapped_key })).await?)
    }

    pub async fn rename_team(&self, access: &str, team_id: &str, name: &str) -> Result<(), ApiError> {
        self.rpc(access, "rename_team", json!({ "team_id": team_id, "name": name })).await.map(drop)
    }

    pub async fn delete_team(&self, access: &str, team_id: &str) -> Result<(), ApiError> {
        self.rpc(access, "delete_team", json!({ "team_id": team_id })).await.map(drop)
    }

    pub async fn invite_member(&self, access: &str, team_id: &str, email: &str, role: &str) -> Result<String, ApiError> {
        parse(self.rpc(access, "invite_member", json!({ "team_id": team_id, "email": email, "role": role })).await?)
    }

    pub async fn accept_invite(&self, access: &str, invite_id: &str) -> Result<(), ApiError> {
        self.rpc(access, "accept_invite", json!({ "invite_id": invite_id })).await.map(drop)
    }

    pub async fn cancel_invite(&self, access: &str, invite_id: &str) -> Result<(), ApiError> {
        self.rpc(access, "cancel_invite", json!({ "invite_id": invite_id })).await.map(drop)
    }

    pub async fn admit_member(&self, access: &str, invite_id: &str, epoch: i64, wrapped_key: &str) -> Result<(), ApiError> {
        self.rpc(access, "admit_member", json!({ "invite_id": invite_id, "epoch": epoch, "wrapped_key": wrapped_key }))
            .await
            .map(drop)
    }

    pub async fn set_member_role(&self, access: &str, team_id: &str, user_id: &str, role: &str) -> Result<(), ApiError> {
        self.rpc(access, "set_member_role", json!({ "team_id": team_id, "user_id": user_id, "role": role }))
            .await
            .map(drop)
    }

    /// `wrapped`: the new epoch's key sealed to each remaining member
    pub async fn remove_member(
        &self,
        access: &str,
        team_id: &str,
        user_id: &str,
        new_epoch: i64,
        wrapped: &[WrappedFor],
    ) -> Result<(), ApiError> {
        let args = json!({ "team_id": team_id, "user_id": user_id, "new_epoch": new_epoch, "wrapped": wrapped });
        self.rpc(access, "remove_member", args).await.map(drop)
    }

    pub async fn leave_team(&self, access: &str, team_id: &str, new_epoch: i64, wrapped: &[WrappedFor]) -> Result<(), ApiError> {
        self.rpc(access, "leave_team", json!({ "team_id": team_id, "new_epoch": new_epoch, "wrapped": wrapped }))
            .await
            .map(drop)
    }

    /// Returns the team's rev after the push
    pub async fn push_team(&self, access: &str, team_id: &str, epoch: i64, items: &[PushItem]) -> Result<i64, ApiError> {
        parse(self.rpc(access, "push_team_items", json!({ "team_id": team_id, "epoch": epoch, "items": items })).await?)
    }

    pub async fn pull_team(&self, access: &str, team_id: &str, after_rev: i64, limit: usize) -> Result<Vec<TeamPullItem>, ApiError> {
        let path = format!(
            "/rest/v1/team_items?team_id=eq.{team_id}&select=id,data,deleted,modified_at,rev,epoch\
             &rev=gt.{after_rev}&order=rev.asc&limit={limit}"
        );
        parse(self.get(access, &path).await?)
    }

    /// Items still encrypted with a key older than `epoch`
    pub async fn stale_team_items(&self, access: &str, team_id: &str, epoch: i64, limit: usize) -> Result<Vec<TeamPullItem>, ApiError> {
        let path = format!(
            "/rest/v1/team_items?team_id=eq.{team_id}&epoch=lt.{epoch}&select=id,data,deleted,modified_at,rev,epoch\
             &order=rev.asc&limit={limit}"
        );
        parse(self.get(access, &path).await?)
    }

    /// Returns how many items still use an older key
    pub async fn rewrite_team(&self, access: &str, team_id: &str, epoch: i64, items: &[RewriteItem]) -> Result<i64, ApiError> {
        parse(self.rpc(access, "rewrite_team_items", json!({ "team_id": team_id, "epoch": epoch, "items": items })).await?)
    }

    // ---- plumbing ----

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        access: Option<&str>,
        body: Option<Value>,
    ) -> Result<Value, ApiError> {
        self.call_with(method, path, access, body, None).await
    }

    async fn call_with(
        &self,
        method: reqwest::Method,
        path: &str,
        access: Option<&str>,
        body: Option<Value>,
        prefer: Option<&str>,
    ) -> Result<Value, ApiError> {
        let mut req = self
            .http
            .request(method, format!("{PROJECT_URL}{path}"))
            .header("apikey", PUBLISHABLE_KEY)
            .bearer_auth(access.unwrap_or(PUBLISHABLE_KEY));
        if let Some(prefer) = prefer {
            req = req.header("Prefer", prefer);
        }
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await.map_err(|_| ApiError::Offline)?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|_| ApiError::Offline)?;
        let value: Value = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        if (200..300).contains(&status) {
            return Ok(value);
        }
        let (code, message) = error_of(&value);
        // PostgREST: an expired or revoked JWT
        if status == 401 && access.is_some() && !path.starts_with("/auth/") {
            return Err(ApiError::Unauthorized);
        }
        let message = friendly(path, status, &code, message);
        Err(ApiError::Refused { status, code, message })
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value(value).map_err(|_| refused("The sync server sent something unexpected"))
}

fn refused(message: &str) -> ApiError {
    ApiError::Refused { status: 0, code: String::new(), message: message.into() }
}

/// (code, message) from a GoTrue or PostgREST error body
fn error_of(value: &Value) -> (String, String) {
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| value.get(*n).and_then(|v| v.as_str().map(str::to_string)))
    };
    let code = field(&["error_code", "code", "error"]).unwrap_or_default();
    let message = field(&["msg", "message", "error_description", "error"])
        .or_else(|| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "The sync server refused the request".into());
    (code, message)
}

/// The server's wording where it is fine for people, ours where it is not.
/// The team functions word their own errors; the codes below mean something
/// else there.
fn friendly(path: &str, status: u16, code: &str, message: String) -> String {
    let personal_vault = path.starts_with("/rest/v1/vaults") || path.starts_with("/rest/v1/rpc/push_items");
    match (status, code) {
        (_, "otp_expired") => "That code is wrong or has expired. Request a new one.".into(),
        (429, _) | (_, "over_email_send_rate_limit") | (_, "over_request_rate_limit") => {
            "Too many attempts. Wait a minute, then try again.".into()
        }
        (_, "email_address_invalid") | (_, "validation_failed") => {
            "That email address doesn't look right.".into()
        }
        (_, "23505") if personal_vault => "This account already has a cloud vault.".into(),
        (_, "P0002") if personal_vault => "The cloud vault no longer exists.".into(),
        (s, _) if s >= 500 => "The sync server had a problem. Try again in a moment.".into(),
        _ => capitalized(message),
    }
}

/// "the team key changed" -> "The team key changed"
fn capitalized(message: String) -> String {
    let mut chars = message.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => first.to_uppercase().chain(chars).collect(),
        _ => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to the real project; sends no email and creates nothing.
    /// `cargo test --lib cloud::api -- --ignored`
    #[tokio::test]
    #[ignore = "needs the network"]
    async fn server_errors_map_to_what_the_app_expects() {
        let api = Api::new();

        match api.verify_code("nobody@example.invalid", "00000000").await {
            Err(ApiError::Refused { code, message, .. }) => {
                assert_eq!(code, "otp_expired", "{message}");
                assert!(message.contains("wrong or has expired"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(api.refresh("not-a-token").await, Err(ApiError::SessionEnded)));
        assert!(matches!(api.get_vault("not-a-jwt").await, Err(ApiError::Unauthorized)));
        assert!(matches!(api.pull("not-a-jwt", 0, 1).await, Err(ApiError::Unauthorized)));
        assert!(matches!(api.push("not-a-jwt", &[]).await, Err(ApiError::Unauthorized)));
    }
}
