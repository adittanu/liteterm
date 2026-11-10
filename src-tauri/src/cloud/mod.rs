//! Optional account and cloud sync, end-to-end encrypted.
//!
//! - `vault`: the encryption (master password, vault key, recovery key)
//! - `items`: synced rows as items, both ways
//! - `api`: the Supabase calls
//! - `engine`: the account states and the background sync loop

pub mod api;
pub mod engine;
pub mod items;
pub mod vault;
