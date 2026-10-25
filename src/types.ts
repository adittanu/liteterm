// Shared types between frontend and backend (mirrors Rust models via serde)

export type AuthMethod = 'password' | 'key' | 'agent'

export interface HostGroup {
  id: number
  name: string
  sort: number
}

export interface Host {
  id: number
  group_id: number | null
  label: string
  address: string
  port: number
  username: string
  auth_method: AuthMethod
  password_saved: boolean
  has_password: boolean // server-computed: a password exists in keyring
  key_path: string | null
  credential_id: number | null
  credential_name: string | null
  os: string | null // detected distro id ("ubuntu", "debian", "macos", …)
  sort: number
  last_used_at: number | null // ms since epoch, bumped on connect
  jump_host_id: number | null // saved host to hop through (ProxyJump)
}

export interface HostInput {
  id?: number
  group_id: number | null
  label: string
  address: string
  port: number
  username: string
  auth_method: AuthMethod
  password?: string | null // only sent when saving; never returned by backend
  save_password?: boolean
  key_path?: string | null
  key_passphrase?: string | null
  credential_id?: number | null
  create_credential?: CredentialInput | null
  // Required on purpose: the backend reads a missing value as "no jump
  // host", so every save must carry the current one forward
  jump_host_id: number | null
}

export interface Credential {
  id: number
  name: string
  username: string
  auth_method: AuthMethod
  password_saved: boolean
  has_password: boolean
  key_path: string | null
}

export interface CredentialInput {
  name: string
  username: string
  auth_method: AuthMethod
  password?: string | null
  save_password?: boolean
  key_path?: string | null
  key_passphrase?: string | null
}

export interface Snippet {
  id: number
  name: string
  command: string
  sort: number
}

export interface Note {
  id: number
  title: string
  body: string
  pinned: boolean
  tags: string[]
  created_at: string
  updated_at: string
  host_id: number | null // host this note belongs to
}

export interface PortForward {
  id: number
  host_id: number
  name: string
  local_port: number
  remote_host: string
  remote_port: number
  auto_start: boolean
}

export type SessionStatus =
  | 'connecting'
  | 'hostkey'
  | 'connected'
  | 'reconnecting' // dropped; the backend redials with backoff on the same session
  | 'error'
  | 'closed'
export type SessionKind = 'ssh' | 'local' | 'herdr'

export interface SessionTab {
  id: number // session id (backend)
  kind: SessionKind
  hostId: number | null
  label: string
  address: string
  status: SessionStatus
  error?: string
  hostKeyFingerprint?: string // present while status === 'hostkey'
  hostKeyAddress?: string // addr:port being verified — a jump host's, not always the target's
  // present while status === 'reconnecting'
  reconnect?: { attempt: number; maxAttempts: number; retryAt: number /* epoch ms; 0 = dialing */ }
  sftpOpen: boolean
}

export interface FileEntry {
  name: string
  kind: 'file' | 'dir' | 'symlink' | 'other'
  size: number
  mtime: number
  permissions: number | null
}

export interface Transfer {
  id: number
  sessionId: number
  kind: 'upload' | 'download'
  fileName: string
  size: number
  transferred: number
  done: boolean
  error?: string
}

export interface AppSettings {
  theme: string // theme id: 'system' | 'dark' | 'light' | 'catppuccin-mocha' | ...
  terminalFontSize: number
  confirmDeleteFile: boolean
  autoSudoPassword: boolean
  autoReconnect: boolean // redial dropped SSH sessions with backoff
  keybindings: Record<string, string> // action id -> combo ("Ctrl+K"); absent = default
  hostsLayout: 'modern' | 'classic' // Hosts page: chips + quick actions, or Termius-style folders
}

export interface HostKeyEvent {
  sessionId: number
  fingerprint: string
  keyType: string
  address: string
}

export interface SessionLog {
  id: number
  host_id: number | null
  label: string
  username: string
  address: string
  started_at: string
  ended_at: string | null
}

export interface KnownHostRow {
  host: string
  key_type: string
  public_key: string
  fingerprint: string
  added_at: string
}

export interface ParsedHost {
  alias: string
  hostname: string
  port: number
  user: string | null
  identity_file: string | null
  group: string | null
  exists: boolean
  password: string | null
  key_pem: string | null
  key_passphrase: string | null
}

export interface ImportPreview {
  config_found: boolean
  hosts: ParsedHost[]
  known_host_count: number
}

export interface FileImportPreview {
  format: 'liteterm' | 'termius' | 'ssh-config'
  hosts: ParsedHost[]
}

export interface ImportResult {
  host_ids: number[]
  known_imported: number
}

export interface ExportResult {
  hostCount: number
  groupCount: number
  path: string
}

// ---- self-update (GitHub Releases) ----
export interface UpdateAsset {
  name: string
  size: number
  url: string
}

export interface UpdateInfo {
  available: boolean
  currentVersion: string
  latestVersion: string
  releaseUrl: string | null
  notes: string | null
  asset: UpdateAsset | null
}
