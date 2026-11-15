# Shared team vaults: requirements

## Introduction

Today each LiteTerm account has one end-to-end encrypted vault that syncs
between that person's computers (`supabase/migrations/20261110000000_vault_sync.sql`,
`src-tauri/src/cloud/`). A team vault is a second kind of vault that several
accounts share: hosts, groups, credentials (with their saved passwords),
snippets, notes and port forwards that every member sees and can connect with.

It stays end-to-end encrypted: the server stores ciphertext and membership,
never a host, a password or a key it can read.

## Glossary

- **Personal vault**: the vault an account has today.
- **Team**: a named group of accounts with one team vault.
- **Team key**: the 32-byte key that encrypts a team's items. Each member gets
  a copy wrapped for their account key.
- **Account key**: an X25519 key pair per account. The private half is
  encrypted with the personal vault key, so every computer of that account
  can use it once the personal vault is unlocked.
- **Key epoch**: which team key is current. Removing a member makes a new one.
- **Safety code**: a short code derived from an account's public key, compared
  out of band to make sure the server did not swap keys.

## Requirements

### 1. Creating a team

**User story:** As a LiteTerm user with sync on, I want to create a team, so
that I can share servers with my colleagues.

1. WHEN a user with an unlocked personal vault creates a team with a name THEN
   the app SHALL make a team key, wrap it for the user's account key, and make
   the user the team's owner.
2. IF the user has no account key yet THEN the app SHALL make one and store it,
   encrypted with the personal vault key, before creating the team.
3. IF the personal vault is not set up or is locked THEN the app SHALL refuse
   and say that team vaults need sync with a master password.

### 2. Inviting and admitting members

**User story:** As a team owner or admin, I want to invite a colleague by
email, so that they get the team's hosts without me sending passwords around.

1. WHEN an owner or admin invites an email address with a role THEN the server
   SHALL record a pending invite that only that email's account can see.
2. WHEN the invited person signs in and accepts THEN the app SHALL attach their
   account public key to the invite.
3. WHEN an owner or admin opens an accepted invite THEN the app SHALL show the
   new member's safety code, and SHALL wrap the team key for them only after
   the owner confirms it matches the code the member reads out.
4. The member SHALL see the team's items only after step 3.
5. An invite SHALL expire after 7 days if not accepted.

### 3. Roles

1. A team SHALL have exactly the roles **owner**, **admin**, **editor** and
   **viewer**.
2. Owners and admins SHALL invite, admit and remove members. Only an owner
   SHALL change roles of admins or delete the team.
3. Editors SHALL add, change and delete items. Viewers SHALL only read and
   connect; the server SHALL refuse their pushes.
4. A team SHALL always keep at least one owner.

Note: every member can read every saved password in the vault, because the
app has to decrypt it to connect. A viewer is read-only, not secret-free.

### 4. Using team items

**User story:** As a member, I want team hosts next to my own, so that
connecting to them works the same way.

1. The hosts view SHALL show each team as its own section, with its groups.
2. Connecting, SFTP, port forwards, snippets and notes SHALL work on team items
   as they do on personal ones.
3. A team host SHALL only point at a credential, group or jump host of the same
   team. The pickers SHALL offer only those.
4. Saved passwords and key passphrases of team items SHALL land in the OS
   keyring of each member's computer, as personal ones do.
5. WHEN a user moves a personal host into a team THEN the app SHALL take its
   credential along (asking first), and the host SHALL leave the personal vault.
6. Viewers SHALL see team items as read-only in the UI.

### 5. Removing a member and rotating the key

1. WHEN an owner or admin removes a member THEN the server SHALL delete the
   member's access, and the app SHALL make a new team key (new epoch),
   re-encrypt every team item with it, and wrap it for the remaining members.
2. The app SHALL then tell the owner which saved passwords the removed member
   could read, so they can be changed on the servers.
3. WHEN a member leaves a team, or is removed, THEN that member's computers
   SHALL delete the team's items, and their keyring entries, on the next sync.

### 6. Sync behaviour

1. Each team vault SHALL sync on its own cursor, next to the personal vault.
2. A conflict SHALL keep the newer edit, as personal sync does.
3. Team item ciphertext SHALL be bound to its team and item id, so the server
   cannot move an item between vaults.
4. An item encrypted with an older epoch than the current one SHALL still be
   readable during a rotation, and SHALL be re-encrypted by the rotation.

### 7. Limits and compatibility

1. A team SHALL have at most 50 members and 5,000 items.
2. A LiteTerm older than this feature SHALL keep syncing its personal vault and
   SHALL never see team items.
3. Key files: a team credential that uses a private key SHALL carry the key
   itself in the vault, so every member can connect without copying files.
   WHEN a key file is chosen for a team credential THEN the app SHALL read the
   key, ask for its passphrase if it has one, and store it in the vault; the
   path SHALL NOT be used on other computers.
4. Team vaults SHALL be free, with no paid plan; the limits in 7.1 keep the
   server within the free tier.
5. Joining or creating a team SHALL require sync with a master password.
