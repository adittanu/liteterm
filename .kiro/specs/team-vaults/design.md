# Shared team vaults: design

## Overview

A team vault is a second vault next to the personal one, with the same item
format, the same last-write-wins rule and the same outbox. What is new:

- an **account key** per user, so a team key can be wrapped for a person
  without the server learning it;
- **team tables** on Supabase, with access decided by membership;
- a **`team_id` column** on local synced rows, so the engine knows which vault
  each row (and each tombstone) belongs to;
- an engine that syncs **several vaults**, each with its own key and cursor.

Personal sync does not change on the wire, so older versions keep working.

## Key hierarchy

```
master password ──Argon2id──► KEK ──wraps──► personal vault key (unchanged)
                                                 │
                                                 └─encrypts─► account private key (X25519)
                                                                  │
        team key (32 random bytes, per epoch) ◄──sealed to──── account public key
                │
                └─encrypts─► team items (XChaCha20-Poly1305)
```

- **Account key**: X25519 (`x25519-dalek`). Stored on the server as
  `public_key` and `private_key_sealed = seal(personal vault key, private key,
  AAD "liteterm:account-key:{user_id}")`. Made on first use; recovery and
  password changes leave it alone because they keep the vault key.
- **Wrapping a team key for a member** (sealed box): ephemeral X25519 key pair,
  shared secret with the member's public key, HKDF-SHA256 (info
  `liteterm:team-key:v1`) to a 32-byte key, XChaCha20-Poly1305 with AAD
  `liteterm:team-key:{team_id}:{epoch}:{member user_id}`. Stored:
  `ephemeral public key || envelope`.
- **Team items**: the existing envelope (`vault.rs` `seal`), AAD
  `liteterm:team-item:{team_id}:{sync_id}`. Unlike personal items, the AAD
  names the vault, so the server can't replay an item into another team.
- **Safety code**: SHA-256 of the public key, first 60 bits as 12 digits in
  groups of four (`4821 0937 6612`). Shown on the member's Account page and in
  the admit dialog.

Why the confirmation step: the server hands out public keys. Without the code
check, a hostile server could swap in its own key at admit time and read the
vault. With it, an attack needs the owner to skip the check.

## Server (new migration `supabase/migrations/2026111x_team_vaults.sql`)

```sql
account_keys (user_id pk → auth.users, public_key text, private_key_sealed text, created_at)
teams        (id uuid pk, name text, rev bigint default 0, epoch int default 1,
              created_by uuid, created_at)
team_members (team_id → teams on delete cascade, user_id → auth.users,
              role text check in ('owner','admin','editor','viewer'),
              epoch int, wrapped_team_key text, added_by uuid, added_at,
              primary key (team_id, user_id))
team_invites (id uuid pk, team_id → teams on delete cascade, email citext,
              role text, invited_by uuid, created_at, expires_at,
              accepted_by uuid null, accepted_public_key text null)
team_items   (team_id → teams on delete cascade, id uuid, data text, deleted bool,
              modified_at bigint, rev bigint, epoch int,
              primary key (team_id, id))
```

All writes go through `security definer` RPCs that check the caller's role
first. RLS gives members `select` on their teams, members, items and keys;
the invitee `select` on invites addressed to their email
(`auth.jwt()->>'email'`).

| RPC | Who | Does |
|---|---|---|
| `put_account_key(public_key, private_key_sealed)` | anyone | first key only; never overwritten |
| `create_team(name, wrapped_team_key)` | anyone | team + owner row, epoch 1 |
| `invite_member(team_id, email, role)` | owner, admin | invite row, 7-day expiry |
| `accept_invite(invite_id)` | invitee | attaches the caller's public key |
| `admit_member(invite_id, wrapped_team_key)` | owner, admin | member row at the current epoch, deletes the invite |
| `remove_member(team_id, user_id)` | owner, admin (owner for admins) | deletes the row; refuses the last owner |
| `set_role(team_id, user_id, role)` | owner | |
| `leave_team(team_id)` | member | refuses the last owner |
| `push_team_items(team_id, items)` | owner, admin, editor | as `push_items`, rev per team, records `epoch` |
| `rotate_team_key(team_id, new_epoch, wrapped_keys jsonb, items jsonb)` | owner, admin | in one transaction: bumps epoch, replaces every member's wrapped key, rewrites items in pages (called repeatedly with `items`, epoch bump on the first call) |
| `delete_team(team_id)` | owner | cascades |

Pull is a plain `GET /rest/v1/team_items?team_id=eq.{id}&rev=gt.{n}&order=rev`.
Membership listing: `GET /rest/v1/team_members?select=team_id,role,epoch,wrapped_team_key,teams(name,rev,epoch)`.

Limits (50 members, 5,000 items) are checked in the RPCs.

## Local database (schema v5)

- New table `teams (id TEXT pk, name, role, epoch, cursor INTEGER, joined_at)`:
  the teams this account belongs to, refreshed from the server each sync.
- `team_id TEXT NULL` on `groups`, `hosts`, `credentials`, `snippets`,
  `notes`, `port_forwards` (NULL = personal). `known_hosts` stay personal in
  the first version: each member trusts a host key once.
- `sync_outbox` gains `team_id TEXT NULL`. The insert/update triggers copy
  `NEW.team_id`; the delete trigger copies `OLD.team_id`, so a tombstone still
  knows its vault after the row is gone.
- `team_id` is not a content column: moving a row between vaults is done as
  "copy under a new sync_id, delete the original", never by changing
  `team_id` in place (the personal tombstone would otherwise delete the moved
  row on the other devices).
- A check in `upsert_host` (and the credential/group/forward/note writers)
  refuses relations across vaults.

## Engine

`Cloud` keeps `personal: VaultKey` as today plus `teams: HashMap<team_id,
TeamState { key per epoch, role, cursor }>`.

`sync_once` becomes:

1. Personal vault, exactly as today, but `items::queued` and `queue_all` only
   take rows with `team_id IS NULL`.
2. Refresh membership. For each team: unwrap the team key(s) with the account
   private key, update the local `teams` row.
3. For each team: pull since its cursor, decrypt with the key for the item's
   epoch, apply with `team_id` stamped on inserted rows; push its outbox
   entries (viewers skip the push and drop local edits, which the UI prevents
   anyway).
4. Teams this account is no longer in: delete their rows and keyring entries
   locally, drop the `teams` row.

`items.rs` gets a `Vault` parameter (`Personal | Team(id)`) for `queued`,
`apply` and `queue_all`; relation resolution looks only inside the same vault.

## UI

- **Settings → Teams** (next to Account & sync): create team, members list with
  role and safety code, invite by email, pending invites, admit dialog with the
  code check, remove/leave, delete team. Your own safety code is shown here.
- **Hosts view**: a section per team (name, role badge), its groups inside.
  "Move to team…" on a personal host or group. Read-only state for viewers.
- **Pickers** (credential, group, jump host) filter to the host's vault.
- Toast after a removal lists the passwords to change.

## Testing

- Unit: sealed box round trip, wrong recipient, AAD binding (item moved
  between teams fails), safety code stability.
- `items.rs` device simulations, as the existing ones: two members, edits both
  ways, viewer push refused, member removed → rows and secrets deleted,
  rotation → old epoch readable until rewritten.
- The SQL can't run here (no Postgres or Supabase CLI on this machine). The
  migration gets a manual test checklist to run against the Supabase project.

## Risks

- **Rotation of a large team** rewrites every item; done in pages, resumable
  by epoch (items with an old epoch are the ones left to rewrite).
- **Server cost**: team data lives on the same Supabase project as personal
  sync. 50 members × 5,000 items is well within the free tier's 500 MB; the
  limits are there to keep it so.
- **Key files** can't be shared (paths differ per computer). Members need the
  same key file or an agent; embedding keys in the vault is a later step.
