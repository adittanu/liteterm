# Shared team vaults: tasks

- [x] 1. Crypto: account keys and team keys
  - Add `x25519-dalek` (static secrets). New `cloud/team_crypto.rs`: make an
    account key pair, seal/open the private key with the personal vault key,
    sealed box for team keys, team item encrypt/decrypt with the team AAD,
    safety code.
  - Unit tests: round trips, wrong recipient, item moved between teams fails.
  - _Requirements: 1.1, 1.2, 2.3, 6.3_

- [x] 2. Server migration
  - `supabase/migrations/2026111x_team_vaults.sql`: tables, RLS, RPCs and
    limits from the design, tried in a rolled-back transaction on the live
    project, then applied with `supabase db query --linked -f`.
  - _Requirements: 2.1, 2.5, 3.1–3.4, 5.1, 7.1_

- [x] 3. API client
  - `cloud/api.rs`: account key, team, invite, member, item and rotation calls,
    with the existing error mapping.
  - _Requirements: 1, 2, 5, 6_

- [x] 4. Local schema v5
  - `teams` table, `team_id` on synced tables and on `sync_outbox`, triggers
    that carry it, cross-vault relation checks in the db writers.
  - Migration test from a v4 database.
  - _Requirements: 4.3, 6.1_

- [x] 5. Sync per vault
  - `items.rs`: a `Vault` parameter for `queued`, `apply`, `queue_all`.
  - `engine.rs`: account key on unlock, membership refresh, team pull/push,
    leaving a team cleans up locally.
  - Device simulation tests: two members, viewer refused, removal cleanup.
  - _Requirements: 4.4, 5.3, 6.1, 6.2, 7.2_

- [ ] 6. Team management commands and Settings → Teams
  - Create, invite, accept, admit with safety code, roles, remove, leave,
    delete. Rotation on removal, with the "change these passwords" list.
  - _Requirements: 1, 2, 3, 5_

- [ ] 7. Team items in the app
  - Team sections in the hosts view, "Move to team…", vault-aware pickers,
    read-only state for viewers.
  - _Requirements: 4.1–4.6_

- [ ] 7b. Embedded private keys
  - `credential_keys` table sealed with a per-device key, the key in the
    credential payload, `AuthSpec::Key` from embedded text, "Store the key in
    LiteTerm" in the credential dialog (always on for team credentials).
  - _Requirements: 7.3_

- [ ] 8. Docs and landing
  - README (how teams work, what the server sees, the safety code), FAQ,
    Compare row "Shared team vaults".
