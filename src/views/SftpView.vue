<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { File as FileIcon, Files } from 'lucide-vue-next'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api } from '../services/api'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import FilePane from '../components/sftp/FilePane.vue'
import TransferTray from '../components/sftp/TransferTray.vue'
import {
  bindHost,
  bindLocal,
  createPane,
  isReady,
  markDisconnected,
  navigate,
  paneLabel,
  type Pane,
} from '../components/sftp/pane'
import { baseName, joinLocal, joinPath, joinRemote, samePath } from '../components/sftp/fsUtils'

const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const panes: [Pane, Pane] = [createPane(0, 'local'), createPane(1, 'ssh')]
const activeIdx = ref<0 | 1>(0)
const paneRefs = ref<(InstanceType<typeof FilePane> | null)[]>([null, null])

// ---- preferences ----

const showHidden = ref(localStorage.getItem('sftp.showHidden') === '1')
watch(showHidden, (v) => localStorage.setItem('sftp.showHidden', v ? '1' : '0'))

const split = ref(Number(localStorage.getItem('sftp.split')) || 0.5)
watch(split, (v) => localStorage.setItem('sftp.split', v.toFixed(3)))

// ---- resizable divider: tracks the pointer 1:1, double-click recenters ----

const bodyEl = ref<HTMLElement | null>(null)
const resizing = ref(false)

function onDividerDown(e: PointerEvent) {
  if (e.button !== 0) return
  ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  resizing.value = true
}

function onDividerMove(e: PointerEvent) {
  if (!resizing.value || !bodyEl.value) return
  const r = bodyEl.value.getBoundingClientRect()
  split.value = Math.min(0.75, Math.max(0.25, (e.clientX - r.left) / r.width))
}

function onDividerUp() {
  resizing.value = false
}

function onDividerKey(e: KeyboardEvent) {
  if (e.key === 'ArrowLeft') split.value = Math.max(0.25, split.value - 0.02)
  else if (e.key === 'ArrowRight') split.value = Math.min(0.75, split.value + 0.02)
  else if (e.key === 'Home' || e.key === 'Enter') split.value = 0.5
}

// ---- transfers ----

// Shown under each transfer in the tray, and used to refresh the destination
const destinations = reactive<Record<string, string>>({})
const refreshOnDone = new Map<string, { paneIdx: 0 | 1; dir: string; sessionId: number | null }>()

function destLabel(to: Pane, dir: string): string {
  const where = baseName(dir) || dir
  return to.kind === 'local' ? `to ${where}` : `to ${to.hostLabel}:${where}`
}

function track(
  start: Promise<number>,
  meta: { sessionId: number; kind: 'upload' | 'download'; name: string; size: number },
  to: Pane,
  dir: string,
) {
  void start
    .then((id) => {
      sessions.trackTransfer({
        id,
        sessionId: meta.sessionId,
        kind: meta.kind,
        fileName: meta.name,
        size: meta.size,
        transferred: 0,
        done: false,
      })
      const k = `${meta.sessionId}:${id}`
      destinations[k] = destLabel(to, dir)
      refreshOnDone.set(k, { paneIdx: to.idx, dir, sessionId: to.sessionId })
    })
    .catch((e) => ui.toast('error', `Couldn’t start ${meta.kind} of “${meta.name}”: ${errorText(e)}`))
}

// When a transfer lands, refresh the pane that's looking at its destination
const refreshTimers: (ReturnType<typeof setTimeout> | undefined)[] = [undefined, undefined]
watch(
  () => sessions.transfers.filter((t) => t.done || t.error).map((t) => `${t.sessionId}:${t.id}:${t.error ? 'x' : 'ok'}`).join('|'),
  () => {
    for (const t of sessions.transfers) {
      if (!t.done && !t.error) continue
      const k = `${t.sessionId}:${t.id}`
      const target = refreshOnDone.get(k)
      if (!target) continue
      refreshOnDone.delete(k)
      if (t.error) continue
      const pane = panes[target.paneIdx]
      if (pane.sessionId !== target.sessionId || !samePath(pane.kind, pane.path, target.dir)) continue
      clearTimeout(refreshTimers[target.paneIdx])
      refreshTimers[target.paneIdx] = setTimeout(() => void navigate(pane, pane.path), 250)
    }
  },
)

async function confirmReplace(names: string[], where: string): Promise<boolean> {
  const list = names.slice(0, 3).map((n) => `“${n}”`).join(', ') + (names.length > 3 ? ` and ${names.length - 3} more` : '')
  return dialogs.confirm({
    title: names.length === 1 ? `Replace “${names[0]}”?` : `Replace ${names.length} files?`,
    message: `${list} already ${names.length === 1 ? 'exists' : 'exist'} in ${where}. Replacing overwrites ${names.length === 1 ? 'it' : 'them'}.`,
    confirmLabel: 'Replace',
    danger: true,
  })
}

/** Move within one filesystem: same server, or same local pane. */
async function moveWithin(from: Pane, names: string[], toDir: string, to: Pane) {
  let moved = 0
  for (const name of names) {
    const a = joinPath(from.kind, from.path, name)
    const b = joinPath(from.kind, toDir, name)
    if (a === b) continue
    try {
      if (from.kind === 'local') await api.localRename(a, b)
      else if (from.sessionId !== null) await api.sftpRename(from.sessionId, a, b)
      moved++
    } catch (e) {
      ui.toast('error', `Couldn’t move “${name}”: ${errorText(e)}`)
    }
  }
  await navigate(from, from.path)
  if (to !== from && isReady(to)) await navigate(to, to.path)
  if (moved) ui.toast('success', moved === 1 ? `Moved “${names[0]}” to ${baseName(toDir) || toDir}` : `Moved ${moved} items to ${baseName(toDir) || toDir}`)
}

async function transfer(fromIdx: 0 | 1, names: string[], toIdx: 0 | 1, folder: string | null) {
  const from = panes[fromIdx]
  const to = panes[toIdx]
  const picked = from.entries.filter((e) => names.includes(e.name))
  if (!picked.length || !isReady(to)) return
  const toDir = folder ? joinPath(to.kind, to.path, folder) : to.path

  const sameFs =
    (from.kind === 'ssh' && to.kind === 'ssh' && from.sessionId === to.sessionId) ||
    (from === to && from.kind === 'local')
  if (sameFs) {
    if (folder && names.includes(folder) && from === to) return
    return moveWithin(from, picked.map((e) => e.name), toDir, to)
  }
  if (from.kind === 'local' && to.kind === 'local') {
    ui.toast('info', 'Copying between local folders isn’t supported — use Explorer for that')
    return
  }

  const files = picked.filter((e) => e.kind === 'file')
  const skipped = picked.length - files.length
  if (!files.length) {
    ui.toast('info', 'Folders can’t be transferred yet — select files instead')
    return
  }

  // Only check clashes when we can see the destination listing
  if (!folder) {
    const clash = files.filter((f) => to.entries.some((e) => e.name === f.name)).map((f) => f.name)
    if (clash.length && !(await confirmReplace(clash, `${paneLabel(to)} · ${baseName(toDir) || toDir}`))) return
  }

  for (const f of files) {
    if (from.kind === 'local' && to.sessionId !== null) {
      track(api.sftpUpload(to.sessionId, joinLocal(from.path, f.name), joinRemote(toDir, f.name)), { sessionId: to.sessionId, kind: 'upload', name: f.name, size: f.size }, to, toDir)
    } else if (from.kind === 'ssh' && from.sessionId !== null && to.kind === 'local') {
      track(api.sftpDownload(from.sessionId, joinRemote(from.path, f.name), joinLocal(toDir, f.name)), { sessionId: from.sessionId, kind: 'download', name: f.name, size: f.size }, to, toDir)
    } else if (from.sessionId !== null && to.sessionId !== null) {
      track(
        api.sftpCrossTransfer(from.sessionId, joinRemote(from.path, f.name), to.sessionId, toDir),
        { sessionId: from.sessionId, kind: 'upload', name: `${f.name} (${from.hostLabel} → ${to.hostLabel})`, size: f.size },
        to,
        toDir,
      )
    }
  }
  if (skipped) ui.toast('info', `${skipped === 1 ? '1 folder was' : `${skipped} folders were`} skipped — only files transfer for now`)
}

/** Files from Explorer (OS drop or file picker) into an SSH pane. */
function uploadPaths(idx: 0 | 1, paths: string[], folder: string | null = null) {
  const to = panes[idx]
  if (to.kind !== 'ssh' || to.sessionId === null) return
  const dir = folder ? joinRemote(to.path, folder) : to.path
  for (const path of paths) {
    const name = baseName(path) || 'file'
    track(api.sftpUpload(to.sessionId, path, joinRemote(dir, name)), { sessionId: to.sessionId, kind: 'upload', name, size: 0 }, to, dir)
  }
}

// ---- in-app drag (pointer events: WebView2 blocks HTML5 DnD here) ----

const drag = ref<{
  from: 0 | 1
  names: string[]
  startX: number
  startY: number
  active: boolean
} | null>(null)
const hover = ref<{ pane: 0 | 1; folder: string | null } | null>(null)
const ghostEl = ref<HTMLElement | null>(null)
// Where the ghost first appears; later moves write its transform directly
// (no style binding) so a drag never re-renders the file lists
const ghostStart = { x: 0, y: 0 }
watch(
  ghostEl,
  (el) => {
    if (el) el.style.transform = ghostTransform(ghostStart.x, ghostStart.y)
  },
  { flush: 'sync' },
)

function ghostTransform(x: number, y: number) {
  return `translate3d(${x + 14}px, ${y + 12}px, 0)`
}

function onDragStart(idx: 0 | 1, e: PointerEvent, names: string[]) {
  drag.value = { from: idx, names, startX: e.clientX, startY: e.clientY, active: false }
}

function hitTest(x: number, y: number): { pane: 0 | 1; folder: string | null } | null {
  const el = document.elementFromPoint(x, y)
  const zone = el?.closest('[data-drop-pane]') as HTMLElement | null
  if (!zone) return null
  const row = el?.closest('[data-drop-name]') as HTMLElement | null
  return { pane: Number(zone.dataset.dropPane) as 0 | 1, folder: row?.dataset.dropName ?? null }
}

function sameHover(a: typeof hover.value, b: typeof hover.value) {
  return a?.pane === b?.pane && a?.folder === b?.folder
}

function onPointerMove(e: PointerEvent) {
  const d = drag.value
  if (!d) return
  if (!d.active) {
    // ~6px of hysteresis before a press becomes a drag
    if (Math.hypot(e.clientX - d.startX, e.clientY - d.startY) <= 6) return
    ghostStart.x = e.clientX
    ghostStart.y = e.clientY
    d.active = true
  }
  if (ghostEl.value) ghostEl.value.style.transform = ghostTransform(e.clientX, e.clientY)
  const hit = hitTest(e.clientX, e.clientY)
  // Dropping a folder onto itself means nothing
  if (hit?.folder && hit.pane === d.from && d.names.includes(hit.folder)) hit.folder = null
  // The source pane only accepts drops onto a folder row
  const next = hit && (hit.pane !== d.from || hit.folder) ? hit : null
  if (!sameHover(next, hover.value)) hover.value = next
}

function onPointerUp() {
  const d = drag.value
  const target = hover.value
  drag.value = null
  hover.value = null
  if (!d?.active || !target) return
  const zone = zoneFor(target.pane)
  if (zone && !zone.ok) {
    ui.toast('info', zone.label)
    return
  }
  void transfer(d.from, d.names, target.pane, target.folder)
}

function onKeydown(e: KeyboardEvent) {
  if (ui.activeView !== 'sftp') return
  if (e.key === 'Escape' && drag.value?.active) {
    drag.value = null
    hover.value = null
    return
  }
  if (e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey && e.code === 'KeyL') {
    e.preventDefault()
    paneRefs.value[activeIdx.value]?.focusPath()
  }
}

// ---- drop cue wording: say exactly what will happen ----

const osHover = ref<{ pane: 0 | 1; folder: string | null } | null>(null)

function describe(fromKind: 'local' | 'ssh' | 'os', from: Pane | null, to: Pane, folder: string | null): { label: string; ok: boolean } {
  const where = folder ?? (baseName(to.path) || to.path)
  if (!isReady(to)) return { label: 'Open a location here first', ok: false }
  if (fromKind === 'os' || fromKind === 'local') {
    if (to.kind === 'ssh') return { label: `Upload to ${to.hostLabel} · ${where}`, ok: true }
    if (from === to && folder) return { label: `Move into ${folder}`, ok: true }
    return { label: fromKind === 'os' ? 'Drop on a server to upload' : 'Copying between local folders isn’t supported', ok: false }
  }
  if (to.kind === 'local') return { label: `Download to ${where}`, ok: true }
  if (from && from.sessionId === to.sessionId) return { label: `Move to ${where}`, ok: true }
  return { label: `Copy to ${to.hostLabel} · ${where}`, ok: true }
}

function zoneFor(idx: 0 | 1): { folder: string | null; label: string; ok: boolean } | null {
  const d = drag.value
  if (d?.active && hover.value?.pane === idx) {
    const from = panes[d.from]
    return { folder: hover.value.folder, ...describe(from.kind, from, panes[idx], hover.value.folder) }
  }
  if (osHover.value?.pane === idx) {
    return { folder: osHover.value.folder, ...describe('os', null, panes[idx], osHover.value.folder) }
  }
  return null
}

const zones = computed(() => [zoneFor(0), zoneFor(1)] as const)

const ghost = computed(() => {
  const d = drag.value
  if (!d?.active) return null
  return { count: d.names.length, label: d.names.length === 1 ? d.names[0] : `${d.names.length} items` }
})

// ---- sessions going away ----

// A request while the view is already open (e.g. from the palette)
watch(
  () => ui.sftpHostRequest,
  (id) => {
    if (id === null) return
    ui.sftpHostRequest = null
    const target = panes[1].kind === 'local' && panes[0].kind === 'ssh' ? panes[0] : panes[1]
    if (target.hostId !== id || target.sessionId === null) void bindHost(target, id)
  },
)

watch(
  () => sessions.tabs.map((t) => `${t.id}:${t.status}`).join('|'),
  () => {
    for (const p of panes) {
      if (p.kind !== 'ssh' || p.sessionId === null) continue
      const tab = sessions.tabs.find((t) => t.id === p.sessionId)
      if (!tab || tab.status === 'closed' || tab.status === 'error') markDisconnected(p)
    }
  },
)

// ---- lifecycle ----

let unlistenDrop: (() => void) | null = null

onMounted(async () => {
  window.addEventListener('pointermove', onPointerMove)
  window.addEventListener('pointerup', onPointerUp)
  window.addEventListener('keydown', onKeydown, true)

  void bindLocal(panes[0])
  // Right side: a host someone asked for, else the terminal you were just using
  const requested = ui.sftpHostRequest
  ui.sftpHostRequest = null
  const live =
    sessions.tabs.find((t) => t.id === sessions.activeId && t.kind === 'ssh' && t.status === 'connected') ??
    sessions.tabs.find((t) => t.kind === 'ssh' && t.status === 'connected')
  if (requested !== null) void bindHost(panes[1], requested)
  else if (live?.hostId != null) void bindHost(panes[1], live.hostId)

  try {
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      if (ui.activeView !== 'sftp') return
      const pl = event.payload
      if (pl.type === 'leave') {
        osHover.value = null
        return
      }
      const scale = window.devicePixelRatio || 1
      const hit = hitTest(pl.position.x / scale, pl.position.y / scale)
      if (pl.type === 'enter' || pl.type === 'over') {
        osHover.value = hit
        return
      }
      // drop
      osHover.value = null
      const idx = hit?.pane ?? 1
      const pane = panes[idx]
      if (pane.kind !== 'ssh' || pane.sessionId === null) {
        ui.toast('info', 'Drop files on a server pane to upload them')
        return
      }
      uploadPaths(idx, pl.paths, hit?.folder ?? null)
    })
  } catch {
    // not running inside Tauri (e.g. plain browser preview)
  }
})

onUnmounted(() => {
  window.removeEventListener('pointermove', onPointerMove)
  window.removeEventListener('pointerup', onPointerUp)
  window.removeEventListener('keydown', onKeydown, true)
  refreshTimers.forEach((t) => clearTimeout(t))
  unlistenDrop?.()
})

function setPaneRef(i: 0 | 1, el: unknown) {
  paneRefs.value[i] = (el as InstanceType<typeof FilePane> | null) ?? null
}
</script>

<template>
  <div class="sftp-view" :class="{ dragging: drag?.active, resizing }">
    <div ref="bodyEl" class="sftp-body" :style="{ gridTemplateColumns: `${split}fr 1px ${1 - split}fr` }">
      <FilePane
        :ref="(el) => setPaneRef(0, el)"
        :pane="panes[0]"
        :other="panes[1]"
        :active="activeIdx === 0"
        :show-hidden="showHidden"
        :drop-zone="zones[0]"
        @activate="activeIdx = 0"
        @send="(names, folder) => transfer(0, names, 1, folder)"
        @drag-start="(e, names) => onDragStart(0, e, names)"
        @toggle-hidden="showHidden = !showHidden"
        @upload="(paths) => uploadPaths(0, paths)"
      />

      <div
        class="divider"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize panes"
        :aria-valuenow="Math.round(split * 100)"
        aria-valuemin="25"
        aria-valuemax="75"
        tabindex="0"
        title="Drag to resize · double-click to center"
        @pointerdown="onDividerDown"
        @pointermove="onDividerMove"
        @pointerup="onDividerUp"
        @pointercancel="onDividerUp"
        @dblclick="split = 0.5"
        @keydown="onDividerKey"
      ></div>

      <FilePane
        :ref="(el) => setPaneRef(1, el)"
        :pane="panes[1]"
        :other="panes[0]"
        :active="activeIdx === 1"
        :show-hidden="showHidden"
        :drop-zone="zones[1]"
        @activate="activeIdx = 1"
        @send="(names, folder) => transfer(1, names, 0, folder)"
        @drag-start="(e, names) => onDragStart(1, e, names)"
        @toggle-hidden="showHidden = !showHidden"
        @upload="(paths) => uploadPaths(1, paths)"
      />
    </div>

    <TransferTray :destinations="destinations" />

    <!-- Drag ghost: a stack when carrying several items -->
    <div
      v-if="ghost"
      ref="ghostEl"
      class="drag-ghost"
      :class="{ stack: ghost.count > 1 }"
      aria-hidden="true"
    >
      <Files v-if="ghost.count > 1" :size="13" :stroke-width="1.6" />
      <FileIcon v-else :size="13" :stroke-width="1.6" />
      <span class="text-ellipsis">{{ ghost.label }}</span>
    </div>
  </div>
</template>

<style scoped>
.sftp-view {
  position: relative;
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
}

.sftp-view.dragging,
.sftp-view.dragging * {
  cursor: grabbing !important;
}

.sftp-view.resizing,
.sftp-view.resizing * {
  cursor: col-resize !important;
  user-select: none;
}

.sftp-body {
  display: grid;
  flex: 1;
  min-height: 0;
}

/* 1px hairline with a wider invisible grab area */
.divider {
  position: relative;
  background: var(--border);
  cursor: col-resize;
  outline: none;
}

.divider::before {
  content: '';
  position: absolute;
  inset: 0 -4px;
  z-index: 4;
}

.divider::after {
  content: '';
  position: absolute;
  inset: 0 -1px;
  background: var(--accent);
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.divider:hover::after,
.divider:focus-visible::after,
.resizing .divider::after {
  opacity: 0.7;
}

.drag-ghost {
  position: fixed;
  left: 0;
  top: 0;
  z-index: 95;
  display: flex;
  align-items: center;
  gap: 7px;
  max-width: 260px;
  padding: 6px 11px;
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  border: 1px solid var(--border-strong);
  box-shadow: var(--shadow-pop);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-1);
  pointer-events: none;
  will-change: transform;
  white-space: nowrap;
}

.drag-ghost svg {
  color: var(--accent);
  flex-shrink: 0;
}

/* A second card peeking out behind = "several things" */
.drag-ghost.stack {
  box-shadow: 4px 4px 0 -1px var(--bg-surface-2), 4px 4px 0 0 var(--border-strong), var(--shadow-pop);
}
</style>
