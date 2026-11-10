//! Synced rows as plaintext items, both ways: what the outbox says to push,
//! and applying what a pull brought in.
//!
//! An item is one row as JSON, tagged with its kind. Relations name their
//! target by sync_id, never by the integer id, which differs per device. A
//! host's or credential's saved password and key passphrase ride along: they
//! live in the OS keyring here, and the item is encrypted before it leaves.
//!
//! Applying follows the rules in `store/sync.rs`: every write sets
//! `modified_at` itself, so the change-tracking triggers stay quiet and
//! nothing is echoed back. The newer edit wins; an equal one (our own push
//! coming back) changes nothing.

use std::cmp::Reverse;
use std::collections::HashSet;
use std::fmt;

use rusqlite::types::ToSql;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::store::db;
use crate::store::sync::{self, SyncedTable};

/// Where saved passwords and key passphrases live: the OS keyring in the app,
/// a map in tests.
pub trait Secrets {
    fn get(&self, account: &str) -> Option<String>;
    fn set(&self, account: &str, value: &str);
    fn delete(&self, account: &str);
}

pub struct Keyring;

impl Secrets for Keyring {
    fn get(&self, account: &str) -> Option<String> {
        db::keyring_get(account)
    }
    fn set(&self, account: &str, value: &str) {
        db::keyring_set(account, value)
    }
    fn delete(&self, account: &str) {
        db::keyring_delete(account)
    }
}

/// A password or passphrase; kept out of Debug output
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(…)")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Payload {
    Group(GroupData),
    Host(HostData),
    Credential(CredentialData),
    Snippet(SnippetData),
    Note(NoteData),
    PortForward(PortForwardData),
    KnownHost(KnownHostData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupData {
    pub name: String,
    #[serde(default)]
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostData {
    pub label: String,
    pub address: String,
    pub port: i64,
    pub username: String,
    pub auth_method: String,
    #[serde(default)]
    pub key_path: Option<String>,
    /// sync_id of its group
    #[serde(default)]
    pub group: Option<String>,
    /// sync_id of its credential
    #[serde(default)]
    pub credential: Option<String>,
    /// sync_id of the host it jumps through
    #[serde(default)]
    pub jump_host: Option<String>,
    #[serde(default)]
    pub sort: i64,
    #[serde(default)]
    pub password_saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_passphrase: Option<Secret>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CredentialData {
    pub name: String,
    pub username: String,
    pub auth_method: String,
    #[serde(default)]
    pub key_path: Option<String>,
    #[serde(default)]
    pub password_saved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_passphrase: Option<Secret>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnippetData {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub sort: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteData {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// sync_id of the host it is linked to
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub sort: i64,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortForwardData {
    /// sync_id of its host
    pub host: String,
    pub name: String,
    pub local_port: i64,
    pub remote_host: String,
    pub remote_port: i64,
    #[serde(default)]
    pub auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownHostData {
    pub host: String,
    pub key_type: String,
    pub public_key: String,
    #[serde(default)]
    pub fingerprint: String,
}

impl Payload {
    pub fn table(&self) -> &'static SyncedTable {
        match self {
            Payload::Group(_) => &sync::GROUPS,
            Payload::Host(_) => &sync::HOSTS,
            Payload::Credential(_) => &sync::CREDENTIALS,
            Payload::Snippet(_) => &sync::SNIPPETS,
            Payload::Note(_) => &sync::NOTES,
            Payload::PortForward(_) => &sync::PORT_FORWARDS,
            Payload::KnownHost(_) => &sync::KNOWN_HOSTS,
        }
    }
}

/// Targets before the rows that point at them: a pull applies (and a push
/// sends) items in this order, deletes in the reverse one.
const ORDER: [&str; 7] = [
    "group",
    "credential",
    "host",
    "snippet",
    "note",
    "port_forward",
    "known_host",
];

fn order_of(kind: &str) -> usize {
    ORDER.iter().position(|k| *k == kind).unwrap_or(ORDER.len())
}

fn table_of(kind: &str) -> Option<&'static SyncedTable> {
    sync::SYNCED.into_iter().find(|t| t.kind == kind)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Push: the outbox as items
// ---------------------------------------------------------------------------

/// One outbox entry, ready to encrypt
#[derive(Debug)]
pub struct Queued {
    pub sync_id: String,
    pub kind: String,
    /// None: a tombstone
    pub payload: Option<Payload>,
    pub modified_at: i64,
    /// The entry's changed_at as read. [`clear_pushed`] removes the entry only
    /// if it is still this, so an edit made during the push stays queued.
    pub changed_at: i64,
}

/// Everything the outbox holds: changes in dependency order, then deletes in
/// the reverse one. An entry whose row is gone without a tombstone (or of a
/// kind this version does not know) has nothing to send and is dropped.
pub fn queued(conn: &Connection, secrets: &dyn Secrets) -> Result<Vec<Queued>, rusqlite::Error> {
    let entries: Vec<(String, String, bool, i64)> = conn
        .prepare("SELECT sync_id, kind, deleted, changed_at FROM sync_outbox")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?)))?
        .collect::<Result<_, _>>()?;

    let mut out = Vec::with_capacity(entries.len());
    for (sync_id, kind, deleted, changed_at) in entries {
        if deleted {
            out.push(Queued { sync_id, kind, payload: None, modified_at: changed_at, changed_at });
            continue;
        }
        match read_payload(conn, secrets, &kind, &sync_id)? {
            Some((payload, modified_at)) => {
                out.push(Queued { sync_id, kind, payload: Some(payload), modified_at, changed_at })
            }
            None => {
                conn.execute(
                    "DELETE FROM sync_outbox WHERE sync_id = ?1 AND changed_at = ?2",
                    params![sync_id, changed_at],
                )?;
            }
        }
    }
    out.sort_by_key(|q| {
        let order = order_of(&q.kind);
        match q.payload {
            Some(_) => (0, order, q.changed_at),
            None => (1, ORDER.len() - order, q.changed_at),
        }
    });
    Ok(out)
}

/// Clear the entries the server has taken
pub fn clear_pushed(conn: &Connection, pushed: &[Queued]) -> Result<(), rusqlite::Error> {
    let mut stmt = conn.prepare("DELETE FROM sync_outbox WHERE sync_id = ?1 AND changed_at = ?2")?;
    for q in pushed {
        stmt.execute(params![q.sync_id, q.changed_at])?;
    }
    Ok(())
}

fn read_payload(
    conn: &Connection,
    secrets: &dyn Secrets,
    kind: &str,
    sync_id: &str,
) -> Result<Option<(Payload, i64)>, rusqlite::Error> {
    let one = |sql: &str, f: &dyn Fn(&rusqlite::Row) -> rusqlite::Result<(Payload, i64)>| {
        conn.query_row(sql, [sync_id], |r| f(r)).optional()
    };
    Ok(match kind {
        "group" => one("SELECT name, sort, modified_at FROM groups WHERE sync_id = ?1", &|r| {
            Ok((Payload::Group(GroupData { name: r.get(0)?, sort: r.get(1)? }), r.get(2)?))
        })?,
        "host" => {
            let row = conn
                .query_row(
                    "SELECT h.id, h.label, h.address, h.port, h.username, h.auth_method, h.key_path, \
                     g.sync_id, c.sync_id, j.sync_id, h.sort, h.password_saved, h.modified_at \
                     FROM hosts h \
                     LEFT JOIN groups g ON g.id = h.group_id \
                     LEFT JOIN credentials c ON c.id = h.credential_id \
                     LEFT JOIN hosts j ON j.id = h.jump_host_id \
                     WHERE h.sync_id = ?1",
                    [sync_id],
                    |r| {
                        let data = HostData {
                            label: r.get(1)?,
                            address: r.get(2)?,
                            port: r.get(3)?,
                            username: r.get(4)?,
                            auth_method: r.get(5)?,
                            key_path: r.get(6)?,
                            group: r.get(7)?,
                            credential: r.get(8)?,
                            jump_host: r.get(9)?,
                            sort: r.get(10)?,
                            password_saved: r.get::<_, i64>(11)? != 0,
                            password: None,
                            key_passphrase: None,
                        };
                        Ok((r.get::<_, i64>(0)?, data, r.get::<_, i64>(12)?))
                    },
                )
                .optional()?;
            row.map(|(id, mut d, modified_at)| {
                if d.password_saved {
                    d.password = secrets.get(&db::password_entry(id)).map(Secret);
                }
                d.key_passphrase = secrets.get(&db::keypass_entry(id)).map(Secret);
                (Payload::Host(d), modified_at)
            })
        }
        "credential" => {
            let row = conn
                .query_row(
                    "SELECT id, name, username, auth_method, key_path, password_saved, modified_at \
                     FROM credentials WHERE sync_id = ?1",
                    [sync_id],
                    |r| {
                        let data = CredentialData {
                            name: r.get(1)?,
                            username: r.get(2)?,
                            auth_method: r.get(3)?,
                            key_path: r.get(4)?,
                            password_saved: r.get::<_, i64>(5)? != 0,
                            password: None,
                            key_passphrase: None,
                        };
                        Ok((r.get::<_, i64>(0)?, data, r.get::<_, i64>(6)?))
                    },
                )
                .optional()?;
            row.map(|(id, mut d, modified_at)| {
                if d.password_saved {
                    d.password = secrets.get(&db::cred_password_entry(id)).map(Secret);
                }
                d.key_passphrase = secrets.get(&db::cred_keypass_entry(id)).map(Secret);
                (Payload::Credential(d), modified_at)
            })
        }
        "snippet" => one("SELECT name, command, sort, modified_at FROM snippets WHERE sync_id = ?1", &|r| {
            Ok((
                Payload::Snippet(SnippetData { name: r.get(0)?, command: r.get(1)?, sort: r.get(2)? }),
                r.get(3)?,
            ))
        })?,
        "note" => one(
            "SELECT n.title, n.body, n.pinned, n.tags, h.sync_id, n.sort, n.archived, \
             n.created_at, n.updated_at, n.modified_at \
             FROM notes n LEFT JOIN hosts h ON h.id = n.host_id WHERE n.sync_id = ?1",
            &|r| {
                let tags: String = r.get(3)?;
                Ok((
                    Payload::Note(NoteData {
                        title: r.get(0)?,
                        body: r.get(1)?,
                        pinned: r.get::<_, i64>(2)? != 0,
                        tags: serde_json::from_str(&tags).unwrap_or_default(),
                        host: r.get(4)?,
                        sort: r.get(5)?,
                        archived: r.get::<_, i64>(6)? != 0,
                        created_at: r.get(7)?,
                        updated_at: r.get(8)?,
                    }),
                    r.get(9)?,
                ))
            },
        )?,
        "port_forward" => one(
            "SELECT h.sync_id, f.name, f.local_port, f.remote_host, f.remote_port, f.auto_start, \
             f.modified_at FROM port_forwards f JOIN hosts h ON h.id = f.host_id \
             WHERE f.sync_id = ?1",
            &|r| {
                Ok((
                    Payload::PortForward(PortForwardData {
                        host: r.get(0)?,
                        name: r.get(1)?,
                        local_port: r.get(2)?,
                        remote_host: r.get(3)?,
                        remote_port: r.get(4)?,
                        auto_start: r.get::<_, i64>(5)? != 0,
                    }),
                    r.get(6)?,
                ))
            },
        )?,
        "known_host" => one(
            "SELECT host, key_type, public_key, fingerprint, modified_at FROM known_hosts \
             WHERE sync_id = ?1",
            &|r| {
                Ok((
                    Payload::KnownHost(KnownHostData {
                        host: r.get(0)?,
                        key_type: r.get(1)?,
                        public_key: r.get(2)?,
                        fingerprint: r.get(3)?,
                    }),
                    r.get(4)?,
                ))
            },
        )?,
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// Pull: applying items
// ---------------------------------------------------------------------------

/// One item from the server, decrypted
#[derive(Debug, Clone)]
pub struct Incoming {
    pub sync_id: String,
    /// None: deleted
    pub payload: Option<Payload>,
    pub modified_at: i64,
}

#[derive(Debug, PartialEq)]
pub enum SecretOp {
    Set(String, Secret),
    Delete(String),
}

#[derive(Debug, Default)]
pub struct Applied {
    /// Kinds whose rows changed here, so the UI knows what to reload
    pub kinds: HashSet<&'static str>,
    /// Keyring writes, made once the transaction has committed: a rolled-back
    /// insert could hand its id, and so its keyring entry, to another row
    pub secrets: Vec<SecretOp>,
    /// Items that could not be placed (a port forward whose host is not here)
    pub skipped: usize,
}

impl Applied {
    pub fn write_secrets(&self, secrets: &dyn Secrets) {
        for op in &self.secrets {
            match op {
                SecretOp::Set(account, value) => secrets.set(account, &value.0),
                SecretOp::Delete(account) => secrets.delete(account),
            }
        }
    }

    fn secret(&mut self, account: String, value: Option<&Secret>, keep_if_missing: bool) {
        match value {
            Some(v) => self.secrets.push(SecretOp::Set(account, v.clone())),
            None if !keep_if_missing => self.secrets.push(SecretOp::Delete(account)),
            None => {}
        }
    }
}

/// Apply a pull. Run it inside a transaction, together with moving the pull
/// cursor; then [`Applied::write_secrets`] after the commit.
pub fn apply(conn: &Connection, items: Vec<Incoming>) -> Result<Applied, rusqlite::Error> {
    let mut out = Applied::default();
    let (mut upserts, deletes): (Vec<Incoming>, Vec<Incoming>) =
        items.into_iter().partition(|i| i.payload.is_some());

    // Stable: within a kind, server order stays
    upserts.sort_by_key(|i| i.payload.as_ref().map_or(ORDER.len(), |p| order_of(p.table().kind)));
    let mut jumps = Vec::new();
    for item in upserts {
        apply_upsert(conn, item, &mut out, &mut jumps)?;
    }
    // A host can jump through one that came later in the same pull
    for (sync_id, jump, modified_at) in jumps {
        if let Some(jump_id) = local_id(conn, "hosts", &jump)? {
            // The update trigger takes this for a local edit; undo what it did
            conn.execute("UPDATE hosts SET jump_host_id = ?1 WHERE sync_id = ?2", params![jump_id, sync_id])?;
            conn.execute("UPDATE hosts SET modified_at = ?1 WHERE sync_id = ?2", params![modified_at, sync_id])?;
            drop_outbox(conn, &sync_id)?;
        }
    }

    // Deletes last, dependents first: a forward deleted before its host is not
    // also cascaded (and queued) here
    let mut doomed = Vec::new();
    for item in deletes {
        match locate(conn, &item.sync_id)? {
            Some((table, rowid, local_modified)) => {
                // A newer local edit outlives an older delete, and stays queued
                if item.modified_at > local_modified {
                    doomed.push((table, rowid, item.sync_id));
                }
            }
            // Deleted on both sides
            None => drop_outbox(conn, &item.sync_id)?,
        }
    }
    doomed.sort_by_key(|(table, _, _)| Reverse(order_of(table.kind)));
    for (table, rowid, sync_id) in doomed {
        conn.execute(&format!("DELETE FROM {} WHERE rowid = ?1", table.table), [rowid])?;
        drop_outbox(conn, &sync_id)?;
        match table.kind {
            "host" => {
                out.secrets.push(SecretOp::Delete(db::password_entry(rowid)));
                out.secrets.push(SecretOp::Delete(db::keypass_entry(rowid)));
            }
            "credential" => {
                out.secrets.push(SecretOp::Delete(db::cred_password_entry(rowid)));
                out.secrets.push(SecretOp::Delete(db::cred_keypass_entry(rowid)));
            }
            _ => {}
        }
        out.kinds.insert(table.kind);
    }
    Ok(out)
}

enum Decision {
    Skip,
    Update(i64),
    Insert,
}

fn decide(conn: &Connection, table: &str, sync_id: &str, remote: i64) -> Result<Decision, rusqlite::Error> {
    if let Some((rowid, local)) = find(conn, table, sync_id)? {
        return Ok(if remote > local { Decision::Update(rowid) } else { Decision::Skip });
    }
    // Deleted here: an older edit stays deleted, a newer one brings it back
    let tombstone: Option<i64> = conn
        .query_row(
            "SELECT changed_at FROM sync_outbox WHERE sync_id = ?1 AND deleted = 1",
            [sync_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match tombstone {
        Some(deleted_at) if remote <= deleted_at => Decision::Skip,
        _ => Decision::Insert,
    })
}

fn apply_upsert(
    conn: &Connection,
    item: Incoming,
    out: &mut Applied,
    jumps: &mut Vec<(String, String, i64)>,
) -> Result<(), rusqlite::Error> {
    let Some(payload) = item.payload else { return Ok(()) };
    let table = payload.table();
    let (sid, m) = (item.sync_id.as_str(), item.modified_at);
    let existing = match decide(conn, table.table, sid, m)? {
        Decision::Skip => return Ok(()),
        Decision::Update(rowid) => Some(rowid),
        Decision::Insert => None,
    };

    match &payload {
        Payload::Group(d) => {
            write_row(conn, "groups", existing, sid, m, &[("name", &d.name), ("sort", &d.sort)])?;
        }
        Payload::Credential(d) => {
            let id = write_row(
                conn,
                "credentials",
                existing,
                sid,
                m,
                &[
                    ("name", &d.name),
                    ("username", &d.username),
                    ("auth_method", &d.auth_method),
                    ("key_path", &d.key_path),
                    ("password_saved", &d.password_saved),
                ],
            )?;
            out.secret(db::cred_password_entry(id), d.password.as_ref(), d.password_saved);
            out.secret(db::cred_keypass_entry(id), d.key_passphrase.as_ref(), false);
        }
        Payload::Host(d) => {
            let group_id = resolve(conn, "groups", d.group.as_deref())?;
            let credential_id = resolve(conn, "credentials", d.credential.as_deref())?;
            let jump = d.jump_host.as_deref().filter(|j| *j != sid);
            let jump_id = resolve(conn, "hosts", jump)?;
            let id = write_row(
                conn,
                "hosts",
                existing,
                sid,
                m,
                &[
                    ("label", &d.label),
                    ("address", &d.address),
                    ("port", &d.port),
                    ("username", &d.username),
                    ("auth_method", &d.auth_method),
                    ("key_path", &d.key_path),
                    ("group_id", &group_id),
                    ("credential_id", &credential_id),
                    ("jump_host_id", &jump_id),
                    ("sort", &d.sort),
                    ("password_saved", &d.password_saved),
                ],
            )?;
            if let (Some(jump), None) = (jump, jump_id) {
                jumps.push((sid.to_string(), jump.to_string(), m));
            }
            // A host that says it has a password but sent none (its device
            // could not read its keyring) keeps whatever this one has
            out.secret(db::password_entry(id), d.password.as_ref(), d.password_saved);
            out.secret(db::keypass_entry(id), d.key_passphrase.as_ref(), false);
        }
        Payload::Snippet(d) => {
            write_row(
                conn,
                "snippets",
                existing,
                sid,
                m,
                &[("name", &d.name), ("command", &d.command), ("sort", &d.sort)],
            )?;
        }
        Payload::Note(d) => {
            let host_id = resolve(conn, "hosts", d.host.as_deref())?;
            let tags = serde_json::to_string(&d.tags).unwrap_or_else(|_| "[]".into());
            let mut cols: Vec<(&str, &dyn ToSql)> = vec![
                ("title", &d.title),
                ("body", &d.body),
                ("pinned", &d.pinned),
                ("tags", &tags),
                ("host_id", &host_id),
                ("sort", &d.sort),
                ("archived", &d.archived),
            ];
            if let Some(at) = &d.created_at {
                cols.push(("created_at", at));
            }
            if let Some(at) = &d.updated_at {
                cols.push(("updated_at", at));
            }
            write_row(conn, "notes", existing, sid, m, &cols)?;
        }
        Payload::PortForward(d) => {
            let Some(host_id) = local_id(conn, "hosts", &d.host)? else {
                // Its host was deleted, or never arrived: nowhere to put it
                out.skipped += 1;
                return Ok(());
            };
            write_row(
                conn,
                "port_forwards",
                existing,
                sid,
                m,
                &[
                    ("host_id", &host_id),
                    ("name", &d.name),
                    ("local_port", &d.local_port),
                    ("remote_host", &d.remote_host),
                    ("remote_port", &d.remote_port),
                    ("auto_start", &d.auto_start),
                ],
            )?;
        }
        Payload::KnownHost(d) => {
            let twin: Option<(i64, String, i64)> = conn
                .query_row(
                    "SELECT rowid, sync_id, modified_at FROM known_hosts \
                     WHERE host = ?1 AND key_type = ?2",
                    params![d.host, d.key_type],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match (twin, existing) {
                (Some((rowid, twin_id, twin_modified)), None) => {
                    return merge_known_host(conn, rowid, &twin_id, twin_modified, sid, m, d, out);
                }
                // The same entry under two ids here; leave it for its twin
                (Some((rowid, _, _)), Some(own)) if rowid != own => {
                    out.skipped += 1;
                    return Ok(());
                }
                _ => {}
            }
            write_row(
                conn,
                "known_hosts",
                existing,
                sid,
                m,
                &[
                    ("host", &d.host),
                    ("key_type", &d.key_type),
                    ("public_key", &d.public_key),
                    ("fingerprint", &d.fingerprint),
                ],
            )?;
        }
    }
    // The remote edit is newer than anything queued here
    drop_outbox(conn, sid)?;
    out.kinds.insert(table.kind);
    Ok(())
}

/// Two devices that trusted the same server key gave it different sync_ids.
/// Both keep the smaller id, whichever device decides: the other id is
/// queued for deletion, so the vault ends up with one item.
#[allow(clippy::too_many_arguments)]
fn merge_known_host(
    conn: &Connection,
    rowid: i64,
    local_id: &str,
    local_modified: i64,
    remote_id: &str,
    remote_modified: i64,
    d: &KnownHostData,
    out: &mut Applied,
) -> Result<(), rusqlite::Error> {
    let remote_newer = remote_modified > local_modified;
    let now = now_ms();
    if remote_id < local_id {
        // Take the remote id, and the newer of the two keys
        let modified_at = if remote_newer { remote_modified } else { local_modified.max(remote_modified + 1) };
        if remote_newer {
            conn.execute(
                "UPDATE known_hosts SET sync_id = ?1, public_key = ?2, fingerprint = ?3, modified_at = ?4 \
                 WHERE rowid = ?5",
                params![remote_id, d.public_key, d.fingerprint, modified_at, rowid],
            )?;
            out.kinds.insert("known_host");
        } else {
            conn.execute(
                "UPDATE known_hosts SET sync_id = ?1, modified_at = ?2 WHERE rowid = ?3",
                params![remote_id, modified_at, rowid],
            )?;
            queue(conn, remote_id, "known_host", false, modified_at)?;
        }
        drop_outbox(conn, local_id)?;
        queue(conn, local_id, "known_host", true, (local_modified + 1).max(now))?;
    } else {
        // Keep ours; the other device takes it when it pulls it
        if remote_newer {
            conn.execute(
                "UPDATE known_hosts SET public_key = ?1, fingerprint = ?2, modified_at = ?3 WHERE rowid = ?4",
                params![d.public_key, d.fingerprint, remote_modified, rowid],
            )?;
            queue(conn, local_id, "known_host", false, remote_modified)?;
            out.kinds.insert("known_host");
        }
        queue(conn, remote_id, "known_host", true, (remote_modified + 1).max(now))?;
    }
    Ok(())
}

/// Insert or update one row with the given columns, stamping sync_id and
/// modified_at. Setting modified_at keeps the change-tracking triggers out.
fn write_row(
    conn: &Connection,
    table: &str,
    existing: Option<i64>,
    sync_id: &str,
    modified_at: i64,
    cols: &[(&str, &dyn ToSql)],
) -> Result<i64, rusqlite::Error> {
    let n = cols.len();
    let mut values: Vec<&dyn ToSql> = cols.iter().map(|(_, v)| *v).collect();
    match existing {
        Some(rowid) => {
            let sets: Vec<String> =
                cols.iter().enumerate().map(|(i, (c, _))| format!("{c} = ?{}", i + 1)).collect();
            let sql = format!(
                "UPDATE {table} SET {}, modified_at = ?{} WHERE rowid = ?{}",
                sets.join(", "),
                n + 1,
                n + 2
            );
            values.push(&modified_at);
            values.push(&rowid);
            conn.execute(&sql, params_from_iter(values))?;
            Ok(rowid)
        }
        None => {
            let names: Vec<&str> = cols.iter().map(|(c, _)| *c).collect();
            let marks: Vec<String> = (1..=n + 2).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "INSERT INTO {table} ({}, sync_id, modified_at) VALUES ({})",
                names.join(", "),
                marks.join(", ")
            );
            values.push(&sync_id);
            values.push(&modified_at);
            conn.execute(&sql, params_from_iter(values))?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// (rowid, modified_at) of the row with this sync_id
fn find(conn: &Connection, table: &str, sync_id: &str) -> Result<Option<(i64, i64)>, rusqlite::Error> {
    conn.query_row(
        &format!("SELECT rowid, modified_at FROM {table} WHERE sync_id = ?1"),
        [sync_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// A delete names no kind: look in every table
fn locate(
    conn: &Connection,
    sync_id: &str,
) -> Result<Option<(&'static SyncedTable, i64, i64)>, rusqlite::Error> {
    for table in sync::SYNCED {
        if let Some((rowid, modified_at)) = find(conn, table.table, sync_id)? {
            return Ok(Some((table, rowid, modified_at)));
        }
    }
    Ok(None)
}

fn local_id(conn: &Connection, table: &str, sync_id: &str) -> Result<Option<i64>, rusqlite::Error> {
    Ok(find(conn, table, sync_id)?.map(|(rowid, _)| rowid))
}

/// A relation's local id; None when it names nothing here
fn resolve(conn: &Connection, table: &str, sync_id: Option<&str>) -> Result<Option<i64>, rusqlite::Error> {
    match sync_id {
        Some(s) => local_id(conn, table, s),
        None => Ok(None),
    }
}

fn drop_outbox(conn: &Connection, sync_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM sync_outbox WHERE sync_id = ?1", [sync_id])?;
    Ok(())
}

fn queue(conn: &Connection, sync_id: &str, kind: &str, deleted: bool, changed_at: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(sync_id) DO UPDATE SET kind = excluded.kind, deleted = excluded.deleted, \
         changed_at = excluded.changed_at",
        params![sync_id, kind, deleted, changed_at],
    )?;
    Ok(())
}

/// Queue every synced row, for a device joining a vault: whatever it holds
/// goes up. Rows the vault already has at the same version are refused by the
/// server without a new rev, so re-joining the same vault costs one upload.
pub fn queue_all(conn: &Connection) -> Result<(), rusqlite::Error> {
    for table in table_of_all() {
        conn.execute(
            &format!(
                "INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at) \
                 SELECT sync_id, ?1, 0, modified_at FROM {} WHERE sync_id IS NOT NULL \
                 ON CONFLICT(sync_id) DO NOTHING",
                table.table
            ),
            [table.kind],
        )?;
    }
    Ok(())
}

fn table_of_all() -> impl Iterator<Item = &'static SyncedTable> {
    ORDER.into_iter().filter_map(table_of)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashMap};

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

    /// A stand-in for push_items / the pull query: last write wins per id
    #[derive(Default)]
    struct Server(BTreeMap<String, (Option<Payload>, i64)>);

    impl Server {
        fn push(&mut self, items: &[Queued]) {
            for q in items {
                let newer = !matches!(self.0.get(&q.sync_id), Some((_, m)) if *m >= q.modified_at);
                if newer {
                    self.0.insert(q.sync_id.clone(), (q.payload.clone(), q.modified_at));
                }
            }
        }
        fn pull(&self) -> Vec<Incoming> {
            self.0
                .iter()
                .map(|(id, (p, m))| Incoming { sync_id: id.clone(), payload: p.clone(), modified_at: *m })
                .collect()
        }
    }

    struct Device {
        conn: Connection,
        secrets: MemSecrets,
    }

    impl Device {
        // In memory, and never through db::upsert_host / delete_host /
        // *_credential: those use the real OS keyring
        fn new() -> Self {
            let conn = Connection::open_in_memory().unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            db::migrate(&conn).unwrap();
            Device { conn, secrets: MemSecrets::default() }
        }

        fn sync_with(&self, server: &mut Server) -> Applied {
            let tx = self.conn.unchecked_transaction().unwrap();
            let applied = apply(&tx, server.pull()).unwrap();
            tx.commit().unwrap();
            applied.write_secrets(&self.secrets);
            let q = queued(&self.conn, &self.secrets).unwrap();
            server.push(&q);
            clear_pushed(&self.conn, &q).unwrap();
            applied
        }

        fn outbox(&self) -> Vec<(String, bool)> {
            self.conn
                .prepare("SELECT sync_id, deleted FROM sync_outbox ORDER BY sync_id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        }

        fn val<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
            self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
        }

        fn sid(&self, table: &str, rowid: i64) -> String {
            self.conn
                .query_row(&format!("SELECT sync_id FROM {table} WHERE rowid = ?1"), [rowid], |r| r.get(0))
                .unwrap()
        }

        fn add_host(&self, label: &str) -> i64 {
            self.conn
                .execute(
                    "INSERT INTO hosts (label, address, port, username, auth_method) \
                     VALUES (?1, ?1 || '.example', 22, 'root', 'password')",
                    [label],
                )
                .unwrap();
            self.conn.last_insert_rowid()
        }
    }

    /// Device A with one of everything, related the way the app relates them
    fn populated() -> (Device, BTreeMap<&'static str, i64>) {
        let a = Device::new();
        let c = &a.conn;
        let mut ids = BTreeMap::new();
        ids.insert("group", db::upsert_group(c, None, "prod").unwrap().id);
        c.execute(
            "INSERT INTO credentials (name, username, auth_method, password_saved) \
             VALUES ('ops', 'alice', 'password', 1)",
            [],
        )
        .unwrap();
        let cred = c.last_insert_rowid();
        a.secrets.set(&db::cred_password_entry(cred), "cred-pw");
        ids.insert("credential", cred);

        let bastion = a.add_host("bastion");
        c.execute("UPDATE hosts SET auth_method = 'key', key_path = '/k/id' WHERE id = ?1", [bastion])
            .unwrap();
        a.secrets.set(&db::keypass_entry(bastion), "key-pp");
        ids.insert("bastion", bastion);

        let web = a.add_host("web");
        c.execute(
            "UPDATE hosts SET group_id = ?2, credential_id = ?3, jump_host_id = ?4, password_saved = 1 \
             WHERE id = ?1",
            params![web, ids["group"], cred, bastion],
        )
        .unwrap();
        a.secrets.set(&db::password_entry(web), "web-pw");
        ids.insert("web", web);

        let note = db::create_note(c, "runbook", "restart #nginx").unwrap().id;
        db::set_note_host(c, note, Some(web)).unwrap();
        db::set_note_tags(c, note, &["nginx".into()]).unwrap();
        ids.insert("note", note);
        ids.insert(
            "forward",
            db::upsert_forward(c, None, web, "pg", 5432, "localhost", 5432, true).unwrap().id,
        );
        ids.insert("snippet", db::upsert_snippet(c, None, "df", "df -h").unwrap().id);
        db::remember_host_key(c, "web.example:22", "ssh-ed25519", "AAAA", "SHA256:x").unwrap();
        (a, ids)
    }

    #[test]
    fn items_survive_json_both_ways() {
        let (a, _) = populated();
        for q in queued(&a.conn, &a.secrets).unwrap() {
            let payload = q.payload.unwrap();
            let json = serde_json::to_string(&payload).unwrap();
            assert!(json.starts_with(&format!("{{\"kind\":\"{}\"", q.kind)), "{json}");
            assert_eq!(serde_json::from_str::<Payload>(&json).unwrap(), payload);
        }
        // Fields a later version adds are ignored, ones it drops default
        let p: Payload = serde_json::from_str(r#"{"kind":"snippet","name":"a","command":"b","new":1}"#).unwrap();
        assert_eq!(p, Payload::Snippet(SnippetData { name: "a".into(), command: "b".into(), sort: 0 }));
    }

    #[test]
    fn a_new_device_gets_everything_with_relations_and_secrets() {
        let (a, ids) = populated();
        let q = queued(&a.conn, &a.secrets).unwrap();
        let kinds: Vec<&str> = q.iter().map(|q| q.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["group", "credential", "host", "host", "snippet", "note", "port_forward", "known_host"]
        );

        // Hand the items over in reverse, so web arrives before the bastion it
        // jumps through
        let items: Vec<Incoming> = q
            .into_iter()
            .rev()
            .map(|q| Incoming { sync_id: q.sync_id, payload: q.payload, modified_at: q.modified_at })
            .collect();
        let b = Device::new();
        let tx = b.conn.unchecked_transaction().unwrap();
        let applied = apply(&tx, items.clone()).unwrap();
        tx.commit().unwrap();
        applied.write_secrets(&b.secrets);

        assert_eq!(applied.kinds.len(), 7);
        assert_eq!(applied.skipped, 0);
        assert!(b.outbox().is_empty(), "applying must not queue anything: {:?}", b.outbox());

        // Same identities and versions on both sides
        for table in ["groups", "credentials", "hosts", "snippets", "notes", "port_forwards", "known_hosts"] {
            let dump = |d: &Device| -> Vec<(String, i64)> {
                d.conn
                    .prepare(&format!("SELECT sync_id, modified_at FROM {table} ORDER BY sync_id"))
                    .unwrap()
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .unwrap()
                    .collect::<Result<_, _>>()
                    .unwrap()
            };
            assert_eq!(dump(&a), dump(&b), "{table}");
        }

        // Relations point at the right rows, by whatever ids B gave them
        let web = b.val::<i64>(&format!("SELECT id FROM hosts WHERE sync_id = '{}'", a.sid("hosts", ids["web"])));
        let (group, cred, jump): (String, String, String) = b
            .conn
            .query_row(
                "SELECT g.name, c.name, j.label FROM hosts h JOIN groups g ON g.id = h.group_id \
                 JOIN credentials c ON c.id = h.credential_id JOIN hosts j ON j.id = h.jump_host_id \
                 WHERE h.id = ?1",
                [web],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!((group.as_str(), cred.as_str(), jump.as_str()), ("prod", "ops", "bastion"));
        assert_eq!(b.val::<i64>("SELECT host_id FROM notes"), web);
        assert_eq!(b.val::<String>("SELECT tags FROM notes"), r#"["nginx"]"#);
        assert_eq!(b.val::<i64>("SELECT host_id FROM port_forwards"), web);
        assert_eq!(b.val::<String>("SELECT public_key FROM known_hosts"), "AAAA");

        // Secrets land in B's keyring under B's ids
        let bastion = b.val::<i64>("SELECT id FROM hosts WHERE label = 'bastion'");
        let cred = b.val::<i64>("SELECT id FROM credentials");
        assert_eq!(b.secrets.get(&db::password_entry(web)).as_deref(), Some("web-pw"));
        assert_eq!(b.secrets.get(&db::keypass_entry(bastion)).as_deref(), Some("key-pp"));
        assert_eq!(b.secrets.get(&db::cred_password_entry(cred)).as_deref(), Some("cred-pw"));
        assert_eq!(b.secrets.0.borrow().len(), 3);

        // The same pull again (our own push coming back) changes nothing
        let again = apply(&b.conn, items).unwrap();
        assert!(again.kinds.is_empty());
        assert!(b.outbox().is_empty());
    }

    #[test]
    fn the_newer_edit_wins_either_way() {
        let a = Device::new();
        let b = Device::new();
        let mut server = Server::default();
        let host = a.add_host("web");
        a.sync_with(&mut server);
        b.sync_with(&mut server);
        let sid = a.sid("hosts", host);
        let on_b = b.val::<i64>(&format!("SELECT id FROM hosts WHERE sync_id = '{sid}'"));

        // A edits first, B later: B's edit is newer and stays queued over A's
        a.conn.execute("UPDATE hosts SET label = 'from-a' WHERE id = ?1", [host]).unwrap();
        b.conn.execute("UPDATE hosts SET modified_at = modified_at + 5000, label = 'from-b' WHERE id = ?1", [on_b]).unwrap();
        b.conn.execute(
            "INSERT INTO sync_outbox (sync_id, kind, deleted, changed_at) \
             SELECT sync_id, 'host', 0, modified_at FROM hosts WHERE id = ?1",
            [on_b],
        )
        .unwrap();
        a.sync_with(&mut server);
        let applied = b.sync_with(&mut server);
        assert!(applied.kinds.is_empty(), "A's older edit must not land on B");
        assert_eq!(b.val::<String>("SELECT label FROM hosts"), "from-b");

        // ... and A takes B's
        let applied = a.sync_with(&mut server);
        assert!(applied.kinds.contains("host"));
        assert_eq!(a.val::<String>("SELECT label FROM hosts"), "from-b");
        assert!(a.outbox().is_empty() && b.outbox().is_empty());
    }

    #[test]
    fn deletes_travel_without_echoes() {
        let (a, ids) = populated();
        let b = Device::new();
        let mut server = Server::default();
        a.sync_with(&mut server);
        b.sync_with(&mut server);

        // Deleting the host cascades: its forward goes, the note is unlinked
        a.conn.execute("DELETE FROM hosts WHERE id = ?1", [ids["web"]]).unwrap();
        a.sync_with(&mut server);
        let applied = b.sync_with(&mut server);

        assert!(applied.kinds.contains("host") && applied.kinds.contains("port_forward"));
        assert_eq!(b.val::<i64>("SELECT count(*) FROM hosts"), 1);
        assert_eq!(b.val::<i64>("SELECT count(*) FROM port_forwards"), 0);
        assert_eq!(b.val::<Option<i64>>("SELECT host_id FROM notes"), None);
        assert!(b.outbox().is_empty(), "B must not send A's cascade back: {:?}", b.outbox());
        // web's password and passphrase went with it; the bastion's and the
        // credential's stay
        assert_eq!(b.secrets.0.borrow().len(), 2);

        // Nothing left to say on either side
        assert!(a.sync_with(&mut server).kinds.is_empty());
        assert!(b.sync_with(&mut server).kinds.is_empty());
    }

    #[test]
    fn a_newer_edit_outlives_an_older_delete_and_back() {
        let a = Device::new();
        let b = Device::new();
        let mut server = Server::default();
        let snip = db::upsert_snippet(&a.conn, None, "df", "df -h").unwrap().id;
        a.sync_with(&mut server);
        b.sync_with(&mut server);
        let on_b = b.val::<i64>("SELECT id FROM snippets");

        // B deletes, then A edits later: the edit wins and B gets it back
        b.conn.execute("DELETE FROM snippets WHERE id = ?1", [on_b]).unwrap();
        let deleted_at: i64 = b.val("SELECT changed_at FROM sync_outbox");
        a.conn
            .execute(
                "UPDATE snippets SET command = 'df -hT', modified_at = ?2 WHERE id = ?1",
                params![snip, deleted_at + 1000],
            )
            .unwrap();
        a.conn
            .execute("INSERT INTO sync_outbox SELECT sync_id, 'snippet', 0, modified_at FROM snippets", [])
            .unwrap();
        a.sync_with(&mut server);
        b.sync_with(&mut server);
        assert_eq!(b.val::<String>("SELECT command FROM snippets"), "df -hT");
        assert!(b.outbox().is_empty());

        // B deletes again, now newest: A loses it
        b.conn.execute("DELETE FROM snippets", []).unwrap();
        b.sync_with(&mut server);
        a.sync_with(&mut server);
        assert_eq!(a.val::<i64>("SELECT count(*) FROM snippets"), 0);
        assert!(a.outbox().is_empty());
    }

    #[test]
    fn an_older_remote_edit_does_not_undelete() {
        let b = Device::new();
        let snip = db::upsert_snippet(&b.conn, None, "df", "df -h").unwrap().id;
        let sid = b.sid("snippets", snip);
        let created: i64 = b.val("SELECT modified_at FROM snippets");
        b.conn.execute("DELETE FROM snippets", []).unwrap();

        let stale = Incoming {
            sync_id: sid.clone(),
            payload: Some(Payload::Snippet(SnippetData { name: "df".into(), command: "x".into(), sort: 0 })),
            modified_at: created,
        };
        assert!(apply(&b.conn, vec![stale]).unwrap().kinds.is_empty());
        assert_eq!(b.val::<i64>("SELECT count(*) FROM snippets"), 0);
        assert_eq!(b.outbox(), [(sid, true)]);
    }

    #[test]
    fn a_host_key_trusted_on_two_devices_ends_up_as_one_item() {
        for concurrent in [false, true] {
            let a = Device::new();
            let b = Device::new();
            let mut server = Server::default();
            for d in [&a, &b] {
                db::remember_host_key(&d.conn, "web:22", "ssh-ed25519", "AAAA", "SHA256:x").unwrap();
            }
            let winner = a.sid("known_hosts", 1).min(b.sid("known_hosts", 1));

            if concurrent {
                // Both push before either pulls
                for d in [&a, &b] {
                    let q = queued(&d.conn, &d.secrets).unwrap();
                    server.push(&q);
                    clear_pushed(&d.conn, &q).unwrap();
                }
            }
            for _ in 0..3 {
                a.sync_with(&mut server);
                b.sync_with(&mut server);
            }

            for d in [&a, &b] {
                assert_eq!(d.val::<i64>("SELECT count(*) FROM known_hosts"), 1, "concurrent={concurrent}");
                assert_eq!(d.sid("known_hosts", 1), winner, "concurrent={concurrent}");
                assert!(d.outbox().is_empty());
            }
            let live: Vec<&String> =
                server.0.iter().filter(|(_, (p, _))| p.is_some()).map(|(id, _)| id).collect();
            assert_eq!(live, [&winner], "concurrent={concurrent}");
        }
    }

    #[test]
    fn a_forward_whose_host_is_missing_is_skipped() {
        let b = Device::new();
        let item = Incoming {
            sync_id: "0b6f5f53-1111-4222-8333-444455556666".into(),
            payload: Some(Payload::PortForward(PortForwardData {
                host: "0b6f5f53-0000-4222-8333-444455556666".into(),
                name: "pg".into(),
                local_port: 5432,
                remote_host: "localhost".into(),
                remote_port: 5432,
                auto_start: false,
            })),
            modified_at: 1,
        };
        let applied = apply(&b.conn, vec![item]).unwrap();
        assert_eq!(applied.skipped, 1);
        assert_eq!(b.val::<i64>("SELECT count(*) FROM port_forwards"), 0);
    }

    #[test]
    fn an_edit_made_during_a_push_stays_queued() {
        let a = Device::new();
        let host = a.add_host("web");
        let q = queued(&a.conn, &a.secrets).unwrap();
        a.conn.execute("UPDATE hosts SET label = 'web-2' WHERE id = ?1", [host]).unwrap();
        clear_pushed(&a.conn, &q).unwrap();
        assert_eq!(a.outbox().len(), 1);
    }

    #[test]
    fn joining_a_vault_queues_everything_once() {
        let (a, _) = populated();
        a.conn.execute("DELETE FROM sync_outbox", []).unwrap();
        queue_all(&a.conn).unwrap();
        queue_all(&a.conn).unwrap();
        assert_eq!(a.outbox().len(), 8);
        assert!(a.outbox().iter().all(|(_, deleted)| !deleted));
    }
}
