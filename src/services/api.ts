// Typed invoke wrappers — the only place that talks to the backend.
import { invoke } from '@tauri-apps/api/core'
import type {
  Host,
  HostGroup,
  HostInput,
  Snippet,
  Note,
  PortForward,
  ForwardKind,
  AppSettings,
  FileEntry,
  ParsedHost,
  ImportPreview,
  ImportResult,
  FileImportPreview,
  ExportResult,
  Credential,
  CredentialInput,
  SessionLog,
  KnownHostRow,
  UpdateAsset,
  UpdateInfo,
  CloudStatus,
  TeamsView,
  TeamSummary,
  TeamDetail,
  TeamRole,
  MoveResult,
  KeyDefaults,
  GeneratedKey,
  SerialPortEntry,
} from '../types'

export const api = {
  // ---- hosts ----
  listHosts: (): Promise<Host[]> => invoke('list_hosts'),
  listGroups: (): Promise<HostGroup[]> => invoke('list_groups'),
  upsertHost: (input: HostInput): Promise<Host> => invoke('upsert_host', { input }),
  deleteHost: (id: number): Promise<void> => invoke('delete_host', { id }),
  /** `teamId` is the vault a new group goes in (null: personal) */
  upsertGroup: (id: number | null, name: string, teamId: string | null = null): Promise<HostGroup> =>
    invoke('upsert_group', { id, name, teamId }),
  deleteGroup: (id: number): Promise<void> => invoke('delete_group', { id }),
  /** Move hosts, or a group with its hosts, to `teamId`'s vault (null: personal) */
  moveHosts: (hostIds: number[], groupId: number | null, teamId: string | null): Promise<MoveResult> =>
    invoke('move_hosts', { hostIds, groupId, teamId }),
  /** The teams this computer is in, read locally */
  listTeams: (): Promise<TeamSummary[]> => invoke('list_teams'),
  forgetHostKey: (hostId: number): Promise<void> => invoke('forget_host_key', { hostId }),
  resolveHost: (address: string): Promise<string[]> => invoke('resolve_host', { address }),
  /** Short single-line clipboard text, or null (see commands/clipboard.rs) */
  clipboardText: (): Promise<string | null> => invoke('clipboard_text'),
  /** Puts text on the clipboard natively (no user gesture needed; OSC 52 copies) */
  clipboardWriteText: (text: string): Promise<void> => invoke('clipboard_write_text', { text }),
  /** Saves a pasted image where the tab's programs can open it; returns its path */
  pasteImage: (sessionId: number, dataBase64: string, ext: string): Promise<string> =>
    invoke('paste_image', { sessionId, dataBase64, ext }),
  notify: (title: string, body: string): Promise<void> => invoke('notify', { title, body }),
  setKeepAwake: (on: boolean): Promise<void> => invoke('set_keep_awake', { on }),
  /** Commands typed on a host (null = local terminals), newest first */
  historyList: (hostId: number | null): Promise<string[]> => invoke('history_list', { hostId }),
  historyAdd: (hostId: number | null, command: string): Promise<void> => invoke('history_add', { hostId, command }),
  historyClear: (): Promise<void> => invoke('history_clear'),
  /** Reads the server's own shell history once per host; how many came in */
  historyImport: (sessionId: number): Promise<number> => invoke('history_import', { sessionId }),
  keygenDefaults: (): Promise<KeyDefaults> => invoke('keygen_defaults'),
  /** New Ed25519 key in ~/.ssh/<name> (+ .pub) */
  keygen: (name: string, comment: string, passphrase: string | null): Promise<GeneratedKey> =>
    invoke('keygen', { name, comment, passphrase }),
  /** The authorized_keys line for a private key file */
  publicKeyOf: (path: string): Promise<string> => invoke('public_key_of', { path }),
  /** Adds the key to the server's ~/.ssh/authorized_keys; false = it was there already */
  installPublicKey: (sessionId: number, publicKey: string): Promise<boolean> =>
    invoke('install_public_key', { sessionId, publicKey }),

  // ---- known hosts / session logs ----
  listKnownHosts: (): Promise<KnownHostRow[]> => invoke('list_known_hosts'),
  deleteKnownHostEntry: (host: string, keyType: string): Promise<void> =>
    invoke('delete_known_host_entry', { host, keyType }),
  listSessionLogs: (limit = 200): Promise<SessionLog[]> => invoke('list_session_logs', { limit }),
  clearSessionLogs: (): Promise<void> => invoke('clear_session_logs'),

  // ---- local filesystem (SFTP local pane) ----
  localDrives: (): Promise<string[]> => invoke('local_drives'),
  localList: (path: string): Promise<FileEntry[]> => invoke('local_list', { path }),
  localPlaces: (): Promise<{ label: string; path: string }[]> => invoke('local_places'),
  localMkdir: (path: string): Promise<void> => invoke('local_mkdir', { path }),
  localRename: (from: string, to: string): Promise<void> => invoke('local_rename', { from, to }),

  // ---- credentials ----
  listCredentials: (): Promise<Credential[]> => invoke('list_credentials'),
  upsertCredential: (id: number | null, input: CredentialInput): Promise<Credential> =>
    invoke('upsert_credential', { id, input }),
  deleteCredential: (id: number): Promise<void> => invoke('delete_credential', { id }),
  /** The public half of a credential's stored key */
  credentialPublicKey: (id: number): Promise<string> => invoke('credential_public_key', { id }),

  // ---- snippets ----
  listSnippets: (): Promise<Snippet[]> => invoke('list_snippets'),
  /** `teamId` is the vault a new snippet goes in (null: personal) */
  upsertSnippet: (id: number | null, name: string, command: string, teamId: string | null = null): Promise<Snippet> =>
    invoke('upsert_snippet', { id, name, command, teamId }),
  deleteSnippet: (id: number): Promise<void> => invoke('delete_snippet', { id }),
  moveSnippet: (id: number, teamId: string | null): Promise<Snippet> => invoke('move_snippet', { id, teamId }),

  // ---- notes ----
  listNotes: (): Promise<Note[]> => invoke('list_notes'),
  saveNoteImage: (dataBase64: string, ext: string): Promise<string> =>
    invoke('save_note_image', { dataBase64, ext }),
  /** `teamId` is the vault the note goes in (null: personal) */
  createNote: (title = '', body = '', teamId: string | null = null): Promise<Note> =>
    invoke('create_note', { title, body, teamId }),
  updateNote: (id: number, title: string, body: string): Promise<Note> =>
    invoke('update_note', { id, title, body }),
  setNotePinned: (id: number, pinned: boolean): Promise<Note> =>
    invoke('set_note_pinned', { id, pinned }),
  reorderNotes: (ids: number[]): Promise<void> => invoke('reorder_notes', { ids }),
  setNoteArchived: (id: number, archived: boolean): Promise<Note> =>
    invoke('set_note_archived', { id, archived }),
  setNoteTags: (id: number, tags: string[]): Promise<Note> =>
    invoke('set_note_tags', { id, tags }),
  setNoteHost: (id: number, hostId: number | null): Promise<Note> =>
    invoke('set_note_host', { id, hostId }),
  deleteNote: (id: number): Promise<void> => invoke('delete_note', { id }),

  // ---- forwards ----
  listForwards: (): Promise<PortForward[]> => invoke('list_forwards'),
  upsertForward: (
    id: number | null,
    hostId: number,
    name: string,
    kind: ForwardKind,
    localPort: number,
    remoteHost: string,
    remotePort: number,
    autoStart: boolean,
  ): Promise<PortForward> =>
    invoke('upsert_forward', {
      id,
      hostId,
      name,
      kind,
      localPort,
      remoteHost,
      remotePort,
      autoStart,
    }),
  deleteForward: (id: number): Promise<void> => invoke('delete_forward', { id }),
  forwardStart: (id: number): Promise<void> => invoke('forward_start', { id }),
  forwardStop: (id: number): Promise<void> => invoke('forward_stop', { id }),
  listActiveForwards: (): Promise<number[]> => invoke('list_active_forwards'),

  // ---- sessions ----
  connect: (hostId: number, password?: string): Promise<number> =>
    invoke('connect_host', { hostId, password: password ?? null }),
  spawnLocal: (command?: string): Promise<number> =>
    invoke('spawn_local', { command: command ?? null }),
  connectAdhoc: (address: string, port: number, username: string, password: string | null): Promise<number> =>
    invoke('connect_adhoc', { address, port, username, password }),
  confirmHostKey: (sessionId: number, accept: boolean, remember: boolean): Promise<void> =>
    invoke('confirm_host_key', { sessionId, accept, remember }),
  reconnectNow: (sessionId: number): Promise<void> => invoke('reconnect_now', { sessionId }),
  disconnect: (sessionId: number): Promise<void> => invoke('disconnect_session', { sessionId }),
  listSerialPorts: (): Promise<SerialPortEntry[]> => invoke('list_serial_ports'),
  write: (sessionId: number, data: string): Promise<void> => invoke('session_write', { sessionId, data }),
  resize: (
    sessionId: number,
    cols: number,
    rows: number,
    pixWidth = 0,
    pixHeight = 0,
  ): Promise<void> =>
    invoke('session_resize', { sessionId, cols, rows, pixWidth, pixHeight }),

  // ---- sftp ----
  sftpOpen: (sessionId: number): Promise<void> => invoke('sftp_open', { sessionId }),
  sftpList: (sessionId: number, path: string): Promise<FileEntry[]> =>
    invoke('sftp_list', { sessionId, path }),
  sftpMkdir: (sessionId: number, path: string): Promise<void> => invoke('sftp_mkdir', { sessionId, path }),
  sftpRemove: (sessionId: number, path: string, isDir: boolean): Promise<void> =>
    invoke('sftp_remove', { sessionId, path, isDir }),
  sftpRename: (sessionId: number, from: string, to: string): Promise<void> =>
    invoke('sftp_rename', { sessionId, from, to }),
  sftpUpload: (sessionId: number, localPath: string, remotePath: string): Promise<number> =>
    invoke('sftp_upload', { sessionId, localPath, remotePath }),
  sftpDownload: (sessionId: number, remotePath: string, localPath: string): Promise<number> =>
    invoke('sftp_download', { sessionId, remotePath, localPath }),
  sftpCancelTransfer: (sessionId: number, transferId: number): Promise<void> =>
    invoke('sftp_cancel_transfer', { sessionId, transferId }),
  sftpHome: (sessionId: number): Promise<string> => invoke('sftp_home', { sessionId }),
  sftpCrossTransfer: (
    srcSessionId: number,
    srcPath: string,
    dstSessionId: number,
    dstDir: string,
  ): Promise<number> =>
    invoke('sftp_cross_transfer', { srcSessionId, srcPath, dstSessionId, dstDir }),

  // ---- settings ----
  getSettings: (): Promise<AppSettings> => invoke('get_settings'),
  setSettings: (settings: AppSettings): Promise<void> => invoke('set_settings', { settings }),

  // ---- account / cloud sync ----
  cloudStatus: (): Promise<CloudStatus> => invoke('cloud_status'),
  cloudSendCode: (email: string): Promise<void> => invoke('cloud_send_code', { email }),
  cloudVerifyCode: (email: string, code: string): Promise<CloudStatus> =>
    invoke('cloud_verify_code', { email, code }),
  /** Resolves to the recovery key, to show once */
  cloudCreateVault: (password: string): Promise<string> => invoke('cloud_create_vault', { password }),
  cloudUnlock: (password: string): Promise<CloudStatus> => invoke('cloud_unlock', { password }),
  /** Resolves to the new recovery key, to show once */
  cloudRecover: (recoveryKey: string, newPassword: string): Promise<string> =>
    invoke('cloud_recover', { recoveryKey, newPassword }),
  cloudChangePassword: (current: string, newPassword: string): Promise<void> =>
    invoke('cloud_change_password', { current, newPassword }),
  cloudResetVault: (): Promise<CloudStatus> => invoke('cloud_reset_vault'),
  cloudSignOut: (): Promise<CloudStatus> => invoke('cloud_sign_out'),
  cloudSyncNow: (): Promise<CloudStatus> => invoke('cloud_sync_now'),

  // ---- team vaults ----
  cloudTeams: (): Promise<TeamsView> => invoke('cloud_teams'),
  cloudTeamDetail: (teamId: string): Promise<TeamDetail> => invoke('cloud_team_detail', { teamId }),
  cloudCreateTeam: (name: string): Promise<TeamSummary> => invoke('cloud_create_team', { name }),
  cloudRenameTeam: (teamId: string, name: string): Promise<void> =>
    invoke('cloud_rename_team', { teamId, name }),
  cloudDeleteTeam: (teamId: string): Promise<void> => invoke('cloud_delete_team', { teamId }),
  cloudInvite: (teamId: string, email: string, role: TeamRole): Promise<void> =>
    invoke('cloud_invite', { teamId, email, role }),
  cloudAcceptInvite: (inviteId: string): Promise<void> => invoke('cloud_accept_invite', { inviteId }),
  /** Decline an invite, or withdraw one */
  cloudCancelInvite: (inviteId: string): Promise<void> => invoke('cloud_cancel_invite', { inviteId }),
  /** `shownCode` is the safety code compared with the person being admitted */
  cloudAdmit: (inviteId: string, shownCode: string): Promise<void> =>
    invoke('cloud_admit', { inviteId, shownCode }),
  cloudSetMemberRole: (teamId: string, userId: string, role: TeamRole): Promise<void> =>
    invoke('cloud_set_member_role', { teamId, userId, role }),
  /** Resolves to the team's saved passwords the removed member could read */
  cloudRemoveMember: (teamId: string, userId: string): Promise<string[]> =>
    invoke('cloud_remove_member', { teamId, userId }),
  cloudLeaveTeam: (teamId: string): Promise<void> => invoke('cloud_leave_team', { teamId }),

  // ---- self-update ----
  checkUpdate: (): Promise<UpdateInfo> => invoke('check_update'),
  downloadUpdate: (asset: UpdateAsset): Promise<string> =>
    invoke('download_update', { asset }),
  installUpdate: (path: string): Promise<void> => invoke('install_update', { path }),
  openReleasePage: (url: string): Promise<void> => invoke('open_release_page', { url }),

  // ---- import / export ----
  importSshPreview: (): Promise<ImportPreview> => invoke('import_ssh_preview'),
  importSshApply: (hosts: ParsedHost[], importKnownHosts: boolean): Promise<ImportResult> =>
    invoke('import_ssh_apply', { hosts, importKnownHosts }),
  importFilePreview: (path: string): Promise<FileImportPreview> =>
    invoke('import_file_preview', { path }),
  importFileApply: (hosts: ParsedHost[], savePasswords: boolean): Promise<ImportResult> =>
    invoke('import_file_apply', { hosts, savePasswords }),
  exportHosts: (path: string, includeSecrets: boolean): Promise<ExportResult> =>
    invoke('export_hosts', { path, includeSecrets }),

  // ---- dialogs (via tauri-plugin-dialog) ----
  pickFile: async (title: string, filters?: { name: string; extensions: string[] }[]): Promise<string | null> => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const result = await open({ title, multiple: false, directory: false, filters })
    return typeof result === 'string' ? result : null
  },
  pickFiles: async (title: string): Promise<string[]> => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const result = await open({ title, multiple: true, directory: false })
    if (!result) return []
    return Array.isArray(result) ? result : [result]
  },
  pickSaveFile: async (title: string, defaultName: string): Promise<string | null> => {
    const { save } = await import('@tauri-apps/plugin-dialog')
    const result = await save({ title, defaultPath: defaultName })
    return typeof result === 'string' ? result : null
  },
  pickDirectory: async (title: string): Promise<string | null> => {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const result = await open({ title, directory: true })
    return typeof result === 'string' ? result : null
  },
}
