// Shared types between frontend and backend (mirrors Rust models via serde)

export type AuthMethod = 'password' | 'key'

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
  sort: number
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

export interface PortForward {
  id: number
  host_id: number
  name: string
  local_port: number
  remote_host: string
  remote_port: number
  auto_start: boolean
}

export type SessionStatus = 'connecting' | 'hostkey' | 'connected' | 'error' | 'closed'
export type SessionKind = 'ssh' | 'local'

export interface SessionTab {
  id: number // session id (backend)
  kind: SessionKind
  hostId: number | null
  label: string
  address: string
  status: SessionStatus
  error?: string
  hostKeyFingerprint?: string // present while status === 'hostkey'
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
}

export interface HostKeyEvent {
  sessionId: number
  fingerprint: string
  keyType: string
  address: string
}

export interface ParsedHost {
  alias: string
  hostname: string
  port: number
  user: string | null
  identity_file: string | null
  group: string | null
  exists: boolean
}

export interface ImportPreview {
  config_found: boolean
  hosts: ParsedHost[]
  known_host_count: number
}

export interface ImportResult {
  host_ids: number[]
  known_imported: number
}
