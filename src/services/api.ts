// Typed invoke wrappers — the only place that talks to the backend.
import { invoke } from '@tauri-apps/api/core'
import type {
  Host,
  HostGroup,
  HostInput,
  Snippet,
  Note,
  PortForward,
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
} from '../types'

export const api = {
  // ---- hosts ----
  listHosts: (): Promise<Host[]> => invoke('list_hosts'),
  listGroups: (): Promise<HostGroup[]> => invoke('list_groups'),
  upsertHost: (input: HostInput): Promise<Host> => invoke('upsert_host', { input }),
  deleteHost: (id: number): Promise<void> => invoke('delete_host', { id }),
  upsertGroup: (id: number | null, name: string): Promise<HostGroup> =>
    invoke('upsert_group', { id, name }),
  deleteGroup: (id: number): Promise<void> => invoke('delete_group', { id }),
  forgetHostKey: (hostId: number): Promise<void> => invoke('forget_host_key', { hostId }),
  resolveHost: (address: string): Promise<string[]> => invoke('resolve_host', { address }),

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

  // ---- snippets ----
  listSnippets: (): Promise<Snippet[]> => invoke('list_snippets'),
  upsertSnippet: (id: number | null, name: string, command: string): Promise<Snippet> =>
    invoke('upsert_snippet', { id, name, command }),
  deleteSnippet: (id: number): Promise<void> => invoke('delete_snippet', { id }),

  // ---- notes ----
  listNotes: (): Promise<Note[]> => invoke('list_notes'),
  createNote: (title = '', body = ''): Promise<Note> => invoke('create_note', { title, body }),
  updateNote: (id: number, title: string, body: string): Promise<Note> =>
    invoke('update_note', { id, title, body }),
  setNotePinned: (id: number, pinned: boolean): Promise<Note> =>
    invoke('set_note_pinned', { id, pinned }),
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
    localPort: number,
    remoteHost: string,
    remotePort: number,
    autoStart: boolean,
  ): Promise<PortForward> =>
    invoke('upsert_forward', {
      id,
      hostId,
      name,
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
  write: (sessionId: number, data: string): Promise<void> => invoke('session_write', { sessionId, data }),
  resize: (sessionId: number, cols: number, rows: number): Promise<void> =>
    invoke('session_resize', { sessionId, cols, rows }),

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

  // ---- self-update ----
  checkUpdate: (): Promise<UpdateInfo> => invoke('check_update'),
  downloadUpdate: (asset: UpdateAsset): Promise<string> =>
    invoke('download_update', { asset }),
  installUpdate: (path: string): Promise<void> => invoke('install_update', { path }),

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
