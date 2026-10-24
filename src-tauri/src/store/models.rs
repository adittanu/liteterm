use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Host {
    pub id: i64,
    pub group_id: Option<i64>,
    pub label: String,
    pub address: String,
    pub port: i64,
    pub username: String,
    pub auth_method: String, // "password" | "key"
    pub password_saved: bool,
    pub has_password: bool,
    pub key_path: Option<String>,
    pub credential_id: Option<i64>,
    pub credential_name: Option<String>,
    pub os: Option<String>, // detected distro id ("ubuntu", "debian", "macos", …)
    pub sort: i64,
}

#[derive(Debug, Deserialize)]
pub struct HostInput {
    pub id: Option<i64>,
    pub group_id: Option<i64>,
    pub label: String,
    pub address: String,
    pub port: i64,
    pub username: String,
    pub auth_method: String,
    pub password: Option<String>,
    pub save_password: Option<bool>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
    pub credential_id: Option<i64>,
    pub create_credential: Option<CredentialInput>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Credential {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub auth_method: String, // "password" | "key"
    pub password_saved: bool,
    pub has_password: bool,
    pub key_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CredentialInput {
    pub name: String,
    pub username: String,
    pub auth_method: String,
    pub password: Option<String>,
    pub save_password: Option<bool>,
    pub key_path: Option<String>,
    pub key_passphrase: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HostGroup {
    pub id: i64,
    pub name: String,
    pub sort: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snippet {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub sort: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortForward {
    pub id: i64,
    pub host_id: i64,
    pub name: String,
    pub local_port: i64,
    pub remote_host: String,
    pub remote_port: i64,
    pub auto_start: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub kind: String, // "file" | "dir" | "symlink" | "other"
    pub size: i64,
    pub mtime: i64,
    pub permissions: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String, // "system" | "light" | "dark" | "catppuccin-mocha" | ...
    pub terminal_font_size: i64,
    pub confirm_delete_file: bool,
    pub auto_sudo_password: bool,
    #[serde(default)]
    pub keybindings: HashMap<String, String>, // action id -> combo ("Ctrl+K")
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            terminal_font_size: 13,
            confirm_delete_file: true,
            auto_sudo_password: true,
            keybindings: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyEvent {
    pub session_id: u32,
    pub fingerprint: String,
    pub key_type: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionLog {
    pub id: i64,
    pub host_id: Option<i64>,
    pub label: String,
    pub username: String,
    pub address: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KnownHostRow {
    pub host: String,
    pub key_type: String,
    pub public_key: String,
    pub fingerprint: String,
    pub added_at: String,
}
