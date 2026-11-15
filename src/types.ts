// Shared types between frontend and backend (mirrors Rust models via serde)

export type AuthMethod = 'password' | 'key' | 'agent'

export interface HostGroup {
  id: number
  name: string
  sort: number
  team_id: string | null // the team vault it's in; null = personal
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
  startup_command: string | null // typed into the shell after every login
  agent_forwarding: boolean // ssh -A: the server may use this computer's agent
  protocol: HostProtocol
  baud_rate: number | null // serial hosts; their device (COM3, /dev/ttyUSB0) is `address`
  team_id: string | null // the team vault it's in; null = personal
}

export type HostProtocol = 'ssh' | 'telnet' | 'serial'

export interface SerialPortEntry {
  name: string
  description: string | null
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
  // Required for the same reason: missing reads as "no startup command"
  startup_command: string | null
  // Optional: left out, the backend keeps what the host has
  agent_forwarding?: boolean
  // Optional the same way; when protocol is sent, baud_rate is written with it
  protocol?: HostProtocol
  baud_rate?: number | null
  // The vault a new host (and the credential it creates) goes in; null =
  // personal. Ignored on an update: moving is its own call.
  team_id?: string | null
}

export interface Credential {
  id: number
  name: string
  username: string
  auth_method: AuthMethod
  password_saved: boolean
  has_password: boolean
  key_path: string | null
  team_id: string | null
}

export interface CredentialInput {
  name: string
  username: string
  auth_method: AuthMethod
  password?: string | null
  save_password?: boolean
  key_path?: string | null
  key_passphrase?: string | null
  team_id?: string | null // the vault a new credential goes in; ignored on an update
}

export interface KeyDefaults {
  name: string // a file name in ~/.ssh not taken yet
  comment: string // user@computer
  dir: string // ~/.ssh, spelled out
}

export interface GeneratedKey {
  path: string
  publicKey: string // the authorized_keys line
  fingerprint: string // SHA256:…
}

export interface Snippet {
  id: number
  name: string
  command: string
  sort: number
  team_id: string | null
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
  sort: number // position in the manual order (lower = higher)
  archived: boolean // hidden from All Notes; listed under Archive
  team_id: string | null
}

/**
 * local (-L): listen on this computer's local_port, reach remote_host:remote_port from the server.
 * remote (-R): the server listens on remote_port, connections reach remote_host:local_port from this computer.
 * dynamic (-D): a SOCKS proxy on this computer's local_port; remote_host and remote_port are unused.
 */
export type ForwardKind = 'local' | 'remote' | 'dynamic'

export interface PortForward {
  id: number
  host_id: number
  name: string
  kind: ForwardKind
  local_port: number
  remote_host: string
  remote_port: number
  auto_start: boolean
  team_id: string | null // always its host's vault
}

export type SessionStatus =
  | 'connecting'
  | 'hostkey'
  | 'connected'
  | 'reconnecting' // dropped; the backend redials with backoff on the same session
  | 'error'
  | 'closed'
export type SessionKind = 'ssh' | 'local' | 'herdr' | 'telnet' | 'serial'

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
  /** Window title the shell or program set (OSC 0/2), shown after the label */
  title?: string
  /** A command typed at the prompt is still running (TerminalPane decides) */
  busy?: boolean
  /** Second pane of another tab's split: not a tab of its own in the top bar */
  parentId?: number
  /** This tab shows a second pane beside (right) or below (down) its own */
  split?: { childId: number; dir: SplitDir; ratio: number }
}

export type SplitDir = 'right' | 'down'

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
  /** Folders: files copied so far, of how many, and entries left out (links to folders, devices) */
  filesDone?: number
  filesTotal?: number
  skipped?: number
}

export interface AppSettings {
  theme: string // theme id: 'system' | 'dark' | 'light' | 'catppuccin-mocha' | ...
  terminalFontSize: number
  confirmDeleteFile: boolean
  autoSudoPassword: boolean
  autoReconnect: boolean // redial dropped SSH sessions with backoff
  notifyCommandDone: boolean // notice when a long command ends in a tab you're not looking at
  keepAwake: boolean // hold off idle sleep while a tab runs a command
  autosuggest: boolean // grey suggestions from the host's history while typing
  autosuggestImport: boolean // seed them from the server's own shell history once
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
  /** "agent" | "key" | "password" as a LiteTerm export declares it */
  auth_method?: string | null
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
  /** SHA-256 GitHub published for the asset; the download is checked against it */
  sha256?: string | null
}

export interface UpdateInfo {
  available: boolean
  currentVersion: string
  latestVersion: string
  releaseUrl: string | null
  notes: string | null
  asset: UpdateAsset | null
  /** The app can download and install the update itself; when false an update means opening the release page */
  autoInstall: boolean
  /** installer (Windows), app (macOS), appimage, package (deb/rpm: asks for a password), page (release page) */
  installMethod: 'installer' | 'app' | 'appimage' | 'package' | 'page'
}

// ---- account / cloud sync ----

/** signedOut → noVault (set a master password) | locked (enter it) → ready */
export type CloudState = 'signedOut' | 'noVault' | 'locked' | 'ready'

export interface CloudStatus {
  state: CloudState
  email: string | null
  syncing: boolean
  lastSyncAt: number | null // ms since epoch
  error: string | null
  pending: number // local changes not on the server yet
}

// ---- team vaults ----

export type TeamRole = 'owner' | 'admin' | 'editor' | 'viewer'

export interface TeamSummary {
  id: string
  name: string
  /** This account's role in the team */
  role: TeamRole
  /** Bumps each time the team key changes */
  epoch: number
}

/** An invite addressed to this account */
export interface InviteForMe {
  id: string
  teamName: string
  invitedBy: string
  role: TeamRole
  expiresAt: string // ISO timestamp
  /** Accepted, waiting for an owner or admin to admit */
  accepted: boolean
}

export interface TeamsView {
  /** This account's, to read out to whoever admits it */
  safetyCode: string
  teams: TeamSummary[]
  invites: InviteForMe[]
}

export interface MemberView {
  userId: string
  email: string
  role: TeamRole
  isMe: boolean
}

/** An invite to the team, as its owners and admins see it */
export interface PendingView {
  id: string
  email: string
  role: TeamRole
  expiresAt: string // ISO timestamp
  accepted: boolean
  /** The code of the key they accepted with, to compare before admitting */
  safetyCode: string | null
}

export interface TeamDetail {
  members: MemberView[]
  pending: PendingView[]
}

/** What moving hosts to another vault did */
export interface MoveResult {
  hosts: number
  /** Credentials copied rather than moved: hosts left behind still use them */
  credentialsCopied: string[]
  /** Moved hosts whose jump host stayed behind; they now connect directly */
  jumpsCleared: string[]
}
