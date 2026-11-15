//! Store module: `models` (serde types), `db` (SQLite access), `sync`
//! (change tracking for cloud sync), `vaults` (personal vs team rows) and
//! `stored_keys` (private keys kept in LiteTerm).
pub mod db;
pub mod models;
pub mod stored_keys;
pub mod sync;
pub mod vaults;
