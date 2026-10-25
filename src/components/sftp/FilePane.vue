<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import {
  ArrowDownToLine,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  ArrowUpFromLine,
  ChevronDown,
  ChevronUp,
  ClipboardCopy,
  Eye,
  EyeOff,
  FolderOpen,
  FolderPlus,
  HardDrive,
  House,
  Laptop,
  MoreHorizontal,
  Pencil,
  RefreshCw,
  ArrowLeftRight,
  SquareTerminal,
  Trash2,
  Unplug,
  Upload,
} from 'lucide-vue-next'
import PathBar from '../PathBar.vue'
import PlanetAvatar from '../PlanetAvatar.vue'
import SearchField from '../SearchField.vue'
import ContextMenu, { type MenuItem } from '../ContextMenu.vue'
import SidePicker from './SidePicker.vue'
import { api } from '../../services/api'
import { useHostsStore } from '../../stores/hosts'
import { useSessionsStore } from '../../stores/sessions'
import { useDialogsStore } from '../../stores/dialogs'
import { useUiStore } from '../../stores/ui'
import { copyText, plural } from '../../utils/pageHelpers'
import {
  baseName,
  crumbsFor,
  fileVisual,
  fmtBytes,
  fmtModified,
  fmtPerms,
  isHidden,
  octalPerms,
  parentOf,
  rangeNames,
  shellQuote,
  sortEntries,
  type SortKey,
} from './fsUtils'
import {
  bindHost,
  bindLocal,
  clearSelection,
  createFolder,
  deleteEntries,
  goBack,
  goForward,
  isReady,
  listDir,
  localRoots,
  navigate,
  openDir,
  paneLabel,
  refresh,
  renameEntry,
  selectedEntries,
  setSelection,
  unbind,
  type Pane,
} from './pane'
import type { FileEntry } from '../../types'

const props = defineProps<{
  pane: Pane
  other: Pane
  active: boolean
  showHidden: boolean
  /** Drag hover from the view: whole pane (folder = null) or one folder row */
  dropZone: { folder: string | null; label: string; ok: boolean } | null
}>()

const emit = defineEmits<{
  activate: []
  send: [names: string[], folder: string | null]
  'drag-start': [e: PointerEvent, names: string[]]
  'toggle-hidden': []
  upload: [paths: string[]]
}>()

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const p = props.pane
const listEl = ref<HTMLElement | null>(null)
const pathBar = ref<InstanceType<typeof PathBar> | null>(null)
const renameEl = ref<HTMLInputElement | null>(null)
function setRenameEl(el: unknown) {
  renameEl.value = (el as HTMLInputElement | null) ?? null
}
const createEl = ref<HTMLInputElement | null>(null)
const renameValue = ref('')
const createValue = ref('')
const pickerOpen = ref(false)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)

const host = computed(() => hosts.hostById(p.hostId))
const ready = computed(() => isReady(p))
const showChooser = computed(() => p.kind === 'ssh' && p.hostId === null)

// ---- what's on screen ----

const hiddenCount = computed(() => p.entries.filter(isHidden).length)

const visible = computed<FileEntry[]>(() => {
  const q = p.filter.trim().toLowerCase()
  const list = p.entries.filter(
    (e) => (props.showHidden || !isHidden(e)) && (!q || e.name.toLowerCase().includes(q)),
  )
  return sortEntries(list, p.sortKey, p.sortDir)
})

const cursorIndex = computed(() => (p.cursor ? visible.value.findIndex((e) => e.name === p.cursor) : -1))

const selectedSet = computed(() => new Set(p.selected))
const selection = computed(() => selectedEntries(p))
const selectionBytes = computed(() =>
  selection.value.reduce((sum, e) => sum + (e.kind === 'file' ? e.size : 0), 0),
)

function setSort(key: SortKey) {
  if (p.sortKey === key) p.sortDir = p.sortDir === 1 ? -1 : 1
  else {
    p.sortKey = key
    // Newest / largest first is what people look for
    p.sortDir = key === 'name' ? 1 : -1
  }
}

// ---- transfer target ("Upload to prod →") ----

const towardRight = computed(() => p.idx === 0)

const send = computed(() => {
  const files = selection.value.filter((e) => e.kind === 'file')
  const folders = selection.value.length - files.length
  const o = props.other
  let label = 'Send'
  let reason: string | null = null
  if (!files.length) reason = folders ? 'Folders can’t be transferred yet — select files' : null
  else if (!isReady(o)) reason = 'Open a location on the other side first'
  else if (p.kind === 'local' && o.kind === 'local') reason = 'Copying between local folders isn’t supported'
  if (p.kind === 'local' && o.kind === 'ssh') label = `Upload to ${o.hostLabel}`
  else if (p.kind === 'ssh' && o.kind === 'local') label = 'Download to This PC'
  else if (p.kind === 'ssh' && o.kind === 'ssh')
    label = o.sessionId === p.sessionId ? `Move to ${baseName(o.path) || '/'}` : `Copy to ${o.hostLabel}`
  return { files, folders, label, reason }
})

function sendSelection() {
  if (send.value.reason || !send.value.files.length) return
  emit('send', send.value.files.map((f) => f.name), null)
}

// ---- selection (Finder/Explorer semantics) ----

function focusList() {
  listEl.value?.focus({ preventScroll: true })
}

function onRowClick(e: MouseEvent, entry: FileEntry) {
  emit('activate')
  if (e.ctrlKey || e.metaKey) {
    const has = selectedSet.value.has(entry.name)
    p.selected = has ? p.selected.filter((n) => n !== entry.name) : [...p.selected, entry.name]
    p.cursor = entry.name
    p.anchor = entry.name
  } else if (e.shiftKey && p.anchor) {
    p.selected = rangeNames(visible.value, p.anchor, entry.name)
    p.cursor = entry.name
  } else {
    setSelection(p, [entry.name])
  }
  focusList()
}

function onRowDblClick(entry: FileEntry) {
  if (entry.kind === 'dir' || entry.kind === 'symlink') {
    // Symlinks are usually folders on servers; if not, listing fails gracefully
    openDir(p, entry.name)
    return
  }
  // FileZilla-style: double-click a file sends it to the other side
  setSelection(p, [entry.name])
  if (!send.value.reason) emit('send', [entry.name], null)
  else ui.toast('info', send.value.reason)
}

function onRowPointerDown(e: PointerEvent, entry: FileEntry) {
  if (e.button !== 0 || p.renaming) return
  if ((e.target as HTMLElement).closest('button, input')) return
  // Drag the whole selection when grabbing a selected row
  const names = selectedSet.value.has(entry.name) ? [...p.selected] : [entry.name]
  emit('drag-start', e, names)
}

function onBlankClick(e: MouseEvent) {
  emit('activate')
  if (e.target === e.currentTarget || (e.target as HTMLElement).classList.contains('rows-end')) {
    clearSelection(p)
  }
  focusList()
}

function scrollCursorIntoView() {
  if (!p.cursor) return
  void nextTick(() => {
    listEl.value
      ?.querySelector(`[data-row="${CSS.escape(p.cursor!)}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  })
}

// Type-to-select: letters jump to the first matching name
let typeBuf = ''
let typeTimer: ReturnType<typeof setTimeout> | undefined

function typeahead(ch: string) {
  clearTimeout(typeTimer)
  typeBuf += ch.toLowerCase()
  typeTimer = setTimeout(() => (typeBuf = ''), 700)
  const hit = visible.value.find((e) => e.name.toLowerCase().startsWith(typeBuf))
  if (hit) {
    setSelection(p, [hit.name])
    scrollCursorIntoView()
  }
}

function moveCursor(delta: number, extend: boolean) {
  const order = visible.value
  if (!order.length) return
  const i = p.cursor ? order.findIndex((e) => e.name === p.cursor) : -1
  const last = order.length - 1
  const target = !Number.isFinite(delta)
    ? delta > 0 ? last : 0
    : i === -1 ? (delta > 0 ? 0 : last) : Math.max(0, Math.min(last, i + delta))
  const next = order[target]
  if (extend && p.anchor) {
    p.selected = rangeNames(order, p.anchor, next.name)
    p.cursor = next.name
  } else {
    setSelection(p, [next.name])
  }
  scrollCursorIntoView()
}

function onListKey(e: KeyboardEvent) {
  if (p.renaming || p.creating) return
  const mod = e.ctrlKey || e.metaKey
  const cursorEntry = p.entries.find((x) => x.name === p.cursor)
  if (e.altKey && e.key === 'ArrowLeft') return void (e.preventDefault(), goBack(p))
  if (e.altKey && e.key === 'ArrowRight') return void (e.preventDefault(), goForward(p))
  if (e.altKey && e.key === 'ArrowUp') return void (e.preventDefault(), goUp())
  switch (e.key) {
    case 'ArrowDown':
      e.preventDefault()
      return moveCursor(1, e.shiftKey)
    case 'ArrowUp':
      e.preventDefault()
      return moveCursor(-1, e.shiftKey)
    case 'PageDown':
      e.preventDefault()
      return moveCursor(12, e.shiftKey)
    case 'PageUp':
      e.preventDefault()
      return moveCursor(-12, e.shiftKey)
    case 'Home':
      e.preventDefault()
      return moveCursor(-Infinity, e.shiftKey)
    case 'End':
      e.preventDefault()
      return moveCursor(Infinity, e.shiftKey)
    case 'Enter':
      e.preventDefault()
      if (cursorEntry) onRowDblClick(cursorEntry)
      return
    case 'Backspace':
      e.preventDefault()
      return goUp()
    case 'F2':
      e.preventDefault()
      if (cursorEntry) startRename(cursorEntry)
      return
    case 'F5':
      e.preventDefault() // never let F5 reload the whole WebView
      return void refresh(p)
    case 'Delete':
      e.preventDefault()
      return void confirmDelete(selection.value)
    case 'Escape':
      if (p.selected.length) {
        e.preventDefault()
        clearSelection(p)
      } else if (p.filter) {
        e.preventDefault()
        p.filter = ''
      }
      return
  }
  if (mod && e.key.toLowerCase() === 'a') {
    e.preventDefault()
    p.selected = visible.value.map((x) => x.name)
    return
  }
  if (mod && e.shiftKey && e.key.toLowerCase() === 'n') {
    e.preventDefault()
    return startCreate()
  }
  if (!mod && !e.altKey && e.key.length === 1 && e.key !== ' ') typeahead(e.key)
}

// Mouse back/forward buttons navigate the pane, like a browser
function onMouseUp(e: MouseEvent) {
  if (e.button === 3) {
    e.preventDefault()
    goBack(p)
  } else if (e.button === 4) {
    e.preventDefault()
    goForward(p)
  }
}

function goUp() {
  const parent = parentOf(p.kind, p.path)
  if (!parent) return
  const from = baseName(p.path)
  void navigate(p, parent, { select: from })
}

// ---- rename / new folder (inline, in the list) ----

function startRename(entry: FileEntry) {
  setSelection(p, [entry.name])
  p.renaming = entry.name
  renameValue.value = entry.name
  void nextTick(() => {
    const el = renameEl.value
    if (!el) return
    el.focus()
    // Select the stem, not the extension — the usual intent
    const dot = entry.kind === 'file' ? entry.name.lastIndexOf('.') : -1
    el.setSelectionRange(0, dot > 0 ? dot : entry.name.length)
  })
}

function commitRename() {
  const from = p.renaming
  if (!from) return
  void renameEntry(p, from, renameValue.value).then(focusList)
}

function cancelRename() {
  p.renaming = null
  focusList()
}

function startCreate() {
  if (!ready.value) return
  emit('activate')
  p.creating = true
  createValue.value = 'New folder'
  void nextTick(() => {
    listEl.value?.scrollTo({ top: 0 })
    createEl.value?.focus()
    createEl.value?.select()
  })
}

let createBusy = false
function commitCreate() {
  if (createBusy) return
  createBusy = true
  void createFolder(p, createValue.value)
    .then((ok) => ok && focusList())
    .finally(() => (createBusy = false))
}

function onCreateBlur() {
  // Clicking away keeps a typed name; an untouched default is dropped
  if (!p.creating) return
  if (createValue.value.trim() && createValue.value !== 'New folder') commitCreate()
  else p.creating = false
}

// ---- delete (remote) ----

async function confirmDelete(entries: FileEntry[]) {
  if (p.kind !== 'ssh' || !entries.length) return
  if (ui.settings.confirmDeleteFile) {
    const dirs = entries.filter((e) => e.kind === 'dir').length
    const one = entries.length === 1 ? entries[0] : null
    const ok = await dialogs.confirm({
      title: one
        ? `Delete “${one.name}”?`
        : `Delete ${entries.length} items?`,
      message:
        (dirs
          ? `${dirs === 1 && one ? 'The folder and everything inside it' : 'Folders and everything inside them'} will be deleted from ${p.hostLabel}.`
          : `${one ? 'This file' : 'These files'} will be deleted from ${p.hostLabel}.`) + ' This can’t be undone.',
      confirmLabel: 'Delete',
      danger: true,
    })
    if (!ok) return
  }
  await deleteEntries(p, entries)
  focusList()
}

// ---- side switching ----

function chooseLocal() {
  pickerOpen.value = false
  if (p.kind !== 'local') void bindLocal(p)
}

function chooseHost(id: number) {
  pickerOpen.value = false
  if (p.hostId === id && p.sessionId !== null) return
  void bindHost(p, id)
}

function retryConnect() {
  if (p.hostId !== null) void bindHost(p, p.hostId)
}

// ---- menus ----

function placesMenu(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const items: MenuItem[] = [
    ...localRoots.places.map((pl) => ({
      label: pl.label,
      icon: pl.label === 'Home' ? House : FolderOpen,
      action: () => void navigate(p, pl.path),
    })),
    ...localRoots.drives.map((d) => ({ label: d, icon: HardDrive, action: () => void navigate(p, d) })),
  ]
  menu.value = { x: r.left, y: r.bottom + 6, items }
}

function openInTerminal() {
  if (p.sessionId === null) return
  sessions.write(p.sessionId, `cd ${shellQuote(p.path)}\n`)
  sessions.activeId = p.sessionId
  ui.openView(`session:${p.sessionId}`)
}

function paneItems(): MenuItem[] {
  const items: MenuItem[] = []
  if (ready.value) {
    items.push({ label: 'New folder', icon: FolderPlus, action: startCreate })
    if (p.kind === 'ssh') items.push({ label: 'Upload files…', icon: Upload, action: () => void pickUpload() })
    items.push(
      { label: 'Refresh', icon: RefreshCw, action: () => void refresh(p) },
      {
        label: props.showHidden ? 'Hide hidden files' : 'Show hidden files',
        icon: props.showHidden ? EyeOff : Eye,
        action: () => emit('toggle-hidden'),
      },
      { label: 'Copy path', icon: ClipboardCopy, action: () => void copyText(p.path, 'Path copied') },
    )
    if (p.kind === 'ssh') items.push({ label: 'Open in terminal', icon: SquareTerminal, action: openInTerminal })
  }
  items.push({ label: 'Switch location…', icon: ArrowLeftRight, action: () => (pickerOpen.value = true) })
  if (p.kind === 'ssh' && p.hostId !== null) {
    items.push({ label: 'Close this server', icon: Unplug, action: () => unbind(p) })
  }
  return items
}

function moreMenu(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu.value = { x: r.right - 200, y: r.bottom + 6, items: paneItems() }
}

function bgMenu(e: MouseEvent) {
  emit('activate')
  clearSelection(p)
  menu.value = { x: e.clientX, y: e.clientY, items: paneItems() }
}

function rowMenu(e: MouseEvent, entry: FileEntry) {
  emit('activate')
  if (!selectedSet.value.has(entry.name)) setSelection(p, [entry.name])
  const sel = selection.value
  const single = sel.length === 1 ? sel[0] : null
  const items: MenuItem[] = []
  if (single && (single.kind === 'dir' || single.kind === 'symlink')) {
    items.push({ label: 'Open', icon: FolderOpen, action: () => openDir(p, single.name) })
  }
  if (send.value.files.length) {
    items.push({
      label: send.value.files.length > 1 ? `${send.value.label} (${send.value.files.length})` : send.value.label,
      icon: p.kind === 'ssh' && props.other.kind === 'local' ? ArrowDownToLine : ArrowUpFromLine,
      disabled: !!send.value.reason,
      action: sendSelection,
    })
  }
  if (single) items.push({ label: 'Rename', icon: Pencil, action: () => startRename(single) })
  items.push({
    label: single ? 'Copy path' : `Copy ${sel.length} paths`,
    icon: ClipboardCopy,
    action: () =>
      void copyText(
        sel.map((x) => (p.kind === 'ssh' ? `${p.path.replace(/\/$/, '')}/${x.name}` : `${p.path.replace(/\\$/, '')}\\${x.name}`)).join('\n'),
        single ? 'Path copied' : 'Paths copied',
      ),
  })
  if (p.kind === 'ssh') {
    items.push({ label: sel.length > 1 ? `Delete ${sel.length} items` : 'Delete', icon: Trash2, danger: true, action: () => void confirmDelete(sel) })
  }
  menu.value = { x: e.clientX, y: e.clientY, items }
}

async function pickUpload() {
  const paths = await api.pickFiles(`Upload to ${p.hostLabel}`)
  if (paths.length) emit('upload', paths)
}

// Selection changes from the outside (e.g. after navigating) keep the row visible
watch(() => p.cursor, scrollCursorIntoView)

// Picker closes on any press outside it (no scrim: it would be clipped to
// this pane, which is a size container)
const switcherWrap = ref<HTMLElement | null>(null)
function onDocPointerDown(e: PointerEvent) {
  if (!switcherWrap.value?.contains(e.target as Node)) pickerOpen.value = false
}
watch(pickerOpen, (open) => {
  if (open) document.addEventListener('pointerdown', onDocPointerDown, true)
  else document.removeEventListener('pointerdown', onDocPointerDown, true)
})
onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onDocPointerDown, true)
  clearTimeout(typeTimer)
})

defineExpose({ focusPath: () => pathBar.value?.beginEdit(), focusList, startCreate })
</script>

<template>
  <section
    class="pane"
    :class="{ active, chooser: showChooser }"
    :aria-label="`${paneLabel(pane)} files`"
    @pointerdown.capture="emit('activate')"
    @mouseup="onMouseUp"
  >
    <!-- ===== Header: where this side points ===== -->
    <header class="pane-head">
      <div ref="switcherWrap" class="switcher-wrap">
        <button
          class="switcher"
          :class="{ open: pickerOpen }"
          :aria-expanded="pickerOpen"
          aria-haspopup="listbox"
          :title="pane.kind === 'ssh' && host ? `${host.username}@${host.address}` : 'Choose what this side shows'"
          @click="pickerOpen = !pickerOpen"
        >
          <span v-if="pane.kind === 'local'" class="sw-tile"><Laptop :size="14" :stroke-width="1.6" aria-hidden="true" /></span>
          <PlanetAvatar v-else-if="host" :host="host" :size="22" />
          <span class="sw-label text-ellipsis">{{ showChooser ? 'Choose a server' : paneLabel(pane) }}</span>
          <span
            v-if="pane.kind === 'ssh' && pane.hostId !== null"
            class="status-dot"
            :class="pane.connecting ? 'connecting' : pane.sessionId !== null ? 'connected' : 'error'"
            aria-hidden="true"
          ></span>
          <ChevronDown :size="13" :stroke-width="1.75" class="sw-chev" aria-hidden="true" />
        </button>

        <Transition name="pop">
          <div v-if="pickerOpen" class="picker-pop surface">
            <SidePicker
              :current-host-id="pane.kind === 'ssh' ? pane.hostId : null"
              :current-local="pane.kind === 'local'"
              @local="chooseLocal"
              @host="chooseHost"
              @close="pickerOpen = false"
            />
          </div>
        </Transition>
      </div>

      <span class="head-spacer"></span>

      <SearchField
        v-if="ready && pane.entries.length"
        v-model="pane.filter"
        class="pane-filter"
        :label="`Filter ${paneLabel(pane)}`"
        placeholder="Filter"
      />
      <button class="tool" :aria-label="`More actions for ${paneLabel(pane)}`" title="More" @click="moreMenu">
        <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
      </button>
    </header>

    <!-- ===== No server chosen: the picker is the page ===== -->
    <div v-if="showChooser" class="chooser-body">
      <div class="chooser-card">
        <p class="chooser-title">Browse a server</p>
        <p class="chooser-sub">Pick a host to open its files here. Connected hosts open instantly.</p>
        <SidePicker :show-local="true" :autofocus="false" @local="chooseLocal" @host="chooseHost" />
      </div>
    </div>

    <!-- ===== Connecting / failed ===== -->
    <div v-else-if="pane.kind === 'ssh' && pane.connecting" class="pane-state">
      <span class="spinner" aria-hidden="true"></span>
      <p class="state-title">Connecting to {{ pane.hostLabel }}…</p>
      <p class="state-sub">Answer any password or host-key prompt to continue.</p>
      <button class="btn btn-ghost" @click="unbind(pane)">Cancel</button>
    </div>

    <div v-else-if="pane.kind === 'ssh' && pane.connectError" class="pane-state">
      <span class="state-icon warn"><Unplug :size="20" :stroke-width="1.5" aria-hidden="true" /></span>
      <p class="state-title">{{ pane.connectError }}</p>
      <div class="state-actions">
        <button class="btn btn-primary" @click="retryConnect">Reconnect</button>
        <button class="btn btn-secondary" @click="pickerOpen = true">Choose another</button>
      </div>
    </div>

    <template v-else>
      <!-- ===== Navigation ===== -->
      <div class="nav-row">
        <div class="nav-group" role="group" aria-label="History">
          <button class="tool" aria-label="Back" title="Back (Alt+←)" :disabled="!pane.back.length" @click="goBack(pane)">
            <ArrowLeft :size="15" :stroke-width="1.6" aria-hidden="true" />
          </button>
          <button class="tool" aria-label="Forward" title="Forward (Alt+→)" :disabled="!pane.fwd.length" @click="goForward(pane)">
            <ArrowRight :size="15" :stroke-width="1.6" aria-hidden="true" />
          </button>
          <button class="tool" aria-label="Up one level" title="Up (Backspace)" :disabled="!parentOf(pane.kind, pane.path)" @click="goUp">
            <ArrowUp :size="15" :stroke-width="1.6" aria-hidden="true" />
          </button>
        </div>

        <button
          v-if="pane.kind === 'local'"
          class="tool places"
          aria-label="Places and drives"
          title="Places and drives"
          @click="placesMenu"
        >
          <HardDrive :size="14" :stroke-width="1.6" aria-hidden="true" />
          <ChevronDown :size="11" :stroke-width="2" aria-hidden="true" />
        </button>

        <PathBar
          ref="pathBar"
          :path="pane.path"
          :crumbs="crumbsFor(pane.kind, pane.path)"
          :list-fn="ready ? (dir: string) => listDir(pane, dir) : null"
          :drives="pane.kind === 'local' ? localRoots.drives : []"
          :sep="pane.kind === 'local' ? '\\' : '/'"
          :placeholder="pane.kind === 'local' ? 'Type a path…' : 'Type a remote path…'"
          @navigate="(path: string) => navigate(pane, path)"
        />

        <button class="tool" aria-label="Refresh" title="Refresh (F5)" @click="refresh(pane)">
          <RefreshCw :size="14" :stroke-width="1.6" :class="{ spinning: pane.loading }" aria-hidden="true" />
        </button>
        <button class="tool" aria-label="New folder" title="New folder (Ctrl+Shift+N)" @click="startCreate">
          <FolderPlus :size="15" :stroke-width="1.6" aria-hidden="true" />
        </button>
        <button v-if="pane.kind === 'ssh'" class="tool" aria-label="Upload files" title="Upload files…" @click="pickUpload">
          <Upload :size="14" :stroke-width="1.6" aria-hidden="true" />
        </button>
      </div>

      <!-- ===== Column header (sortable) ===== -->
      <div class="cols">
        <span></span>
        <button class="col" :class="{ on: pane.sortKey === 'name' }" @click="setSort('name')">
          Name
          <component :is="pane.sortDir === 1 ? ChevronUp : ChevronDown" v-if="pane.sortKey === 'name'" :size="11" :stroke-width="2" aria-hidden="true" />
        </button>
        <button class="col col-mtime" :class="{ on: pane.sortKey === 'mtime' }" @click="setSort('mtime')">
          Modified
          <component :is="pane.sortDir === 1 ? ChevronUp : ChevronDown" v-if="pane.sortKey === 'mtime'" :size="11" :stroke-width="2" aria-hidden="true" />
        </button>
        <button class="col col-size" :class="{ on: pane.sortKey === 'size' }" @click="setSort('size')">
          <component :is="pane.sortDir === 1 ? ChevronUp : ChevronDown" v-if="pane.sortKey === 'size'" :size="11" :stroke-width="2" aria-hidden="true" />
          Size
        </button>
        <span v-if="pane.kind === 'ssh'" class="col col-perm">Permissions</span>
      </div>

      <!-- ===== Files ===== -->
      <div
        ref="listEl"
        class="file-list"
        :class="{ 'drop-pane': dropZone && dropZone.folder === null, invalid: dropZone && !dropZone.ok }"
        role="listbox"
        aria-multiselectable="true"
        tabindex="0"
        :aria-label="`${paneLabel(pane)} — ${pane.path}`"
        :aria-activedescendant="cursorIndex >= 0 ? `p${pane.idx}-r${cursorIndex}` : undefined"
        :data-drop-pane="pane.idx"
        @keydown="onListKey"
        @click="onBlankClick"
        @contextmenu.prevent="bgMenu"
      >
        <!-- New folder, inline at the top -->
        <div v-if="pane.creating" class="row editing" @click.stop>
          <span class="row-icon dir"><FolderPlus :size="16" :stroke-width="1.5" aria-hidden="true" /></span>
          <input
            ref="createEl"
            v-model="createValue"
            class="row-input mono"
            spellcheck="false"
            aria-label="New folder name"
            @keydown.enter.prevent="commitCreate"
            @keydown.esc.prevent.stop="pane.creating = false; focusList()"
            @blur="onCreateBlur"
          />
        </div>

        <div v-if="pane.loading && !pane.entries.length" class="pane-state">
          <span class="spinner" aria-label="Loading"></span>
        </div>

        <div v-else-if="pane.error" class="pane-state">
          <p class="state-title">Couldn’t open this folder</p>
          <p class="state-error mono">{{ pane.error }}</p>
          <div class="state-actions">
            <button class="btn btn-secondary" @click.stop="refresh(pane)">Try again</button>
            <button v-if="parentOf(pane.kind, pane.path)" class="btn btn-ghost" @click.stop="goUp">Go up</button>
          </div>
        </div>

        <div v-else-if="!visible.length && !pane.creating" class="pane-state quiet">
          <template v-if="pane.filter.trim()">
            <p class="state-title">No matches for “{{ pane.filter.trim() }}”</p>
            <button class="btn btn-ghost" @click.stop="pane.filter = ''">Clear filter</button>
          </template>
          <template v-else>
            <span class="state-icon"><FolderOpen :size="20" :stroke-width="1.5" aria-hidden="true" /></span>
            <p class="state-title">This folder is empty</p>
            <p class="state-sub">
              {{ hiddenCount ? `${plural(hiddenCount, 'hidden item')} — ` : '' }}{{ pane.kind === 'ssh' ? 'Drop files here to upload.' : 'Drag files here from the other side.' }}
            </p>
            <button v-if="hiddenCount" class="btn btn-ghost" @click.stop="emit('toggle-hidden')">Show hidden</button>
          </template>
        </div>

        <template v-else>
          <div
            v-for="(entry, i) in visible"
            :id="`p${pane.idx}-r${i}`"
            :key="entry.name"
            class="row"
            :class="{
              selected: selectedSet.has(entry.name),
              cursor: pane.cursor === entry.name,
              hidden: isHidden(entry),
              'drop-folder': dropZone?.folder === entry.name,
            }"
            role="option"
            :aria-selected="selectedSet.has(entry.name)"
            :data-row="entry.name"
            :data-drop-name="entry.kind === 'dir' ? entry.name : undefined"
            :data-drop-kind="entry.kind"
            :title="pane.kind === 'ssh' && entry.permissions !== null ? `${entry.name}\n${fmtPerms(entry.permissions)} (${octalPerms(entry.permissions)})` : entry.name"
            @pointerdown="onRowPointerDown($event, entry)"
            @click.stop="onRowClick($event, entry)"
            @dblclick.stop="onRowDblClick(entry)"
            @contextmenu.prevent.stop="rowMenu($event, entry)"
          >
            <span class="row-icon" :class="fileVisual(entry).tone">
              <component :is="fileVisual(entry).icon" :size="16" :stroke-width="1.5" aria-hidden="true" />
            </span>
            <input
              v-if="pane.renaming === entry.name"
              :ref="setRenameEl"
              v-model="renameValue"
              class="row-input mono"
              spellcheck="false"
              :aria-label="`Rename ${entry.name}`"
              @click.stop
              @dblclick.stop
              @keydown.enter.prevent="commitRename"
              @keydown.esc.prevent.stop="cancelRename"
              @blur="commitRename"
            />
            <span v-else class="row-name text-ellipsis">{{ entry.name }}</span>
            <span class="row-mtime">{{ fmtModified(entry.mtime) }}</span>
            <span class="row-size">{{ entry.kind === 'file' ? fmtBytes(entry.size) : '' }}</span>
            <span v-if="pane.kind === 'ssh'" class="row-perm mono">{{ fmtPerms(entry.permissions) }}</span>
          </div>
          <div class="rows-end" aria-hidden="true"></div>
        </template>
      </div>

      <!-- Drop target cue: what will happen, in words -->
      <Transition name="drop-cue">
        <div v-if="dropZone && dropZone.folder === null" class="drop-cue" :class="{ invalid: !dropZone.ok }">
          <component :is="dropZone.ok ? ArrowDownToLine : Unplug" :size="15" :stroke-width="1.6" aria-hidden="true" />
          <span class="text-ellipsis">{{ dropZone.label }}</span>
        </div>
      </Transition>

      <!-- ===== Status bar: counts + the one obvious next action ===== -->
      <footer class="status">
        <span class="status-info">
          <template v-if="pane.selected.length">
            <strong>{{ pane.selected.length }}</strong> of {{ visible.length }} selected<template v-if="selectionBytes"> · {{ fmtBytes(selectionBytes) }}</template>
          </template>
          <template v-else>{{ plural(visible.length, 'item') }}</template>
          <button v-if="hiddenCount" class="status-link" @click="emit('toggle-hidden')">
            {{ showHidden ? 'Hide hidden' : `${hiddenCount} hidden` }}
          </button>
        </span>

        <Transition name="send" mode="out-in">
          <span v-if="send.reason" key="hint" class="send-hint">{{ send.reason }}</span>
          <button
            v-else-if="send.files.length"
            key="send"
            class="btn btn-primary send-btn"
            :title="`${send.label} — you can also drag, or double-click a file`"
            @click="sendSelection"
          >
            <ArrowLeft v-if="!towardRight" :size="13" :stroke-width="2" aria-hidden="true" />
            <span class="text-ellipsis">{{ send.files.length > 1 ? `${send.label} · ${send.files.length}` : send.label }}</span>
            <ArrowRight v-if="towardRight" :size="13" :stroke-width="2" aria-hidden="true" />
          </button>
        </Transition>
      </footer>
    </template>

    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </section>
</template>

<style scoped>
.pane {
  position: relative;
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
  container-type: inline-size;
}

/* ---- header ---- */

.pane-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  height: 46px;
  padding: 0 var(--space-2) 0 var(--space-3);
  flex-shrink: 0;
}

.switcher-wrap {
  position: relative;
  min-width: 0;
}

.switcher {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  max-width: 260px;
  height: 32px;
  padding: 0 8px 0 5px;
  border-radius: var(--radius-md);
  font-size: var(--text-md);
  font-weight: 600;
  letter-spacing: -0.005em;
  color: var(--text-1);
  transition: background-color var(--dur-hover) ease, transform var(--dur-press) ease-out;
}

.switcher:hover,
.switcher.open {
  background: var(--bg-hover);
}

.switcher:active {
  transform: scale(0.97);
}

.switcher :deep(.planet-avatar) {
  border-radius: 6px;
}

.sw-tile {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.sw-label {
  min-width: 0;
}

.switcher .status-dot {
  width: 6px;
  height: 6px;
}

.sw-chev {
  color: var(--text-3);
  flex-shrink: 0;
  transition: transform var(--dur-hover) var(--ease-out);
}

.switcher.open .sw-chev {
  transform: rotate(180deg);
}

.picker-pop {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  z-index: 31;
  display: flex;
  width: 340px;
  max-width: calc(100vw - 80px);
  max-height: 420px;
  padding: var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-3);
  /* scales out of the switcher it belongs to */
  transform-origin: top left;
}

.picker-pop > .side-picker {
  flex: 1;
}

/* The pane with focus sits above its neighbour, so its popovers can overlap it */
.pane.active {
  z-index: 1;
}

.head-spacer {
  flex: 1;
}

.pane-filter {
  width: min(180px, 34cqi);
  height: 28px;
}

/* ---- toolbar buttons ---- */

.tool {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 2px;
  min-width: 28px;
  height: 28px;
  padding: 0 4px;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  flex-shrink: 0;
  transition:
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.tool:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-1);
}

.tool:active:not(:disabled) {
  transform: scale(0.94);
}

.tool:disabled {
  opacity: 0.3;
  cursor: default;
}

.spinning {
  animation: spin 0.7s linear infinite;
}

.nav-row {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 0 var(--space-2) 6px;
  flex-shrink: 0;
}

.nav-group {
  display: flex;
  gap: 1px;
  margin-right: 2px;
}

.places {
  padding-inline: 5px;
  color: var(--text-2);
}

/* ---- columns ---- */

.cols,
.row {
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) 128px 70px;
  align-items: center;
  gap: 10px;
  padding: 0 14px 0 12px;
}

.pane:has(.col-perm) .cols,
.pane:has(.col-perm) .row {
  grid-template-columns: 22px minmax(0, 1fr) 128px 70px 82px;
}

.cols {
  height: 26px;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.col {
  display: inline-flex;
  align-items: center;
  gap: 3px;
  min-width: 0;
  height: 22px;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-4);
  text-align: left;
  border-radius: 4px;
  transition: color var(--dur-hover) ease;
}

button.col:hover,
.col.on {
  color: var(--text-2);
}

.col-size {
  justify-content: flex-end;
}

.col-perm {
  cursor: default;
}

/* Narrow panes drop the least important columns first */
@container (max-width: 560px) {
  .pane .cols,
  .pane .row,
  .pane:has(.col-perm) .cols,
  .pane:has(.col-perm) .row {
    grid-template-columns: 22px minmax(0, 1fr) 112px 64px;
  }
  .col-perm,
  .row-perm {
    display: none;
  }
}

@container (max-width: 400px) {
  .pane .cols,
  .pane .row,
  .pane:has(.col-perm) .cols,
  .pane:has(.col-perm) .row {
    grid-template-columns: 22px minmax(0, 1fr) 64px;
  }
  .col-mtime,
  .row-mtime {
    display: none;
  }
}

/* ---- file list ---- */

.file-list {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 6px var(--space-3);
  outline: none;
  transition: background-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.file-list:focus-visible {
  box-shadow: inset 0 0 0 2px var(--border-focus);
}

.row {
  height: 30px;
  padding-inline: 6px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  color: var(--text-1);
  cursor: default;
}

@media (hover: hover) and (pointer: fine) {
  .row:hover:not(.selected) {
    background: color-mix(in oklch, var(--bg-hover) 75%, transparent);
  }
}

/* Selection is accent while this side has focus, quiet grey otherwise —
   so it's always clear which side the keyboard is driving */
.row.selected {
  background: var(--bg-active);
}

.pane.active .row.selected {
  background: var(--accent-bg-hover);
}

.pane.active .file-list:focus .row.cursor:not(.selected) {
  box-shadow: inset 0 0 0 1px var(--border-strong);
}

/* Adjacent selected rows merge into one block */
.row.selected:has(+ .row.selected) {
  border-bottom-left-radius: 0;
  border-bottom-right-radius: 0;
}

.row.selected + .row.selected {
  border-top-left-radius: 0;
  border-top-right-radius: 0;
}

.row.hidden .row-name,
.row.hidden .row-icon {
  opacity: 0.55;
}

.row.drop-folder {
  background: var(--accent-bg-hover);
  box-shadow: inset 0 0 0 1.5px var(--accent);
}

.row-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
}

.row-icon.dir { color: var(--accent); }
.row-icon.link { color: var(--warning); }
.row-icon.image { color: oklch(0.7 0.14 330); }
.row-icon.video,
.row-icon.audio { color: oklch(0.7 0.13 300); }
.row-icon.archive { color: oklch(0.72 0.13 60); }
.row-icon.code,
.row-icon.script { color: oklch(0.72 0.12 200); }
.row-icon.data { color: oklch(0.74 0.12 110); }
.row-icon.key { color: var(--warning); }

.row-name {
  min-width: 0;
}

.row-mtime,
.row-size,
.row-perm {
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.row-size {
  text-align: right;
}

.row-perm {
  color: var(--text-4);
  letter-spacing: 0.02em;
}

.row.editing {
  background: var(--accent-bg);
}

.row-input {
  min-width: 0;
  height: 24px;
  padding: 0 7px;
  margin-left: -7px;
  border: 1px solid var(--accent);
  border-radius: 5px;
  background: var(--bg-input);
  box-shadow: 0 0 0 3px var(--accent-bg);
  outline: none;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.rows-end {
  min-height: 24px;
}

/* ---- drop cue ---- */

.file-list.drop-pane {
  background: color-mix(in oklch, var(--accent-bg) 60%, transparent);
  box-shadow: inset 0 0 0 2px var(--accent);
}

.file-list.drop-pane.invalid {
  background: transparent;
  box-shadow: inset 0 0 0 2px var(--border-strong);
}

.drop-cue {
  position: absolute;
  bottom: 54px;
  left: 50%;
  translate: -50% 0;
  z-index: 3;
  display: flex;
  align-items: center;
  gap: 8px;
  max-width: calc(100% - 32px);
  padding: 8px 14px;
  border-radius: 999px;
  background: var(--accent);
  color: var(--accent-text);
  font-size: var(--text-sm);
  font-weight: 500;
  box-shadow: var(--shadow-pop);
  pointer-events: none;
  white-space: nowrap;
}

.drop-cue.invalid {
  background: var(--bg-surface-3);
  color: var(--text-2);
}

.drop-cue-enter-active {
  transition: transform 180ms var(--ease-out), opacity 180ms var(--ease-out);
}

.drop-cue-leave-active {
  transition: opacity 100ms ease;
}

.drop-cue-enter-from {
  transform: translateY(6px) scale(0.97);
  opacity: 0;
}

.drop-cue-leave-to {
  opacity: 0;
}

/* ---- states ---- */

.pane-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: var(--space-7) var(--space-4);
  text-align: center;
  color: var(--text-3);
  flex: 1;
}

.pane-state.quiet {
  padding-top: var(--space-6);
}

.state-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 42px;
  height: 42px;
  margin-bottom: 4px;
  border-radius: 12px;
  background: var(--bg-surface-2);
  border: 1px solid var(--border);
  color: var(--text-3);
}

.state-icon.warn {
  color: var(--warning);
}

.state-title {
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--text-1);
}

.state-sub {
  max-width: 36ch;
  font-size: var(--text-sm);
}

.state-error {
  max-width: 48ch;
  font-size: var(--text-xs);
  color: var(--danger);
  word-break: break-word;
  user-select: text;
}

.state-actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-2);
}

.pane-state > .btn {
  margin-top: var(--space-2);
}

/* ---- chooser ---- */

.chooser-body {
  flex: 1;
  min-height: 0;
  display: flex;
  justify-content: center;
  padding: var(--space-5) var(--space-4);
  overflow: hidden;
  border-top: 1px solid var(--border);
}

.chooser-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  width: 100%;
  max-width: 420px;
  min-height: 0;
}

.chooser-title {
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  color: var(--text-1);
}

.chooser-sub {
  margin-bottom: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-3);
}

/* ---- status bar ---- */

.status {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  min-height: 40px;
  padding: 5px var(--space-2) 5px var(--space-3);
  border-top: 1px solid var(--border);
  flex-shrink: 0;
}

.status-info {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.status-info strong {
  font-weight: 600;
  color: var(--text-1);
}

.status-link {
  padding: 2px 6px;
  border-radius: 999px;
  font-size: var(--text-xs);
  color: var(--text-3);
  background: var(--bg-surface-2);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.status-link:hover {
  background: var(--bg-surface-3);
  color: var(--text-1);
}

.send-btn {
  min-width: 0;
  max-width: 62%;
  padding: 5px 11px;
  font-size: var(--text-sm);
}

.send-hint {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--text-4);
}

.send-enter-active {
  transition: transform 160ms var(--ease-out), opacity 160ms var(--ease-out);
}

.send-leave-active {
  transition: opacity 100ms ease;
}

.send-enter-from {
  transform: translateX(v-bind("towardRight ? '-6px' : '6px'"));
  opacity: 0;
}

.send-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .send-enter-from,
  .drop-cue-enter-from {
    transform: none;
  }
}
</style>
