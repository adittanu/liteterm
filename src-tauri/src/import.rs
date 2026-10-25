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
    // File-import extras (always None for the ~/.ssh/config flow)
    pub password: Option<String>,
    pub key_pem: Option<String>, // inline private key material (Termius exports embed PEM)
    pub key_passphrase: Option<String>,
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
            password: None,
            key_pem: None,
            key_passphrase: None,
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

/// Result of auto-detecting and parsing an import file.
#[derive(Debug, Serialize)]
pub struct FileImport {
    pub format: &'static str, // "liteterm" | "termius" | "ssh-config"
    pub hosts: Vec<ParsedHost>,
}

pub fn parse_import_text(text: &str) -> Result<FileImport, String> {
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        let root: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("Not a valid JSON file: {e}"))?;

        let format = if root
            .get("app")
            .and_then(|a| a.as_str())
            .is_some_and(|a| a.eq_ignore_ascii_case("liteterm"))
        {
            "liteterm"
        } else {
            "termius"
        };

        let host_values: Vec<serde_json::Value> = match &root {
            serde_json::Value::Array(items) => items.clone(),
            serde_json::Value::Object(_) => root
                .get("hosts")
                .and_then(|h| h.as_array())
                .cloned()
                .ok_or("JSON has no \"hosts\" list")?,
            _ => return Err("Unsupported JSON shape".into()),
        };

        let index = JsonIndex::build(&root);
        let hosts: Vec<ParsedHost> = host_values
            .iter()
            .filter_map(|h| host_from_json(h, &index))
            .collect();
        if hosts.is_empty() {
            return Err("No hosts with an address found in this file".into());
        }
        Ok(FileImport { format, hosts })
    } else {
        let hosts = parse_ssh_config(text);
        if hosts.is_empty() {
            return Err("No hosts found in this SSH config file".into());
        }
        Ok(FileImport {
            format: "ssh-config",
            hosts,
        })
    }
}

// ---------- tolerant JSON walking (Termius / LiteTerm exports) ----------

fn jstr(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn jnum(v: &serde_json::Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .filter(|n| *n > 0 && *n <= 65535)
}

fn first_str(obj: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| jstr(&obj[*k]))
}

fn id_key(v: Option<&serde_json::Value>) -> Option<String> {
    match v? {
        serde_json::Value::String(s) => (!s.trim().is_empty()).then(|| s.trim().to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Cross-reference tables for Termius-style bundles where hosts point at
/// top-level `identities` / `keys` / `groups` lists by id.
struct JsonIndex {
    identities: std::collections::HashMap<String, serde_json::Value>,
    keys: std::collections::HashMap<String, serde_json::Value>,
    groups: std::collections::HashMap<String, serde_json::Value>,
}

impl JsonIndex {
    fn build(root: &serde_json::Value) -> Self {
        let mut idx = JsonIndex {
            identities: Default::default(),
            keys: Default::default(),
            groups: Default::default(),
        };
        let insert = |map: &mut std::collections::HashMap<String, serde_json::Value>,
                      item: &serde_json::Value| {
            for k in [id_key(item.get("id")), id_key(item.get("localId"))].into_iter().flatten() {
                map.insert(k, item.clone());
            }
        };
        for list in ["identities", "ssh_identities"] {
            if let Some(items) = root.get(list).and_then(|v| v.as_array()) {
                for item in items {
                    insert(&mut idx.identities, item);
                }
            }
        }
        for list in ["keys", "ssh_keys", "multiKeys"] {
            if let Some(items) = root.get(list).and_then(|v| v.as_array()) {
                for item in items {
                    insert(&mut idx.keys, item);
                }
            }
        }
        if let Some(items) = root.get("groups").and_then(|v| v.as_array()) {
            for item in items {
                insert(&mut idx.groups, item);
            }
        }
        idx
    }

    fn get<'a>(
        map: &'a std::collections::HashMap<String, serde_json::Value>,
        v: &serde_json::Value,
    ) -> Option<&'a serde_json::Value> {
        match v {
            serde_json::Value::Object(o) => id_key(o.get("id")).and_then(|k| map.get(&k)),
            serde_json::Value::String(_) | serde_json::Value::Number(_) => {
                id_key(Some(v)).and_then(|k| map.get(&k))
            }
            _ => None,
        }
    }
}

fn group_from_json(host: &serde_json::Value, idx: &JsonIndex) -> Option<String> {
    if let Some(g) = first_str(host, &["groupPath", "group_path", "folderPath"]) {
        return Some(g);
    }
    let raw = host.get("group").or_else(|| host.get("folder"))?;
    match raw {
        serde_json::Value::String(_) => jstr(raw),
        serde_json::Value::Object(_) => {
            first_str(raw, &["path", "label", "name"]).or_else(|| {
                JsonIndex::get(&idx.groups, raw)
                    .and_then(|gr| first_str(gr, &["path", "name", "label"]))
            })
        }
        serde_json::Value::Number(_) => JsonIndex::get(&idx.groups, raw)
            .and_then(|gr| first_str(gr, &["path", "name", "label"])),
        _ => None,
    }
}

fn host_from_json(host: &serde_json::Value, idx: &JsonIndex) -> Option<ParsedHost> {
    let address = first_str(host, &["address", "hostname", "host", "ip", "url"])?;
    let label = first_str(host, &["name", "label", "alias", "title"]).unwrap_or_else(|| address.clone());
    let port = jnum(&host["port"])
        .or_else(|| {
            host.get("sshConfig").and_then(|c| {
                jnum(&c["port"]).or_else(|| c.get("content").and_then(|cc| jnum(&cc["port"])))
            })
        })
        .unwrap_or(22);

    let mut user = first_str(host, &["username", "user"]);
    let mut password = first_str(host, &["password"]);
    let mut key_pem = first_str(host, &["privateKeyPem", "private_key", "privateKey"]);
    let mut key_passphrase = first_str(host, &["passphrase"]);
    let identity_file = first_str(host, &["keyPath", "key_path", "identityFile", "identity_file"]);

    // Termius nests credentials in `identity` (inline object or id reference)
    if let Some(raw) = host.get("identity") {
        let identity = match raw {
            serde_json::Value::Object(o) => {
                let inline = o.contains_key("username")
                    || o.contains_key("password")
                    || o.contains_key("sshKey")
                    || o.contains_key("ssh_key");
                if inline {
                    Some(raw.clone())
                } else {
                    id_key(raw.get("id")).and_then(|k| idx.identities.get(&k).cloned())
                }
            }
            _ => id_key(Some(raw)).and_then(|k| idx.identities.get(&k).cloned()),
        };
        if let Some(id) = identity {
            user = user.or_else(|| first_str(&id, &["username", "user"]));
            password = password.or_else(|| first_str(&id, &["password"]));

            let key_raw = id.get("sshKey").or_else(|| id.get("ssh_key")).or_else(|| id.get("key"));
            if let Some(kr) = key_raw {
                let key = match kr {
                    serde_json::Value::Object(o) => {
                        let inline = o.contains_key("privateKeyPem")
                            || o.contains_key("private_key")
                            || o.contains_key("privateKey")
                            || o.contains_key("passphrase");
                        if inline {
                            Some(kr.clone())
                        } else {
                            id_key(kr.get("id")).and_then(|k| idx.keys.get(&k).cloned())
                        }
                    }
                    _ => id_key(Some(kr)).and_then(|k| idx.keys.get(&k).cloned()),
                };
                if let Some(k) = key {
                    key_pem =
                        key_pem.or_else(|| first_str(&k, &["privateKeyPem", "private_key", "privateKey"]));
                    key_passphrase = key_passphrase.or_else(|| first_str(&k, &["passphrase"]));
                }
            }
        }
    }

    Some(ParsedHost {
        alias: label,
        hostname: address,
        port,
        user,
        identity_file,
        group: group_from_json(host, idx),
        exists: false,
        password: password.filter(|p| !p.is_empty()),
        key_pem: key_pem.filter(|k| k.contains("PRIVATE KEY") || k.contains("OPENSSH")),
        key_passphrase,
    })
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

    const PEM: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc123\n-----END OPENSSH PRIVATE KEY-----";

    #[test]
    fn termius_bundle_with_inline_identity() {
        let text = format!(
            r#"{{
                "meta": {{"hosts": 2}},
                "groups": [{{"id": 7, "name": "Staging", "path": "Work/Staging"}}],
                "hosts": [
                    {{
                        "id": 1, "name": "web-01", "address": "10.0.0.5",
                        "groupPath": "Work/Staging",
                        "identity": {{"username": "ubuntu", "password": "s3cret",
                                      "sshKey": {{"name": "ops", "privateKeyPem": {pem}, "passphrase": "pp"}}}},
                        "sshConfig": {{"port": 2222}}
                    }},
                    {{"id": 2, "name": "db", "address": "10.0.0.9", "port": 22,
                      "identity": {{"username": "root"}}}}
                ]
            }}"#,
            pem = serde_json::json!(PEM)
        );
        let parsed = parse_import_text(&text).unwrap();
        assert_eq!(parsed.format, "termius");
        assert_eq!(parsed.hosts.len(), 2);

        let web = &parsed.hosts[0];
        assert_eq!(web.alias, "web-01");
        assert_eq!(web.hostname, "10.0.0.5");
        assert_eq!(web.port, 2222);
        assert_eq!(web.user.as_deref(), Some("ubuntu"));
        assert_eq!(web.password.as_deref(), Some("s3cret"));
        assert_eq!(web.key_pem.as_deref(), Some(PEM));
        assert_eq!(web.key_passphrase.as_deref(), Some("pp"));
        assert_eq!(web.group.as_deref(), Some("Work/Staging"));

        let db = &parsed.hosts[1];
        assert_eq!(db.port, 22);
        assert!(db.password.is_none() && db.key_pem.is_none());
    }

    #[test]
    fn termius_bundle_with_id_references() {
        let text = r#"{
            "hosts": [
                {"id": 11, "name": "jump", "address": "gw.example.com",
                 "identity": {"id": 50}, "group": {"id": 9}}
            ],
            "identities": [
                {"id": 50, "username": "deploy", "sshKey": {"id": 90}}
            ],
            "keys": [
                {"id": 90, "name": "deploy.pem", "privateKeyPem": "-----BEGIN OPENSSH PRIVATE KEY-----\nz\n-----END OPENSSH PRIVATE KEY-----", "passphrase": null}
            ],
            "groups": [
                {"id": 9, "name": "Infra", "path": "Infra"}
            ]
        }"#;
        let parsed = parse_import_text(text).unwrap();
        assert_eq!(parsed.hosts.len(), 1);
        let h = &parsed.hosts[0];
        assert_eq!(h.user.as_deref(), Some("deploy"));
        assert!(h.key_pem.as_deref().unwrap().starts_with("-----BEGIN"));
        assert_eq!(h.group.as_deref(), Some("Infra"));
    }

    #[test]
    fn liteterm_round_trip() {
        let text = r#"{
            "app": "liteterm",
            "version": 1,
            "groups": [{"name": "AWS"}],
            "hosts": [
                {"label": "nas", "address": "192.168.1.10", "port": 2222,
                 "username": "admin", "authMethod": "key",
                 "group": "AWS", "keyPath": "C:\\Users\\me\\.ssh\\id_ed25519"}
            ]
        }"#;
        let parsed = parse_import_text(text).unwrap();
        assert_eq!(parsed.format, "liteterm");
        let h = &parsed.hosts[0];
        assert_eq!(h.alias, "nas");
        assert_eq!(h.port, 2222);
        assert_eq!(h.user.as_deref(), Some("admin"));
        assert_eq!(h.identity_file.as_deref(), Some("C:\\Users\\me\\.ssh\\id_ed25519"));
        assert_eq!(h.group.as_deref(), Some("AWS"));
    }

    #[test]
    fn plain_array_and_ssh_config_detected() {
        let arr = r#"[{"label": "box", "hostname": "1.2.3.4", "port": 2200, "username": "root", "group": "Misc"}]"#;
        let parsed = parse_import_text(arr).unwrap();
        assert_eq!(parsed.format, "termius");
        assert_eq!(parsed.hosts[0].hostname, "1.2.3.4");
        assert_eq!(parsed.hosts[0].group.as_deref(), Some("Misc"));

        let ssh = parse_import_text("Host nas\n  HostName 192.168.1.10\n  Port 2222\n").unwrap();
        assert_eq!(ssh.format, "ssh-config");
        assert_eq!(ssh.hosts.len(), 1);

        assert!(parse_import_text("{\"groups\": []}").is_err());
        assert!(parse_import_text("not a config at all: [broken").is_err());
    }
}
