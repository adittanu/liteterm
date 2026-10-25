<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref } from 'vue'
import {
  ArrowUp,
  RefreshCw,
  Folder,
  File as FileIcon,
  Link2,
  Upload,
  Download,
  Trash2,
  Pencil,
  FilePlus2,
  Check,
  X,
  Search,
  FolderOpen,
  Plug,
  ChevronDown,
  MonitorSmartphone,
  Server,
} from 'lucide-vue-next'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api } from '../services/api'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useHostsStore } from '../stores/hosts'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import PlanetAvatar from '../components/PlanetAvatar.vue'
import SearchSelect, { type ComboOption } from '../components/SearchSelect.vue'
import PathBar from '../components/PathBar.vue'
import type { FileEntry } from '../types'

const sessions = useSessionsStore()
const hosts = useHostsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

// ---- pane model: each side is Local or an SSH session, switchable ----

interface PaneState {
  kind: 'local' | 'ssh'
  sessionId: number | null
  hostLabel: string
  path: string
  entries: FileEntry[]
  loading: boolean
  error: string | null
  selected: string | null
  filter: string
  renaming: string | null
  renameValue: string
  newFolder: boolean
  newFolderName: string
  selectorQuery: string
}

function newPane(kind: 'local' | 'ssh'): PaneState {
  return {
    kind,
    sessionId: null,
    hostLabel: '',
    path: kind === 'local' ? '' : '/',
    entries: [],
    loading: false,
    error: null,
    selected: null,
    filter: '',
    renaming: null,
    renameValue: '',
    newFolder: false,
    newFolderName: '',
    selectorQuery: '',
  }
}

const panes = reactive<PaneState[]>([newPane('local'), newPane('ssh')])
const localDrives = ref<string[]>([])

const driveOptions = computed<ComboOption[]>(() =>
  localDrives.value.map((d) => ({ value: d, label: d })),
)

// ---- path bars: Ctrl+L focuses the active pane's bar ----

const pathBars = ref<(InstanceType<typeof PathBar> | null)[]>([])
const lastActivePane = ref(0)

function setPathBar(idx: number, el: unknown) {
  pathBars.value[idx] = (el as InstanceType<typeof PathBar> | null) ?? null
}

function canUp(pane: PaneState): boolean {
  return pane.kind === 'ssh' ? pane.path !== '/' && pane.path !== '' : localParent(pane.path) !== null
}

/** Directory lister injected into the pane's PathBar autocomplete */
function paneLister(idx: number): ((dir: string) => Promise<FileEntry[]>) | null {
  const pane = panes[idx]
  if (pane.kind === 'local') return (dir) => api.localList(dir)
  if (pane.sessionId === null) return null
  return async (dir) => {
    await api.sftpOpen(pane.sessionId!)
    return api.sftpList(pane.sessionId!, dir)
  }
}

function onViewKeydown(e: KeyboardEvent) {
  if (ui.activeView !== 'sftp') return
  if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyL') {
    e.preventDefault()
    pathBars.value[lastActivePane.value]?.beginEdit()
  }
}

// ---- context menu ----
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)
function openMenu(e: MouseEvent, items: MenuItem[]) {
  menu.value = { x: e.clientX, y: e.clientY, items }
}

// ---- pointer-based drag (WebView2 blocks HTML5 DnD) ----

const drag = ref<{
  paneIdx: number
  entry: FileEntry
  startX: number
  startY: number
  active: boolean
  x: number
  y: number
} | null>(null)

const hoverDrop = ref<{ paneIdx: number; name: string | null } | null>(null)
let justDragged = false

function onRowPointerDown(e: PointerEvent, idx: number, entry: FileEntry) {
  if (e.button !== 0) return
  if ((e.target as HTMLElement).closest('button')) return // row action buttons
  drag.value = {
    paneIdx: idx,
    entry,
    startX: e.clientX,
    startY: e.clientY,
    active: false,
    x: e.clientX,
    y: e.clientY,
  }
}

function onPointerMove(e: PointerEvent) {
  const d = drag.value
  if (!d) return
  d.x = e.clientX
  d.y = e.clientY
  if (!d.active) {
    const dist = Math.hypot(e.clientX - d.startX, e.clientY - d.startY)
    if (dist > 6) d.active = true
  }
  if (d.active) {
    const el = document.elementFromPoint(e.clientX, e.clientY)
    const zone = el?.closest('[data-drop-pane]') as HTMLElement | null
    if (zone) {
      const paneIdx = Number(zone.dataset.dropPane)
      const row = el?.closest('[data-drop-name]') as HTMLElement | null
      const name = row?.dataset.dropName ?? null
      const isDir = row?.dataset.dropKind === 'dir'
      // Targets: the other pane, or a folder row anywhere (incl. same pane)
      if (paneIdx !== d.paneIdx || isDir) {
        hoverDrop.value = { paneIdx, name: isDir ? name : null }
      } else {
        hoverDrop.value = null
      }
    } else {
      hoverDrop.value = null
    }
  }
}

function onPointerUp() {
  const d = drag.value
  drag.value = null
  const target = hoverDrop.value
  hoverDrop.value = null
  if (!d || !d.active || !target) return
  justDragged = true
  transferFile(d.paneIdx, d.entry, target.paneIdx, target.name)
}

function onWindowClick() {
  // Swallow the click that follows a completed drag
  if (justDragged) {
    justDragged = false
  }
}

// ---- helpers ----

function join(dir: string, name: string): string {
  return dir === '/' ? `/${name}` : `${dir}/${name}`
}

function joinLocal(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`
}

function localParent(p: string): string | null {
  const norm = p.replace(/\//g, '\\')
  if (/^[A-Z]:\\?$/i.test(norm)) return null
  const idx = norm.lastIndexOf('\\')
  if (idx <= 2) return norm.slice(0, 3)
  return norm.slice(0, idx)
}

function remoteParent(p: string): string {
  if (p === '/' || p === '') return '/'
  const idx = p.lastIndexOf('/')
  return idx <= 0 ? '/' : p.slice(0, idx)
}

function iconFor(kind: FileEntry['kind']) {
  if (kind === 'dir') return Folder
  if (kind === 'symlink') return Link2
  return FileIcon
}

function formatSize(e: FileEntry): string {
  if (e.kind !== 'file') return '—'
  return fmtBytes(e.size)
}

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`
  if (n < 1024 * 1024 * 1024 * 1024) return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`
  return `${(n / (1024 * 1024 * 1024 * 1024)).toFixed(2)} TB`
}

function kindLabel(e: FileEntry): string {
  if (e.kind === 'dir') return 'folder'
  if (e.kind === 'symlink') return 'symlink'
  const ext = e.name.includes('.') ? e.name.split('.').pop()! : 'file'
  return ext.toLowerCase()
}

function sortedFiltered(pane: PaneState): FileEntry[] {
  const q = pane.filter.trim().toLowerCase()
  return [...pane.entries]
    .filter((e) => !q || e.name.toLowerCase().includes(q))
    .sort((a, b) => {
      const da = a.kind === 'dir' ? 0 : 1
      const db = b.kind === 'dir' ? 0 : 1
      if (da !== db) return da - db
      return a.name.localeCompare(b.name)
    })
}

function paneCrumbs(pane: PaneState): { name: string; path: string }[] {
  if (pane.kind === 'ssh') {
    const parts = pane.path.split('/').filter(Boolean)
    return [{ name: '/', path: '/' }, ...parts.map((p, i) => ({
      name: p,
      path: '/' + parts.slice(0, i + 1).join('/'),
    }))]
  }
  if (!pane.path) return []
  const norm = pane.path.replace(/\//g, '\\')
  const parts = norm.split('\\').filter(Boolean)
  return parts.map((part, i) => ({
    name: part,
    path: i === 0 ? `${part}\\` : parts.slice(0, i + 1).join('\\') + '\\',
  }))
}

// ---- listing ----

function handlePaneError(pane: PaneState, e: unknown) {
  const msg = errorText(e)
  if (
    pane.kind === 'ssh' &&
    (msg.includes('Session not found') || msg.includes('Not connected'))
  ) {
    pane.sessionId = null
    pane.error = null
  } else {
    pane.error = msg
  }
}

async function loadPane(idx: number, path?: string) {
  const pane = panes[idx]
  const target = path ?? pane.path
  if (pane.kind === 'local' && !target) return
  pane.loading = true
  pane.error = null
  try {
    if (pane.kind === 'local') {
      pane.entries = await api.localList(target)
    } else if (pane.sessionId !== null) {
      await api.sftpOpen(pane.sessionId)
      pane.entries = await api.sftpList(pane.sessionId, target)
    } else {
      return
    }
    pane.path = target
    pane.selected = null
    pane.renaming = null
  } catch (e) {
    handlePaneError(pane, e)
  } finally {
    pane.loading = false
  }
}

async function openEntry(idx: number, entry: FileEntry) {
  const pane = panes[idx]
  if (entry.kind !== 'dir') return
  await loadPane(idx, pane.kind === 'local' ? joinLocal(pane.path, entry.name) : join(pane.path, entry.name))
}

function paneUp(idx: number) {
  const pane = panes[idx]
  if (pane.kind === 'ssh') {
    void loadPane(idx, remoteParent(pane.path))
  } else {
    const parent = localParent(pane.path)
    if (parent) void loadPane(idx, parent)
  }
}

// ---- pane side switching ----

const connectedHostIds = computed(
  () =>
    new Set(
      sessions.tabs
        .filter((t) => t.status === 'connected' && t.kind === 'ssh')
        .map((t) => t.hostId),
    ),
)

function paneTitle(pane: PaneState): string {
  if (pane.kind === 'local') return 'Local'
  return pane.sessionId !== null ? pane.hostLabel || 'Remote' : 'Select host'
}

// Searchable popover anchored under the pane's switcher button
const sidePicker = ref<{ idx: number } | null>(null)
const sideQuery = ref('')
const sideInputEl = ref<HTMLInputElement | null>(null)

function openSidePicker(idx: number) {
  sidePicker.value = { idx }
  sideQuery.value = ''
  // Focus after the click's default action settles — a nextTick-only focus
  // can be stolen back by the switcher button's native focus on WebView2
  void nextTick(() => {
    sideInputEl.value?.focus({ preventScroll: true })
    window.setTimeout(() => sideInputEl.value?.focus({ preventScroll: true }), 0)
  })
}

function closeSidePicker() {
  sidePicker.value = null
}

const sidePickerHosts = computed(() => {
  const q = sideQuery.value.trim().toLowerCase()
  const sorted = [...hosts.hosts].sort((a, b) => {
    const ca = connectedHostIds.value.has(a.id) ? 0 : 1
    const cb = connectedHostIds.value.has(b.id) ? 0 : 1
    return ca - cb || a.label.localeCompare(b.label)
  })
  if (!q) return sorted
  return sorted.filter(
    (h) =>
      h.label.toLowerCase().includes(q) ||
      h.address.toLowerCase().includes(q) ||
      h.username.toLowerCase().includes(q),
  )
})

function pickSideLocal(idx: number) {
  useLocal(idx)
  closeSidePicker()
}

function pickSideHost(idx: number, hostId: number) {
  void useHost(idx, hostId)
  closeSidePicker()
}

// Current drive of a local pane, e.g. "C:\"
function currentDrive(pane: PaneState): string {
  const m = pane.path.match(/^[A-Za-z]:\\/)
  return m ? m[0] : localDrives.value[0] ?? ''
}

function onDriveChange(idx: number, drive: string) {
  void loadPane(idx, drive)
}

function useLocal(idx: number) {
  const pane = panes[idx]
  pane.kind = 'local'
  pane.sessionId = null
  pane.hostLabel = ''
  pane.path = localDrives.value[0] ?? ''
  void loadPane(idx)
}

function useHost(idx: number, hostId: number) {
  const pane = panes[idx]
  const host = hosts.hostById(hostId)
  if (!host) return

  const existing = sessions.tabs.find(
    (t) => t.hostId === hostId && t.status === 'connected' && t.kind === 'ssh',
  )
  if (existing) {
    pane.kind = 'ssh'
    pane.sessionId = existing.id
    pane.hostLabel = host.label
    pane.path = '/'
    void loadPane(idx)
    return
  }
  // Connect on demand (silently — SFTP must not steal the terminal view),
  // then bind the pane when the session is up
  const stop = sessions.$subscribe(() => {
    const tab = sessions.tabs.find(
      (t) => t.hostId === hostId && t.status === 'connected' && t.kind === 'ssh',
    )
    if (tab) {
      pane.kind = 'ssh'
      pane.sessionId = tab.id
      pane.hostLabel = host.label
      pane.path = '/'
      void loadPane(idx)
      stop()
    }
  })
  void sessions.connect(hostId, undefined, { silent: true })
}

// Reset panes whose session disappeared
function watchSessions() {
  sessions.$subscribe(() => {
    for (const pane of panes) {
      if (pane.kind === 'ssh' && pane.sessionId !== null) {
        const alive = sessions.tabs.some((t) => t.id === pane.sessionId)
        if (!alive) {
          pane.sessionId = null
          pane.entries = []
        }
      }
    }
  })
}

// ---- drive selector ----
// handled by the always-visible drive <select> in the pane header

// ---- file operations (remote only) ----

async function mkdir(idx: number) {
  const pane = panes[idx]
  const name = pane.newFolderName.trim()
  if (!name || pane.sessionId === null) return
  try {
    await api.sftpMkdir(pane.sessionId, join(pane.path, name))
    pane.newFolder = false
    pane.newFolderName = ''
    await loadPane(idx)
  } catch (e) {
    ui.toast('error', `mkdir failed: ${errorText(e)}`)
  }
}

async function removeEntry(idx: number, entry: FileEntry) {
  const pane = panes[idx]
  if (pane.sessionId === null) return
  if (ui.settings.confirmDeleteFile) {
    const ok = await dialogs.confirm({
      title: entry.kind === 'dir' ? `Delete folder “${entry.name}”?` : `Delete “${entry.name}”?`,
      message:
        entry.kind === 'dir'
          ? 'The folder and everything inside it will be deleted on the server. This cannot be undone.'
          : 'This file will be deleted on the server. This cannot be undone.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!ok) return
  }
  try {
    await api.sftpRemove(pane.sessionId, join(pane.path, entry.name), entry.kind === 'dir')
    await loadPane(idx)
  } catch (e) {
    ui.toast('error', `Delete failed: ${errorText(e)}`)
  }
}

function startRename(idx: number, entry: FileEntry) {
  const pane = panes[idx]
  pane.selected = entry.name
  pane.renaming = entry.name
  pane.renameValue = entry.name
}

async function commitRename(idx: number) {
  const pane = panes[idx]
  const from = pane.renaming
  const to = pane.renameValue.trim()
  pane.renaming = null
  if (!from || !to || from === to || pane.sessionId === null) return
  try {
    await api.sftpRename(pane.sessionId, join(pane.path, from), join(pane.path, to))
    await loadPane(idx)
  } catch (e) {
    ui.toast('error', `Rename failed: ${errorText(e)}`)
  }
}

// ---- transfers (any direction) ----

function trackUpload(sessionId: number, localPathFull: string, remoteDir: string, name: string, size: number) {
  void api
    .sftpUpload(sessionId, localPathFull, join(remoteDir, name))
    .then((id) => {
      sessions.trackTransfer({
        id, sessionId, kind: 'upload', fileName: name, size, transferred: 0, done: false,
      })
    })
    .catch((e) => ui.toast('error', `Upload failed: ${errorText(e)}`))
}

function trackDownload(sessionId: number, remotePathFull: string, localDir: string, name: string, size: number) {
  void api
    .sftpDownload(sessionId, remotePathFull, joinLocal(localDir, name))
    .then((id) => {
      sessions.trackTransfer({
        id, sessionId, kind: 'download', fileName: name, size, transferred: 0, done: false,
      })
    })
    .catch((e) => ui.toast('error', `Download failed: ${errorText(e)}`))
}

/**
 * Copy one file from pane `fromIdx` into `toIdx` (into its current dir,
 * or into `dirName` when dropped on a folder row).
 */
function transferFile(fromIdx: number, entry: FileEntry, toIdx: number, dirName: string | null) {
  const from = panes[fromIdx]
  const to = panes[toIdx]
  if (entry.kind !== 'file') {
    ui.toast('info', 'Folder transfer is not supported yet — files only')
    return
  }

  const toDir =
    to.kind === 'ssh'
      ? dirName ? join(to.path, dirName) : to.path
      : dirName ? joinLocal(to.path, dirName) : to.path

  if (from.kind === 'local' && to.kind === 'ssh' && to.sessionId !== null) {
    trackUpload(to.sessionId, joinLocal(from.path, entry.name), toDir, entry.name, entry.size)
    return
  }
  if (from.kind === 'ssh' && from.sessionId !== null && to.kind === 'local' && to.path) {
    trackDownload(from.sessionId, join(from.path, entry.name), toDir, entry.name, entry.size)
    return
  }
  if (from.kind === 'ssh' && to.kind === 'ssh' && from.sessionId !== null && to.sessionId !== null) {
    if (from.sessionId === to.sessionId) {
      // Same server: a cheap rename/move
      const fromPath = join(from.path, entry.name)
      const toPath = join(toDir, entry.name)
      if (fromPath !== toPath) {
        void api
          .sftpRename(from.sessionId, fromPath, toPath)
          .then(() => loadPane(fromIdx))
          .catch((e) => ui.toast('error', `Move failed: ${errorText(e)}`))
      }
      return
    }
    // Different servers: routed through a temp file in one combined transfer
    const sessionId = from.sessionId
    void api
      .sftpCrossTransfer(sessionId, join(from.path, entry.name), to.sessionId, toDir)
      .then((id) => {
        sessions.trackTransfer({
          id, sessionId, kind: 'upload',
          fileName: `${entry.name} (${from.hostLabel} → ${to.hostLabel})`,
          size: entry.size, transferred: 0, done: false,
        })
      })
      .catch((e) => ui.toast('error', `Transfer failed: ${errorText(e)}`))
    return
  }
  if (from.kind === 'local' && to.kind === 'local') {
    ui.toast('info', 'Local-to-local copy is not supported yet')
  }
}

async function uploadPicked(idx: number) {
  const pane = panes[idx]
  if (pane.sessionId === null) return
  const paths = await api.pickFiles('Choose files to upload')
  if (!paths.length) return
  for (const p of paths) {
    const name = p.split(/[\\/]/).pop() ?? 'file'
    trackUpload(pane.sessionId, p, pane.path, name, 0)
  }
}

// ---- context menus per pane ----

function entryMenu(e: MouseEvent, idx: number, entry: FileEntry) {
  const pane = panes[idx]
  pane.selected = entry.name
  const other = panes[1 - idx]

  const items: MenuItem[] = []
  if (entry.kind === 'dir') {
    items.push({ label: 'Open', icon: FolderOpen, action: () => void openEntry(idx, entry) })
  }
  if (pane.kind === 'ssh') {
    if (entry.kind === 'file') {
      const dirLabel = other.kind === 'ssh' ? other.hostLabel : 'Local'
      items.push({
        label: `Send to ${dirLabel}`,
        icon: Download,
        action: () => transferFile(idx, entry, 1 - idx, null),
      })
    }
    items.push({ label: 'Rename', icon: Pencil, action: () => startRename(idx, entry) })
    items.push({ label: 'Delete', icon: Trash2, danger: true, action: () => void removeEntry(idx, entry) })
  } else {
    if (entry.kind === 'file' && other.kind === 'ssh') {
      items.push({
        label: `Upload to ${other.hostLabel || 'remote'}`,
        icon: Upload,
        action: () => transferFile(idx, entry, 1 - idx, null),
      })
    }
  }
  openMenu(e, items)
}

function bgMenu(e: MouseEvent, idx: number) {
  const pane = panes[idx]
  const items: MenuItem[] = []
  if (pane.kind === 'ssh' && pane.sessionId !== null) {
    items.push(
      {
        label: 'New folder',
        icon: FilePlus2,
        action: () => {
          pane.newFolder = true
          pane.newFolderName = ''
        },
      },
      { label: 'Upload files…', icon: Upload, action: () => void uploadPicked(idx) },
    )
  }
  items.push(
    { label: 'Refresh', icon: RefreshCw, action: () => void loadPane(idx) },
    { label: 'Switch side…', icon: Plug, action: () => openSidePicker(idx) },
  )
  openMenu(e, items)
}

// ---- native OS file drop (Explorer → pane) ----
let unlistenDrop: (() => void) | null = null

onMounted(async () => {
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('click', onWindowClick, true)
  window.addEventListener('keydown', onViewKeydown, true)

  try {
    localDrives.value = await api.localDrives()
  } catch {
    localDrives.value = []
  }
  panes[0].path = localDrives.value[0] ?? ''
  void loadPane(0)

  // Right pane: prefer an already-connected SSH session
  const live = sessions.tabs.find((t) => t.status === 'connected' && t.kind === 'ssh')
  if (live && live.hostId !== null) {
    void useHost(1, live.hostId)
  }

  unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type !== 'drop') return
    if (ui.activeView !== 'sftp') return
    // Map the physical position to a pane
    const scale = window.devicePixelRatio || 1
    const el = document.elementFromPoint(
      event.payload.position.x / scale,
      event.payload.position.y / scale,
    )
    const zone = el?.closest('[data-drop-pane]') as HTMLElement | null
    const idx = zone ? Number(zone.dataset.dropPane) : 1
    const pane = panes[idx]
    if (pane.kind !== 'ssh' || pane.sessionId === null) {
      ui.toast('info', 'Drop files on an SSH pane to upload')
      return
    }
    for (const path of event.payload.paths) {
      const name = path.split(/[\\/]/).pop() ?? 'file'
      trackUpload(pane.sessionId, path, pane.path, name, 0)
    }
  })

  watchSessions()
})

onUnmounted(() => {
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  window.removeEventListener('click', onWindowClick, true)
  window.removeEventListener('keydown', onViewKeydown, true)
  unlistenDrop?.()
})
</script>

<template>
  <div class="sftp-view">
    <!-- Click-away layer for the side picker -->
    <div v-if="sidePicker" class="picker-backdrop" @click="closeSidePicker"></div>
    <section
      v-for="(pane, idx) in panes"
      :key="idx"
      class="pane"
      :class="{ 'drop-here': hoverDrop?.paneIdx === idx && hoverDrop?.name === null }"
      @pointerdown.capture="lastActivePane = idx"
    >
      <header class="pane-header">
        <div class="switcher-wrap">
          <button class="side-switcher" title="Switch this side" @click="sidePicker?.idx === idx ? closeSidePicker() : openSidePicker(idx)">
            <MonitorSmartphone v-if="pane.kind === 'local'" :size="14" :stroke-width="1.5" aria-hidden="true" />
            <Server v-else :size="14" :stroke-width="1.5" aria-hidden="true" />
            <span class="side-label text-ellipsis">{{ paneTitle(pane) }}</span>
            <ChevronDown :size="12" :stroke-width="2" aria-hidden="true" />
          </button>

          <!-- Searchable side picker -->
          <div
            v-if="sidePicker?.idx === idx"
            class="side-popover surface"
            @click.stop
          >
            <div class="picker-search">
              <Search :size="14" :stroke-width="1.5" aria-hidden="true" />
              <input
                ref="sideInputEl"
                v-model="sideQuery"
                class="picker-input"
                type="text"
                placeholder="Search hosts…"
                spellcheck="false"
                aria-label="Search hosts"
                @keydown.esc="closeSidePicker"
              />
            </div>
            <div class="side-popover-list">
              <button class="picker-row" @click="pickSideLocal(idx)">
                <span class="side-popover-icon local">
                  <MonitorSmartphone :size="15" :stroke-width="1.5" aria-hidden="true" />
                </span>
                <span class="picker-text">
                  <span class="picker-name">Local</span>
                  <span class="picker-sub">this PC</span>
                </span>
              </button>
              <button
                v-for="h in sidePickerHosts"
                :key="h.id"
                class="picker-row"
                @click="pickSideHost(idx, h.id)"
              >
                <PlanetAvatar :host="h" :size="30" />
                <span class="picker-text">
                  <span class="picker-name text-ellipsis">{{ h.label }}</span>
                  <span class="picker-sub mono text-ellipsis">{{ h.username }}@{{ h.address }}</span>
                </span>
                <span v-if="connectedHostIds.has(h.id)" class="badge badge-success">
                  <Plug :size="10" :stroke-width="2" aria-hidden="true" />
                  live
                </span>
              </button>
              <div v-if="!sidePickerHosts.length" class="side-popover-empty">No hosts match</div>
            </div>
          </div>
        </div>

        <div class="pane-filter" :class="{ filled: pane.filter }">
          <Search :size="13" :stroke-width="1.5" aria-hidden="true" />
          <input
            v-model="pane.filter"
            type="text"
            placeholder="Filter"
            spellcheck="false"
            :aria-label="`Filter pane ${idx + 1}`"
          />
        </div>
      </header>

      <!-- SSH side without a session: inline host selector -->
      <div v-if="pane.kind === 'ssh' && pane.sessionId === null" class="picker">
        <div class="picker-search">
          <Search :size="15" :stroke-width="1.5" aria-hidden="true" />
          <input v-model="pane.selectorQuery" class="picker-input" type="text" placeholder="Select a host to browse its files…" spellcheck="false" :aria-label="`Select host for pane ${idx + 1}`" />
        </div>
        <div class="picker-list">
          <div v-if="!hosts.hosts.length" class="pane-state">
            <p>No hosts yet — add one in Hosts first.</p>
          </div>
          <button
            v-for="h in hosts.hosts.filter((x) =>
              !pane.selectorQuery.trim() ||
              x.label.toLowerCase().includes(pane.selectorQuery.trim().toLowerCase()) ||
              x.address.toLowerCase().includes(pane.selectorQuery.trim().toLowerCase())
            )"
            :key="h.id"
            class="picker-row"
            @click="useHost(idx, h.id)"
          >
            <PlanetAvatar :host="h" :size="32" />
            <span class="picker-text">
              <span class="picker-name text-ellipsis">{{ h.label }}</span>
              <span class="picker-sub mono text-ellipsis">{{ h.username }}@{{ h.address }}</span>
            </span>
            <span v-if="connectedHostIds.has(h.id)" class="badge badge-success">
              <Plug :size="10" :stroke-width="2" aria-hidden="true" />
              live
            </span>
          </button>
        </div>
      </div>

      <template v-else>
        <div class="path-row">
          <button class="path-tool" aria-label="Up" title="Up" :disabled="!canUp(pane)" @click="paneUp(idx)">
            <ArrowUp :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>

          <SearchSelect
            v-if="pane.kind === 'local'"
            class="drive-combo"
            :model-value="currentDrive(pane)"
            :options="driveOptions"
            :searchable="false"
            compact
            aria-label="Drive"
            title="Switch drive"
            @update:model-value="onDriveChange(idx, $event as string)"
          />

          <PathBar
            :ref="(el) => setPathBar(idx, el)"
            :path="pane.path"
            :crumbs="paneCrumbs(pane)"
            :list-fn="paneLister(idx)"
            :drives="pane.kind === 'local' ? localDrives : []"
            :sep="pane.kind === 'local' ? '\\' : '/'"
            :placeholder="pane.kind === 'local' ? 'Type a path…' : 'Type a remote path…'"
            @navigate="(p) => loadPane(idx, p)"
          />

          <button class="path-tool" aria-label="Refresh" title="Refresh" @click="loadPane(idx)">
            <RefreshCw :size="14" :stroke-width="1.5" :class="{ spinning: pane.loading }" aria-hidden="true" />
          </button>
          <button
            v-if="pane.kind === 'ssh'"
            class="path-tool"
            aria-label="New folder"
            title="New folder"
            @click="pane.newFolder = true; pane.newFolderName = ''"
          >
            <FilePlus2 :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button
            v-if="pane.kind === 'ssh' && pane.sessionId !== null"
            class="path-tool"
            aria-label="Upload files"
            title="Upload files…"
            @click="uploadPicked(idx)"
          >
            <Upload :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>

        <div v-if="pane.newFolder" class="inline-form">
          <input
            v-model="pane.newFolderName"
            class="field-input mono"
            type="text"
            placeholder="folder name"
            spellcheck="false"
            @keydown.enter="mkdir(idx)"
            @keydown.esc="pane.newFolder = false"
          />
          <button class="btn-icon" aria-label="Create folder" @click="mkdir(idx)">
            <Check :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="btn-icon" aria-label="Cancel" @click="pane.newFolder = false">
            <X :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>

        <div
          class="file-list"
          role="listbox"
          :aria-label="`Pane ${idx + 1} files`"
          :data-drop-pane="idx"
          @contextmenu.prevent="bgMenu($event, idx)"
        >
          <div v-if="pane.loading && !pane.entries.length" class="pane-state">
            <span class="spinner" aria-hidden="true"></span>
          </div>
          <div v-else-if="pane.error" class="pane-state">
            <p class="pane-error mono">{{ pane.error }}</p>
            <button class="btn btn-secondary" @click="loadPane(idx)">Retry</button>
          </div>
          <div v-else-if="!sortedFiltered(pane).length" class="pane-state">
            <FolderOpen :size="22" :stroke-width="1.25" class="state-icon" aria-hidden="true" />
            <p>{{ pane.filter.trim() ? 'No matches' : 'Empty folder' }}</p>
          </div>

          <template v-else>
            <div class="cols" :aria-hidden="true">
              <span></span>
              <span>Name</span>
              <span class="col-size">Size</span>
              <span class="col-kind">Kind</span>
              <span class="col-count">{{ sortedFiltered(pane).length }} {{ sortedFiltered(pane).length === 1 ? 'item' : 'items' }}</span>
            </div>
            <div
              v-for="entry in sortedFiltered(pane)"
              :key="entry.name"
              class="file-row"
              :class="{
                selected: pane.selected === entry.name,
                'drop-here': hoverDrop?.paneIdx === idx && hoverDrop?.name === entry.name,
              }"
              role="option"
              :aria-selected="pane.selected === entry.name"
              :data-drop-name="entry.kind === 'dir' ? entry.name : undefined"
              :data-drop-kind="entry.kind"
              @pointerdown="onRowPointerDown($event, idx, entry)"
              @click="pane.selected = entry.name"
              @dblclick="openEntry(idx, entry)"
              @contextmenu.prevent.stop="entryMenu($event, idx, entry)"
            >
              <template v-if="pane.renaming === entry.name">
                <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" aria-hidden="true" />
                <input
                  v-model="pane.renameValue"
                  class="field-input mono rename-input"
                  spellcheck="false"
                  autofocus
                  @keydown.enter="commitRename(idx)"
                  @keydown.esc="pane.renaming = null"
                  @blur="commitRename(idx)"
                />
              </template>
              <template v-else>
                <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" :class="['file-icon', entry.kind]" aria-hidden="true" />
                <span class="file-name">
                  <span class="text-ellipsis">{{ entry.name }}</span>
                  <span v-if="pane.kind === 'ssh' && entry.permissions !== null" class="perm mono">{{ (entry.permissions >>> 0).toString(8).padStart(4, '0') }}</span>
                </span>
                <span class="file-size">{{ formatSize(entry) }}</span>
                <span class="file-kind">{{ kindLabel(entry) }}</span>
                <span class="file-actions" @click.stop>
                  <template v-if="pane.kind === 'ssh'">
                    <button class="btn-icon" :aria-label="`Rename ${entry.name}`" title="Rename" @click="startRename(idx, entry)">
                      <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
                    </button>
                    <button class="btn-icon file-delete" :aria-label="`Delete ${entry.name}`" title="Delete" @click="removeEntry(idx, entry)">
                      <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
                    </button>
                  </template>
                  <button
                    v-else-if="entry.kind === 'file' && panes[1 - idx].kind === 'ssh'"
                    class="btn-icon"
                    :aria-label="`Upload ${entry.name}`"
                    title="Upload to other pane"
                    @click="transferFile(idx, entry, 1 - idx, null)"
                  >
                    <Upload :size="13" :stroke-width="1.5" aria-hidden="true" />
                  </button>
                </span>
              </template>
            </div>
          </template>
        </div>
      </template>
    </section>

    <!-- Transfers strip (all directions) -->
    <Transition name="toast">
      <div v-if="sessions.transfers.length" class="transfers-strip">
        <div v-for="t in sessions.transfers" :key="t.id" class="transfer">
        <div class="transfer-row">
          <span class="transfer-kind" :class="t.kind">{{ t.kind === 'upload' ? '↑' : '↓' }}</span>
          <span class="transfer-name text-ellipsis">{{ t.fileName }}</span>
          <span v-if="!t.done && !t.error" class="spinner small" aria-hidden="true"></span>
          <button class="btn-icon" :aria-label="t.done || t.error ? 'Dismiss' : 'Cancel transfer'" @click="t.done || t.error ? sessions.removeTransfer(t.id) : sessions.cancelTransfer(t.id)">
            <X :size="12" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
        <div v-if="!t.error" class="transfer-bar" role="progressbar" :aria-valuenow="Math.round((t.transferred / Math.max(t.size, 1)) * 100)" aria-valuemin="0" aria-valuemax="100">
          <div class="transfer-fill" :class="{ indeterminate: !t.done && t.size === 0, complete: t.done }" :style="{ width: t.size ? `${(t.transferred / t.size) * 100}%` : undefined }"></div>
        </div>
        <div class="transfer-meta">
          <span v-if="t.error" class="transfer-error">{{ t.error }}</span>
          <span v-else-if="t.done" class="transfer-done">
            {{ t.size ? fmtBytes(t.size) : '' }}<template v-if="t.size"> · </template>Done
          </span>
          <span v-else-if="t.size">
            {{ fmtBytes(t.transferred) }} / {{ fmtBytes(t.size) }}
            · {{ Math.round((t.transferred / t.size) * 100) }}%
          </span>
          <span v-else>Starting…</span>
          </div>
        </div>
      </div>
    </Transition>

    <!-- Drag ghost -->
    <div v-if="drag?.active" class="drag-ghost" :style="{ left: `${drag.x + 12}px`, top: `${drag.y + 12}px` }">
      <FileIcon :size="12" :stroke-width="1.5" aria-hidden="true" />
      {{ drag.entry.name }}
    </div>

    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
.sftp-view {
  display: flex;
  height: 100%;
  min-height: 0;
  position: relative;
}

.picker-backdrop {
  position: fixed;
  inset: 0;
  z-index: 25;
}

.pane {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  border-right: 1px solid var(--border);
}

.pane:last-of-type {
  border-right: none;
}

.pane.drop-here {
  outline: 2px dashed var(--accent);
  outline-offset: -4px;
  background: var(--accent-bg);
}

.pane-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.switcher-wrap {
  position: relative;
  min-width: 0;
}

.side-popover {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 30;
  width: 340px;
  max-width: calc(100vw - 80px);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-3);
  padding: var(--space-2);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  animation: side-pop 140ms var(--ease-out);
  transform-origin: top left;
}

@keyframes side-pop {
  from {
    opacity: 0;
    transform: scale(0.97) translateY(-3px);
  }
}

.side-popover-list {
  max-height: 320px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.side-popover-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-sm);
  flex-shrink: 0;
}

.side-popover-icon.local {
  background: var(--accent-bg);
  color: var(--accent);
}

.side-popover-empty {
  padding: var(--space-4);
  text-align: center;
  color: var(--text-4);
  font-size: var(--text-sm);
}

.drive-combo {
  flex-shrink: 0;
}

.side-switcher {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  max-width: 220px;
  padding: 5px 9px;
  border-radius: var(--radius-md);
  background: var(--bg-input);
  border: 1px solid var(--border);
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  transition: border-color var(--dur-hover) ease, background-color var(--dur-hover) ease;
}

.side-switcher:hover {
  border-color: var(--border-strong);
  background: var(--bg-hover);
}

.side-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.pane-filter {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 6px;
  width: 150px;
  height: 26px;
  padding: 0 8px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  color: var(--text-4);
  transition: border-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.pane-filter:focus-within {
  border-color: var(--accent);
  color: var(--text-3);
}

.pane-filter input {
  flex: 1;
  min-width: 0;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.pane-filter input::placeholder {
  color: var(--text-4);
}

/* Inline host selector */
.picker {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: var(--space-3) var(--space-4);
  gap: var(--space-2);
}

.picker-search {
  display: flex;
  align-items: center;
  gap: 9px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 8px 12px;
  color: var(--text-3);
}

.picker-search:focus-within {
  border-color: var(--accent);
}

.picker-input {
  flex: 1;
  border: none;
  background: none;
  font-size: var(--text-md);
  color: var(--text-1);
}

.picker-input:focus {
  outline: none;
}

.picker-list {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  max-width: 560px;
}

.picker-row {
  display: flex;
  align-items: center;
  gap: 11px;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  text-align: left;
  transition: background-color var(--dur-hover) ease;
}

.picker-row:hover {
  background: var(--bg-hover);
}

.picker-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.3;
}

.picker-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.picker-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

/* ---- path row: [up] [drive] [breadcrumb ⇄ path input] [actions] ---- */

.path-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  min-height: 42px;
}

.path-tool {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  flex-shrink: 0;
  transition:
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.path-tool:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-1);
}

.path-tool:active:not(:disabled) {
  transform: scale(0.94);
}

.path-tool:disabled {
  opacity: 0.35;
  cursor: default;
}

.path-tool :deep(.spinning),
.spinning {
  animation: refresh-rotate 0.7s linear infinite;
}

@keyframes refresh-rotate {
  to {
    transform: rotate(360deg);
  }
}

.file-list {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  padding: 0 0 var(--space-2);
}

/* Sticky column labels — same grid as the rows so they stay aligned */
.cols {
  position: sticky;
  top: 0;
  z-index: 2;
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 84px 64px 56px;
  gap: 10px;
  align-items: center;
  padding: 5px 10px 4px;
  background: var(--bg);
  border-bottom: 1px solid var(--border);
  font-size: 10.5px;
  letter-spacing: 0.02em;
  color: var(--text-4);
}

.col-size {
  text-align: right;
}

.col-count {
  text-align: right;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.state-icon {
  color: var(--text-4);
}

.file-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 84px 64px 56px;
  align-items: center;
  gap: 10px;
  min-height: 30px;
  padding: 0 10px;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  cursor: default;
  transition: background-color var(--dur-hover) ease;
}

.file-row:hover {
  background: var(--bg-hover);
}

.file-row.selected {
  background: var(--bg-selected);
}

.file-row.drop-here {
  outline: 2px dashed var(--accent);
  outline-offset: -2px;
  background: var(--accent-bg);
}

.file-icon.dir { color: var(--accent); }
.file-icon.symlink { color: var(--warning); }
.file-icon.file { color: var(--text-3); }

.file-name {
  min-width: 0;
  display: flex;
  flex-direction: column;
  line-height: 1.25;
}

.file-name > span:first-child {
  color: var(--text-1);
}

.perm {
  font-size: 0.625rem;
  color: var(--text-4);
}

.rename-input {
  padding: 2px 6px !important;
  font-size: var(--text-sm) !important;
}

.file-size {
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.file-kind {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.file-actions {
  display: flex;
  gap: 1px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.file-row:hover .file-actions,
.file-row:focus-within .file-actions {
  opacity: 1;
}

.file-delete:hover {
  color: var(--danger);
}

.pane-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-6) var(--space-4);
  color: var(--text-3);
  font-size: var(--text-sm);
  text-align: center;
}

.pane-error {
  font-size: var(--text-xs);
  color: var(--danger);
  word-break: break-all;
  user-select: text;
}

.inline-form {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border);
}

.inline-form .field-input {
  flex: 1;
  padding: 5px 8px;
  font-size: var(--text-sm);
}

/* Transfers strip spans the whole view bottom */
.transfers-strip {
  position: absolute;
  left: var(--space-3);
  right: var(--space-3);
  bottom: var(--space-2);
  z-index: 5;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-pop);
  max-height: 180px;
  overflow-y: auto;
}

.transfer-row {
  display: flex;
  align-items: center;
  gap: 7px;
}

.transfer-kind {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  width: 14px;
  text-align: center;
}

.transfer-kind.upload { color: var(--accent); }
.transfer-kind.download { color: var(--success); }

.transfer-name {
  flex: 1;
  min-width: 0;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.transfer-bar {
  height: 3px;
  border-radius: 2px;
  background: var(--bg-surface-3);
  margin: 4px 0 2px;
  overflow: hidden;
}

.transfer-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 2px;
}

.transfer-fill.indeterminate {
  width: 30% !important;
  animation: indeterminate 1.1s ease-in-out infinite;
}

@keyframes indeterminate {
  0% { transform: translateX(-100%); }
  100% { transform: translateX(430%); }
}

.transfer-fill.complete {
  background: var(--success);
}

.transfer-meta {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.transfer-done { color: var(--success); }
.transfer-error { color: var(--danger); user-select: text; word-break: break-all; }

/* Pointer-drag ghost */
.drag-ghost {
  position: fixed;
  z-index: 95;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 5px 10px;
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  border: 1px solid var(--accent);
  box-shadow: var(--shadow-pop);
  font-size: var(--text-sm);
  color: var(--text-1);
  pointer-events: none;
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
