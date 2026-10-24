<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import {
  ChevronRight,
  ArrowUp,
  RefreshCw,
  File as FileIcon,
  Folder,
  Link2,
  FilePlus2,
  Upload,
  Download,
  Trash2,
  Pencil,
  X,
  Check,
} from 'lucide-vue-next'
import { api } from '../services/api'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { FileEntry } from '../types'

const props = defineProps<{ sessionId: number }>()

const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const path = ref('/')
const entries = ref<FileEntry[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)
const selected = ref<string | null>(null)
const renaming = ref<string | null>(null)
const renameValue = ref('')
const newFolderMode = ref(false)
const newFolderName = ref('')
const renameInput = ref<HTMLInputElement | null>(null)

const crumbs = computed(() => {
  const parts = path.value.split('/').filter(Boolean)
  return [{ name: '/', path: '/' }, ...parts.map((p, i) => ({
    name: p,
    path: '/' + parts.slice(0, i + 1).join('/'),
  }))]
})

const sorted = computed(() => {
  return [...entries.value].sort((a, b) => {
    const da = a.kind === 'dir' ? 0 : 1
    const db = b.kind === 'dir' ? 0 : 1
    if (da !== db) return da - db
    return a.name.localeCompare(b.name)
  })
})

const selectedEntry = computed(() => entries.value.find((e) => e.name === selected.value) ?? null)

function parentPath(p: string): string {
  if (p === '/' || p === '') return '/'
  const idx = p.lastIndexOf('/')
  return idx <= 0 ? '/' : p.slice(0, idx)
}

function join(dir: string, name: string): string {
  return dir === '/' ? `/${name}` : `${dir}/${name}`
}

async function list(p: string) {
  loading.value = true
  loadError.value = null
  try {
    // The SFTP channel is opened lazily and is idempotent backend-side
    await api.sftpOpen(props.sessionId)
    const result = await api.sftpList(props.sessionId, p)
    path.value = p
    entries.value = result
    selected.value = null
  } catch (e) {
    loadError.value = errorText(e)
  } finally {
    loading.value = false
  }
}

function iconFor(kind: FileEntry['kind']) {
  if (kind === 'dir') return Folder
  if (kind === 'symlink') return Link2
  return FileIcon
}

function formatSize(e: FileEntry): string {
  if (e.kind !== 'file') return '—'
  const s = e.size
  if (s < 1024) return `${s} B`
  if (s < 1024 * 1024) return `${(s / 1024).toFixed(1)} KB`
  if (s < 1024 * 1024 * 1024) return `${(s / (1024 * 1024)).toFixed(1)} MB`
  return `${(s / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

function formatTime(ts: number): string {
  if (!ts) return '—'
  const d = new Date(ts * 1000)
  return d.toLocaleString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric',
    hour: '2-digit', minute: '2-digit',
  })
}

function open(entry: FileEntry) {
  if (entry.kind === 'dir') void list(join(path.value, entry.name))
  else if (entry.kind === 'file') void download(entry)
}

async function download(entry: FileEntry) {
  const local = await api.pickSaveFile('Save file as', entry.name)
  if (!local) return
  try {
    const id = await api.sftpDownload(props.sessionId, join(path.value, entry.name), local)
    sessions.trackTransfer({
      id, sessionId: props.sessionId, kind: 'download',
      fileName: entry.name, size: entry.size, transferred: 0, done: false,
    })
  } catch (e) {
    ui.toast('error', `Download failed: ${errorText(e)}`)
  }
}

async function upload() {
  const local = await api.pickFile('Choose file to upload')
  if (!local) return
  const name = local.split(/[\\/]/).pop() ?? 'file'
  try {
    const id = await api.sftpUpload(props.sessionId, local, join(path.value, name))
    sessions.trackTransfer({
      id, sessionId: props.sessionId, kind: 'upload',
      fileName: name, size: 0, transferred: 0, done: false,
    })
  } catch (e) {
    ui.toast('error', `Upload failed: ${errorText(e)}`)
  }
}

async function mkdir() {
  const name = newFolderName.value.trim()
  if (!name) return
  try {
    await api.sftpMkdir(props.sessionId, join(path.value, name))
    newFolderMode.value = false
    newFolderName.value = ''
    await list(path.value)
  } catch (e) {
    ui.toast('error', `mkdir failed: ${errorText(e)}`)
  }
}

function startRename(entry: FileEntry) {
  renaming.value = entry.name
  renameValue.value = entry.name
  void nextTick(() => renameInput.value?.select())
}

async function commitRename() {
  const from = renaming.value
  const to = renameValue.value.trim()
  renaming.value = null
  if (!from || !to || from === to) return
  try {
    await api.sftpRename(props.sessionId, join(path.value, from), join(path.value, to))
    await list(path.value)
  } catch (e) {
    ui.toast('error', `Rename failed: ${errorText(e)}`)
  }
}

async function remove(entry: FileEntry) {
  // Explicit confirmation — destructive on the server
  const ok = await dialogs.confirm({
    title: entry.kind === 'dir' ? `Delete folder “${entry.name}”?` : `Delete “${entry.name}”?`,
    message:
      entry.kind === 'dir'
        ? 'The folder and everything inside it will be deleted on the remote server. This cannot be undone.'
        : 'This file will be deleted on the remote server. This cannot be undone.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  try {
    await api.sftpRemove(props.sessionId, join(path.value, entry.name), entry.kind === 'dir')
    await list(path.value)
  } catch (e) {
    ui.toast('error', `Delete failed: ${errorText(e)}`)
  }
}

function onListKeydown(e: KeyboardEvent) {
  if (renaming.value || newFolderMode.value) return
  if (e.key === 'F2' && selectedEntry.value) startRename(selectedEntry.value)
  else if (e.key === 'Delete' && selectedEntry.value) void remove(selectedEntry.value)
  else if (e.key === 'Enter' && selectedEntry.value) open(selectedEntry.value)
  else if (e.key === 'Backspace') void list(parentPath(path.value))
  else if (e.key === 'Escape') selected.value = null
}

onMounted(() => {
  void list('/')
})
</script>

<template>
  <aside class="sftp" aria-label="Remote files">
    <header class="sftp-header">
      <nav class="crumbs" aria-label="Remote path">
        <template v-for="(c, i) in crumbs" :key="c.path">
          <button v-if="i < crumbs.length - 1" class="crumb text-ellipsis" @click="list(c.path)">
            {{ c.name }}
          </button>
          <span v-else class="crumb current text-ellipsis" aria-current="location">{{ c.name }}</span>
          <ChevronRight v-if="i < crumbs.length - 1" :size="12" :stroke-width="1.5" class="crumb-sep" aria-hidden="true" />
        </template>
      </nav>
      <div class="sftp-tools">
        <button class="btn-icon" aria-label="Go up" title="Up (Backspace)" @click="list(parentPath(path))">
          <ArrowUp :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <button class="btn-icon" aria-label="Refresh" title="Refresh" @click="list(path)">
          <RefreshCw :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <button class="btn-icon" aria-label="New folder" title="New folder" @click="newFolderMode = true; newFolderName = ''">
          <FilePlus2 :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <button class="btn-icon" aria-label="Upload file" title="Upload file" @click="upload">
          <Upload :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>
    </header>

    <div v-if="newFolderMode" class="inline-form">
      <input
        v-model="newFolderName"
        class="field-input mono"
        type="text"
        placeholder="folder name"
        spellcheck="false"
        autofocus
        @keydown.enter="mkdir"
        @keydown.esc="newFolderMode = false"
      />
      <button class="btn-icon" aria-label="Create folder" @click="mkdir">
        <Check :size="14" :stroke-width="1.5" aria-hidden="true" />
      </button>
      <button class="btn-icon" aria-label="Cancel" @click="newFolderMode = false">
        <X :size="14" :stroke-width="1.5" aria-hidden="true" />
      </button>
    </div>

    <div
      class="file-list"
      tabindex="0"
      role="listbox"
      aria-label="Remote directory contents"
      @keydown="onListKeydown"
    >
      <div v-if="loading && !entries.length" class="sftp-state">
        <span class="spinner" aria-hidden="true"></span>
      </div>
      <div v-else-if="loadError" class="sftp-state">
        <p class="sftp-error mono">{{ loadError }}</p>
        <button class="btn btn-secondary" @click="list(path)">Retry</button>
      </div>
      <div v-else-if="!sorted.length" class="sftp-state">
        <p>Empty folder</p>
      </div>

      <div
        v-for="entry in sorted"
        v-else
        :key="entry.name"
        class="file-row"
        :class="{ selected: selected === entry.name }"
        role="option"
        :aria-selected="selected === entry.name"
        @click="selected = entry.name"
        @dblclick="open(entry)"
      >
        <template v-if="renaming === entry.name">
          <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" aria-hidden="true" />
          <input
            ref="renameInput"
            v-model="renameValue"
            class="field-input mono rename-input"
            spellcheck="false"
            @keydown.enter="commitRename"
            @keydown.esc="renaming = null"
            @blur="commitRename"
          />
        </template>
        <template v-else>
          <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" :class="['file-icon', entry.kind]" aria-hidden="true" />
          <span class="file-name text-ellipsis">{{ entry.name }}</span>
          <span class="file-size">{{ formatSize(entry) }}</span>
          <span class="file-time">{{ formatTime(entry.mtime) }}</span>
          <span class="file-actions">
            <button v-if="entry.kind === 'file'" class="btn-icon" :aria-label="`Download ${entry.name}`" title="Download" @click.stop="download(entry)">
              <Download :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Rename ${entry.name}`" title="Rename (F2)" @click.stop="startRename(entry)">
              <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon file-delete" :aria-label="`Delete ${entry.name}`" title="Delete (Del)" @click.stop="remove(entry)">
              <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </span>
        </template>
      </div>
    </div>

    <!-- Transfers -->
    <div v-if="sessions.transfers.length" class="transfers">
      <div class="section-label transfers-label">Transfers</div>
      <div v-for="t in sessions.transfers" :key="t.id" class="transfer">
        <div class="transfer-row">
          <span class="transfer-kind" :class="t.kind">{{ t.kind === 'upload' ? '↑' : '↓' }}</span>
          <span class="transfer-name text-ellipsis">{{ t.fileName }}</span>
          <span v-if="!t.done && !t.error" class="spinner small" aria-hidden="true"></span>
          <button class="btn-icon" :aria-label="t.done ? 'Dismiss' : 'Cancel transfer'" @click="t.done || t.error ? sessions.removeTransfer(t.id) : sessions.cancelTransfer(t.id)">
            <X :size="12" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
        <div v-if="!t.error" class="transfer-bar" role="progressbar" :aria-valuenow="Math.round((t.transferred / Math.max(t.size, 1)) * 100)" aria-valuemin="0" aria-valuemax="100">
          <div class="transfer-fill" :class="{ indeterminate: !t.done && t.size === 0, complete: t.done }" :style="{ width: t.size ? `${(t.transferred / t.size) * 100}%` : undefined }"></div>
        </div>
        <div class="transfer-meta">
          <span v-if="t.error" class="transfer-error">{{ t.error }}</span>
          <span v-else-if="t.done" class="transfer-done">Done</span>
          <span v-else-if="t.size">{{ formatSize({ name: '', kind: 'file', size: t.transferred, mtime: 0, permissions: null }) }} / {{ formatSize({ name: '', kind: 'file', size: t.size, mtime: 0, permissions: null }) }}</span>
          <span v-else>Starting…</span>
        </div>
      </div>
    </div>
  </aside>
</template>

<style scoped>
.sftp {
  display: flex;
  flex-direction: column;
  width: 360px;
  flex-shrink: 0;
  border-left: 1px solid var(--border);
  background: var(--bg);
  min-height: 0;
}

.sftp-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}

.crumbs {
  display: flex;
  align-items: center;
  gap: 2px;
  min-width: 0;
  overflow: hidden;
}

.crumb {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  color: var(--text-3);
  padding: 2px 5px;
  border-radius: 4px;
  max-width: 180px;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

button.crumb:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.crumb.current {
  color: var(--text-1);
  font-weight: 500;
}

.crumb-sep {
  color: var(--text-4);
  flex-shrink: 0;
}

.sftp-tools {
  display: flex;
  gap: 2px;
}

.inline-form {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-2) var(--space-3);
  border-bottom: 1px solid var(--border);
}

.inline-form .field-input {
  flex: 1;
  padding: 5px 8px;
  font-size: var(--text-sm);
}

.file-list {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  outline: none;
  padding: var(--space-1);
}

.file-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
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

.file-icon.dir { color: var(--accent); }
.file-icon.symlink { color: var(--warning); }
.file-icon.file { color: var(--text-3); }

.file-name {
  flex: 1;
  min-width: 0;
  color: var(--text-1);
}

.rename-input {
  flex: 1;
  padding: 2px 6px !important;
  font-size: var(--text-sm) !important;
}

.file-size {
  width: 72px;
  text-align: right;
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
  flex-shrink: 0;
}

.file-time {
  width: 108px;
  font-size: var(--text-xs);
  color: var(--text-4);
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
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

.sftp-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-7) var(--space-4);
  color: var(--text-3);
  font-size: var(--text-sm);
}

.sftp-error {
  font-size: var(--text-xs);
  color: var(--danger);
  word-break: break-all;
  text-align: center;
  user-select: text;
}

.transfers {
  border-top: 1px solid var(--border);
  padding: var(--space-2) var(--space-3) var(--space-3);
  max-height: 180px;
  overflow-y: auto;
}

.transfers-label {
  margin-bottom: var(--space-2);
}

.transfer + .transfer {
  margin-top: var(--space-2);
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
  margin: 5px 0 3px;
  overflow: hidden;
}

.transfer-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 2px;
  /* width updates are discrete chunk events (10/s) — no transition needed;
     animating width would also violate the transform/opacity-only motion rule */
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
  transition: none;
}

.transfer-meta {
  display: flex;
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}

.transfer-done { color: var(--success); }
.transfer-error { color: var(--danger); user-select: text; word-break: break-all; }
</style>
