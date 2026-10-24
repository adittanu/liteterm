//! Import hosts from the user's OpenSSH config (`~/.ssh/config`) and
//! pre-seed the TOFU table from `~/.ssh/known_hosts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ParsedHost {
    pub alias: String,
    pub hostname: String,
    pub port: i64,
    pub user: Option<String>,
    pub identity_file: Option<String>,
    pub group: Option<String>,
    pub exists: bool, // already in the DB (filled by preview)
}

impl Default for ParsedHost {
    fn default() -> Self {
        Self {
            alias: String::new(),
            hostname: String::new(),
            port: 22,
            user: None,
            identity_file: None,
            group: None,
            exists: false,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ImportPreview {
    pub config_found: bool,
    pub hosts: Vec<ParsedHost>,
    pub known_host_count: usize,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub host_ids: Vec<i64>,
    pub known_imported: usize,
}

/// Extract a group name from a comment line. Supports both the
/// `# @group Name` convention and decorative section headers like
/// `# ── AWS ──────`.
fn group_from_comment(comment: &str) -> Option<String> {
    let text = comment.trim();
    if let Some(rest) = text.strip_prefix("@group") {
        let name = rest.trim();
        return (!name.is_empty()).then(|| name.to_string());
    }
    let trimmed = text.trim_matches(|c: char| {
        matches!(c, '─' | '━' | '═' | '-' | '=' | '—' | '–') || c.is_whitespace()
    });
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn expand_home(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/").or(path.strip_prefix("~\\")) {
        if let Ok(home) = std::env::var("USERPROFILE") {
            let mut p = home;
            if !p.ends_with('\\') && !p.ends_with('/') {
                p.push('\\');
            }
            p.push_str(rest);
            return p;
        }
    }
    path.to_string()
}

fn is_wildcard(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

/// Line-oriented parser for the common OpenSSH config subset:
/// Host / HostName / Port / User / IdentityFile. `Include` is ignored.
pub fn parse_ssh_config(text: &str) -> Vec<ParsedHost> {
    let mut hosts: Vec<ParsedHost> = Vec::new();
    let mut current_group: Option<String> = None;
    let mut current: Option<ParsedHost> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        if let Some(comment) = trimmed.strip_prefix('#') {
            if let Some(group) = group_from_comment(comment) {
                current_group = Some(group);
            }
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }

        // Keys and values are separated by whitespace or `=`
        let (key, value) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => match trimmed.split_once(char::is_whitespace) {
                Some((k, v)) => (k, v.trim()),
                None => (trimmed, ""),
            },
        };

        match key.to_ascii_lowercase().as_str() {
            "host" => {
                if let Some(done) = current.take() {
                    hosts.push(done);
                }
                let patterns: Vec<&str> = value.split_whitespace().collect();
                // A single concrete alias starts a new host; wildcard
                // patterns only carry options and are skipped.
                if patterns.len() == 1 && !is_wildcard(patterns[0]) {
                    current = Some(ParsedHost {
                        alias: patterns[0].to_string(),
                        group: current_group.clone(),
                        ..Default::default()
                    });
                }
            }
            "hostname" => {
                if let Some(h) = current.as_mut() {
                    h.hostname = value.to_string();
                }
            }
            "port" => {
                if let Some(h) = current.as_mut() {
                    if let Ok(p) = value.parse() {
                        h.port = p;
                    }
                }
            }
            "user" => {
                if let Some(h) = current.as_mut() {
                    h.user = Some(value.to_string());
                }
            }
            "identityfile" => {
                if let Some(h) = current.as_mut() {
                    if !value.is_empty() {
                        h.identity_file = Some(expand_home(value));
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(done) = current.take() {
        hosts.push(done);
    }

    // Hosts without an explicit HostName use the alias as the address
    for h in &mut hosts {
        if h.hostname.is_empty() {
            h.hostname = h.alias.clone();
        }
    }
    hosts.retain(|h| !h.hostname.is_empty());
    hosts
}

/// A known_hosts entry mapped to LiteTerm's `addr:port` lookup tag.
#[derive(Debug, Serialize)]
pub struct KnownHostEntry {
    pub host_tag: String,
    pub key_type: String,
    pub public_key: String,
}

/// Parse non-hashed known_hosts lines. Hashed entries (`|1|...`) are
/// skipped — matching them requires HMAC computation we don't need
/// given the TOFU prompt remains as fallback.
pub fn parse_known_hosts(text: &str) -> Vec<KnownHostEntry> {
    let mut entries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Skip: hashed (`|1|...`), revoked (`@revoked host ...`), negated (`!host`)
        if parts.len() < 3
            || parts[0].starts_with('|')
            || parts[0].starts_with('@')
            || parts[0].starts_with('!')
        {
            continue;
        }
        // Only the first host pattern is imported (entries with multiple
        // patterns share the same key)
        let raw = parts[0].split(',').next().unwrap_or(parts[0]);
        let host_tag = if let Some(inner) = raw.strip_prefix('[') {
            match inner.split_once("]:") {
                Some((addr, port)) => format!("{addr}:{port}"),
                None => continue,
            }
        } else {
            format!("{raw}:22")
        };
        entries.push(KnownHostEntry {
            host_tag,
            key_type: parts[1].to_string(),
            public_key: parts[2].to_string(),
        });
    }
    entries
}

pub fn ssh_config_path() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    std::path::Path::new(&home).join(".ssh").join("config")
}

pub fn known_hosts_path() -> std::path::PathBuf {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    std::path::Path::new(&home).join(".ssh").join("known_hosts")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# ── Global defaults ─────────────────────────
Host *
  ServerAliveInterval 30

# ── AWS ─────────────────────────────────────
# @group AWS

Host bersamabot
  HostName 43.218.119.113
  User ubuntu
  IdentityFile ~/.ssh/keys/bersamabot-key.pem
  StrictHostKeyChecking no

Host zeroclaw
  HostName 43.218.127.216
  User ubuntu
  IdentityFile ~/.ssh/keys/bersamadot-key.pem

# ── Homelab ──
Host nas
  HostName 192.168.1.10
  Port 2222
  User admin
"#;

    #[test]
    fn parses_groups_hosts_and_options() {
        let hosts = parse_ssh_config(SAMPLE);
        assert_eq!(hosts.len(), 3, "wildcard Host * must be skipped");

        let bot = &hosts[0];
        assert_eq!(bot.alias, "bersamabot");
        assert_eq!(bot.hostname, "43.218.119.113");
        assert_eq!(bot.port, 22);
        assert_eq!(bot.user.as_deref(), Some("ubuntu"));
        assert_eq!(bot.group.as_deref(), Some("AWS"), "@group wins over header");
        assert!(bot.identity_file.as_deref().unwrap_or("").ends_with("bersamabot-key.pem"));

        assert_eq!(hosts[1].group.as_deref(), Some("AWS"));

        let nas = &hosts[2];
        assert_eq!(nas.alias, "nas");
        assert_eq!(nas.port, 2222);
        assert_eq!(nas.group.as_deref(), Some("Homelab"), "decorative header becomes group");
    }

    #[test]
    fn alias_used_when_hostname_missing() {
        let hosts = parse_ssh_config("Host web\n  User root\n");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "web");
    }

    #[test]
    fn equals_syntax_supported() {
        let hosts = parse_ssh_config("Host=box\nHostName=10.0.0.1\nPort=2200\n");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "10.0.0.1");
        assert_eq!(hosts[0].port, 2200);
    }

    #[test]
    fn known_hosts_plain_entries_mapped() {
        let text = concat!(
            "example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAItest\n",
            "[example.com]:2222 ssh-rsa AAAAB3NzaC1yc2EAAAtest\n",
            "|1|salt|hash ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIhashed\n",
            "@revoked example.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIrev\n",
            "\n",
            "# comment\n",
        );
        let entries = parse_known_hosts(text);
        assert_eq!(entries.len(), 2, "hashed and @revoked entries skipped");
        assert_eq!(entries[0].host_tag, "example.com:22");
        assert_eq!(entries[0].key_type, "ssh-ed25519");
        assert_eq!(entries[1].host_tag, "example.com:2222");
    }
}
