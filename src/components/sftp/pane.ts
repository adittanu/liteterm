// Pane model for the SFTP view: each side is Local or one SSH session, with
// its own location, history, selection and sort. Operations live here so the
// pane component and the view (transfers, drag) share one source of truth.
import { reactive } from 'vue'
import { api } from '../../services/api'
import { errorText, useSessionsStore } from '../../stores/sessions'
import { useHostsStore } from '../../stores/hosts'
import { useUiStore } from '../../stores/ui'
import { waitForConnection } from '../../utils/sessionWait'
import type { FileEntry } from '../../types'
import { baseName, joinPath, samePath, type PaneKind, type SortKey } from './fsUtils'

export interface Pane {
  idx: 0 | 1
  kind: PaneKind
  sessionId: number | null
  hostId: number | null
  hostLabel: string
  connecting: boolean
  connectError: string | null
  path: string
  entries: FileEntry[]
  loading: boolean
  error: string | null
  selected: string[]
  anchor: string | null
  cursor: string | null
  filter: string
  sortKey: SortKey
  sortDir: 1 | -1
  renaming: string | null
  creating: boolean
  back: string[]
  fwd: string[]
  loadToken: number
}

export function createPane(idx: 0 | 1, kind: PaneKind): Pane {
  return reactive({
    idx,
    kind,
    sessionId: null,
    hostId: null,
    hostLabel: '',
    connecting: false,
    connectError: null,
    path: '',
    entries: [],
    loading: false,
    error: null,
    selected: [],
    anchor: null,
    cursor: null,
    filter: '',
    sortKey: 'name',
    sortDir: 1,
    renaming: null,
    creating: false,
    back: [],
    fwd: [],
    loadToken: 0,
  }) as Pane
}

/** True when the pane can list files right now. */
export function isReady(p: Pane): boolean {
  return p.kind === 'local' ? !!p.path : p.sessionId !== null && !p.connecting
}

export function paneLabel(p: Pane): string {
  return p.kind === 'local' ? 'This PC' : p.hostLabel || 'Remote'
}

// ---- local roots (drives + Home/Desktop/…) loaded once per app run ----

export const localRoots = reactive<{ loaded: boolean; drives: string[]; places: { label: string; path: string }[] }>({
  loaded: false,
  drives: [],
  places: [],
})

export async function loadLocalRoots() {
  if (localRoots.loaded) return
  const [drives, places] = await Promise.all([
    api.localDrives().catch(() => [] as string[]),
    api.localPlaces().catch(() => [] as { label: string; path: string }[]),
  ])
  localRoots.drives = drives
  localRoots.places = places
  localRoots.loaded = true
}

// ---- remembered locations: reopening a host lands where you left it ----

const LAST_KEY = (p: Pane) => `sftp.lastPath.${p.kind === 'local' ? 'local' : p.hostId}`

function remember(p: Pane) {
  if (p.kind === 'ssh' && p.hostId === null) return
  try {
    localStorage.setItem(LAST_KEY(p), p.path)
  } catch {
    /* storage full / disabled — not worth surfacing */
  }
}

function remembered(p: Pane): string | null {
  try {
    return localStorage.getItem(LAST_KEY(p))
  } catch {
    return null
  }
}

// ---- listing & navigation ----

export async function listDir(p: Pane, dir: string): Promise<FileEntry[]> {
  if (p.kind === 'local') return api.localList(dir)
  if (p.sessionId === null) throw new Error('Not connected')
  await api.sftpOpen(p.sessionId)
  return api.sftpList(p.sessionId, dir)
}

function isGone(msg: string) {
  return msg.includes('Session not found') || msg.includes('Not connected')
}

export async function navigate(
  p: Pane,
  target: string,
  opts: { push?: boolean; select?: string | null } = {},
): Promise<boolean> {
  const token = ++p.loadToken
  p.loading = true
  p.error = null
  try {
    const entries = await listDir(p, target)
    if (token !== p.loadToken) return false
    const refresh = samePath(p.kind, p.path, target)
    if (!refresh && opts.push !== false && p.path) {
      p.back.push(p.path)
      if (p.back.length > 60) p.back.shift()
      p.fwd = []
    }
    p.entries = entries
    p.path = target
    if (opts.select) {
      setSelection(p, [opts.select])
    } else if (refresh) {
      // Refresh keeps what's still there selected
      const names = new Set(entries.map((e) => e.name))
      p.selected = p.selected.filter((n) => names.has(n))
      if (p.cursor && !names.has(p.cursor)) p.cursor = null
    } else {
      clearSelection(p)
      p.filter = ''
    }
    if (!refresh) {
      p.renaming = null
      p.creating = false
    }
    remember(p)
    return true
  } catch (e) {
    if (token !== p.loadToken) return false
    const msg = errorText(e)
    if (p.kind === 'ssh' && isGone(msg)) {
      markDisconnected(p)
    } else if (p.entries.length && !samePath(p.kind, p.path, target)) {
      // Couldn't open a subfolder (permissions, broken link…): stay where we
      // are and say why, rather than blanking the listing
      useUiStore().toast('error', `Couldn’t open “${baseName(target) || target}”: ${msg}`)
    } else {
      p.error = msg
    }
    return false
  } finally {
    if (token === p.loadToken) p.loading = false
  }
}

export function refresh(p: Pane) {
  if (isReady(p)) return navigate(p, p.path)
}

export function goBack(p: Pane) {
  const prev = p.back.pop()
  if (prev === undefined) return
  p.fwd.push(p.path)
  void navigate(p, prev, { push: false })
}

export function goForward(p: Pane) {
  const next = p.fwd.pop()
  if (next === undefined) return
  p.back.push(p.path)
  void navigate(p, next, { push: false })
}

export function openDir(p: Pane, name: string) {
  const from = p.path
  void navigate(p, joinPath(p.kind, from, name))
}

// ---- binding a side ----

export async function bindLocal(p: Pane, path?: string) {
  await loadLocalRoots()
  p.loadToken++
  p.kind = 'local'
  p.sessionId = null
  p.hostId = null
  p.hostLabel = ''
  p.connecting = false
  p.connectError = null
  p.back = []
  p.fwd = []
  p.entries = []
  p.path = ''
  const start =
    path ?? remembered(p) ?? localRoots.places.find((x) => x.label === 'Home')?.path ?? localRoots.drives[0] ?? 'C:\\'
  const ok = await navigate(p, start, { push: false })
  if (!ok && start !== localRoots.drives[0] && localRoots.drives[0]) {
    await navigate(p, localRoots.drives[0], { push: false })
  }
}

/** Point the pane at a host, connecting on demand without stealing the view. */
export async function bindHost(p: Pane, hostId: number) {
  const sessions = useSessionsStore()
  const host = useHostsStore().hostById(hostId)
  if (!host) return
  const token = ++p.loadToken
  p.kind = 'ssh'
  p.hostId = hostId
  p.hostLabel = host.label
  p.sessionId = null
  p.connecting = true
  p.connectError = null
  p.error = null
  p.entries = []
  p.path = ''
  p.back = []
  p.fwd = []
  clearSelection(p)

  const live = sessions.tabs.find((t) => t.hostId === hostId && t.kind === 'ssh' && t.status === 'connected')
  if (!live) void sessions.connect(hostId, undefined, { silent: true })
  const sessionId = live?.id ?? (await waitForConnection(hostId))
  if (token !== p.loadToken) return // user picked something else meanwhile
  if (sessionId === null) {
    p.connecting = false
    p.connectError = `Couldn’t connect to ${host.label}`
    return
  }

  p.sessionId = sessionId
  let home = '/'
  try {
    await api.sftpOpen(sessionId)
    home = await api.sftpHome(sessionId)
  } catch {
    /* older servers may refuse realpath — root is a fine fallback */
  }
  if (token !== p.loadToken) return
  p.connecting = false
  const start = remembered(p) ?? home
  const ok = await navigate(p, start, { push: false })
  if (!ok && start !== home && p.sessionId !== null) await navigate(p, home, { push: false })
}

/** Back to the host chooser. */
export function unbind(p: Pane) {
  p.loadToken++
  p.kind = 'ssh'
  p.sessionId = null
  p.hostId = null
  p.hostLabel = ''
  p.connecting = false
  p.connectError = null
  p.entries = []
  p.path = ''
  p.error = null
}

export function markDisconnected(p: Pane) {
  p.sessionId = null
  p.connecting = false
  p.connectError = p.hostLabel ? `Disconnected from ${p.hostLabel}` : 'Disconnected'
}

// ---- selection ----

export function setSelection(p: Pane, names: string[], cursor = names[names.length - 1] ?? null) {
  p.selected = names
  p.cursor = cursor
  p.anchor = cursor
}

export function clearSelection(p: Pane) {
  p.selected = []
  p.cursor = null
  p.anchor = null
}

export function selectedEntries(p: Pane): FileEntry[] {
  const set = new Set(p.selected)
  return p.entries.filter((e) => set.has(e.name))
}

// ---- file operations ----

export async function createFolder(p: Pane, name: string): Promise<boolean> {
  const ui = useUiStore()
  const clean = name.trim()
  if (!clean) return false
  if (/[\\/]/.test(clean)) {
    ui.toast('error', 'Folder names can’t contain slashes')
    return false
  }
  if (p.entries.some((e) => e.name === clean)) {
    ui.toast('error', `“${clean}” already exists here`)
    return false
  }
  const full = joinPath(p.kind, p.path, clean)
  try {
    if (p.kind === 'local') await api.localMkdir(full)
    else if (p.sessionId !== null) await api.sftpMkdir(p.sessionId, full)
    else return false
    p.creating = false
    await navigate(p, p.path, { select: clean })
    return true
  } catch (e) {
    ui.toast('error', `Couldn’t create folder: ${errorText(e)}`)
    return false
  }
}

export async function renameEntry(p: Pane, from: string, to: string) {
  const ui = useUiStore()
  const clean = to.trim()
  p.renaming = null
  if (!clean || clean === from) return
  if (/[\\/]/.test(clean)) {
    ui.toast('error', 'Names can’t contain slashes')
    return
  }
  if (p.entries.some((e) => e.name === clean) && clean.toLowerCase() !== from.toLowerCase()) {
    ui.toast('error', `“${clean}” already exists here`)
    return
  }
  try {
    const a = joinPath(p.kind, p.path, from)
    const b = joinPath(p.kind, p.path, clean)
    if (p.kind === 'local') await api.localRename(a, b)
    else if (p.sessionId !== null) await api.sftpRename(p.sessionId, a, b)
    await navigate(p, p.path, { select: clean })
  } catch (e) {
    ui.toast('error', `Rename failed: ${errorText(e)}`)
  }
}

/** Remote only — local deletes stay in Explorer, where the Recycle Bin is. */
export async function deleteEntries(p: Pane, entries: FileEntry[]) {
  const ui = useUiStore()
  if (p.kind !== 'ssh' || p.sessionId === null || !entries.length) return
  let failed = 0
  const deleted: string[] = []
  for (const e of entries) {
    try {
      await api.sftpRemove(p.sessionId, joinPath('ssh', p.path, e.name), e.kind === 'dir')
      deleted.push(e.name)
    } catch (err) {
      failed++
      ui.toast('error', `Couldn’t delete “${e.name}”: ${errorText(err)}`)
    }
  }
  await navigate(p, p.path)
  if (deleted.length) ui.toast('info', deleted.length === 1 ? `Deleted “${deleted[0]}”` : `Deleted ${deleted.length} items`)
  return failed === 0
}
