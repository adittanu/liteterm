//! Store module: `models` (serde types), `db` (SQLite access), `sync`
//! (change tracking for cloud sync) and `vaults` (personal vs team rows).
pub mod db;
pub mod models;
pub mod sync;
pub mod vaults;
