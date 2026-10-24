<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
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
  MonitorSmartphone,
} from 'lucide-vue-next'
import { api } from '../services/api'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useHostsStore } from '../stores/hosts'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import type { FileEntry } from '../types'

const sessions = useSessionsStore()
const hosts = useHostsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

// ---- remote pane state ----
const remoteSessionId = ref<number | null>(null)
const remotePath = ref('/')
const remoteEntries = ref<FileEntry[]>([])
const remoteLoading = ref(false)
const remoteError = ref<string | null>(null)
const remoteSelected = ref<string | null>(null)
const remoteRenaming = ref<string | null>(null)
const remoteRenameValue = ref('')
const remoteNewFolder = ref(false)
const remoteNewFolderName = ref('')
const remoteFilter = ref('')

// ---- local pane state ----
const localDrives = ref<string[]>([])
const localPath = ref('')
const localEntries = ref<FileEntry[]>([])
const localLoading = ref(false)
const localError = ref<string | null>(null)
const localSelected = ref<string | null>(null)
const localFilter = ref('')

const remoteHostLabel = computed(() => {
  const tab = sessions.tabs.find((t) => t.id === remoteSessionId.value)
  return tab?.label ?? ''
})

const connectedChoices = computed(() =>
  sessions.tabs.filter((t) => t.status === 'connected' && t.kind === 'ssh'),
)

const remoteFiltered = computed(() =>
  sortEntries(
    remoteEntries.value.filter((e) =>
      e.name.toLowerCase().includes(remoteFilter.value.trim().toLowerCase()),
    ),
  ),
)

const localFiltered = computed(() =>
  sortEntries(
    localEntries.value.filter((e) =>
      e.name.toLowerCase().includes(localFilter.value.trim().toLowerCase()),
    ),
  ),
)

function sortEntries(list: FileEntry[]): FileEntry[] {
  return [...list].sort((a, b) => {
    const da = a.kind === 'dir' ? 0 : 1
    const db = b.kind === 'dir' ? 0 : 1
    if (da !== db) return da - db
    return a.name.localeCompare(b.name)
  })
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

function kindLabel(e: FileEntry): string {
  if (e.kind === 'dir') return 'folder'
  if (e.kind === 'symlink') return 'symlink'
  const ext = e.name.includes('.') ? e.name.split('.').pop()! : 'file'
  return ext.toLowerCase()
}

// ---- remote ----
function handleRemoteError(e: unknown) {
  const msg = errorText(e)
  // The picked session died — fall back to the host picker
  if (msg.includes('Session not found') || msg.includes('Not connected')) {
    remoteSessionId.value = null
    remoteError.value = null
  } else {
    remoteError.value = msg
  }
}

async function openRemote() {
  const sessionId = remoteSessionId.value
  if (sessionId === null) return
  remoteLoading.value = true
  remoteError.value = null
  try {
    await api.sftpOpen(sessionId)
    const result = await api.sftpList(sessionId, remotePath.value)
    remoteEntries.value = result
  } catch (e) {
    handleRemoteError(e)
  } finally {
    remoteLoading.value = false
  }
}

async function remoteGo(p: string) {
  if (remoteSessionId.value === null) return
  remoteLoading.value = true
  remoteError.value = null
  try {
    const result = await api.sftpList(remoteSessionId.value, p)
    remotePath.value = p
    remoteEntries.value = result
    remoteSelected.value = null
  } catch (e) {
    handleRemoteError(e)
  } finally {
    remoteLoading.value = false
  }
}

function remoteParent(p: string): string {
  if (p === '/' || p === '') return '/'
  const idx = p.lastIndexOf('/')
  return idx <= 0 ? '/' : p.slice(0, idx)
}

function join(dir: string, name: string): string {
  return dir === '/' ? `/${name}` : `${dir}/${name}`
}

const remoteCrumbs = computed(() => {
  const parts = remotePath.value.split('/').filter(Boolean)
  return [{ name: '/', path: '/' }, ...parts.map((p, i) => ({
    name: p,
    path: '/' + parts.slice(0, i + 1).join('/'),
  }))]
})

const localCrumbs = computed(() => {
  const p = localPath.value
  if (!p) return []
  const norm = p.replace(/\//g, '\\')
  const parts = norm.split('\\').filter(Boolean)
  // First segment is the drive, e.g. "C:"
  return parts.map((part, i) => ({
    name: part,
    path: i === 0 ? `${part}\\` : parts.slice(0, i + 1).join('\\') + '\\',
  }))
})

const selectedRemoteEntry = computed(
  () => remoteEntries.value.find((e) => e.name === remoteSelected.value) ?? null,
)
const selectedLocalEntry = computed(
  () => localEntries.value.find((e) => e.name === localSelected.value) ?? null,
)

async function remoteMkdir() {
  const name = remoteNewFolderName.value.trim()
  if (!name || remoteSessionId.value === null) return
  try {
    await api.sftpMkdir(remoteSessionId.value, join(remotePath.value, name))
    remoteNewFolder.value = false
    remoteNewFolderName.value = ''
    await remoteGo(remotePath.value)
  } catch (e) {
    ui.toast('error', `mkdir failed: ${errorText(e)}`)
  }
}

async function remoteRemove(entry: FileEntry) {
  const ok = await dialogs.confirm({
    title: entry.kind === 'dir' ? `Delete folder “${entry.name}”?` : `Delete “${entry.name}”?`,
    message:
      entry.kind === 'dir'
        ? 'The folder and everything inside it will be deleted on the remote server. This cannot be undone.'
        : 'This file will be deleted on the remote server. This cannot be undone.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok || remoteSessionId.value === null) return
  try {
    await api.sftpRemove(remoteSessionId.value, join(remotePath.value, entry.name), entry.kind === 'dir')
    await remoteGo(remotePath.value)
  } catch (e) {
    ui.toast('error', `Delete failed: ${errorText(e)}`)
  }
}

async function remoteCommitRename() {
  const from = remoteRenaming.value
  const to = remoteRenameValue.value.trim()
  remoteRenaming.value = null
  if (!from || !to || from === to || remoteSessionId.value === null) return
  try {
    await api.sftpRename(remoteSessionId.value, join(remotePath.value, from), join(remotePath.value, to))
    await remoteGo(remotePath.value)
  } catch (e) {
    ui.toast('error', `Rename failed: ${errorText(e)}`)
  }
}

async function uploadSelected() {
  const local = selectedLocalEntry.value
  if (!local || local.kind !== 'file' || remoteSessionId.value === null) return
  const localPathFull = joinLocal(localPath.value, local.name)
  try {
    const id = await api.sftpUpload(remoteSessionId.value, localPathFull, join(remotePath.value, local.name))
    sessions.trackTransfer({
      id, sessionId: remoteSessionId.value, kind: 'upload',
      fileName: local.name, size: local.size, transferred: 0, done: false,
    })
  } catch (e) {
    ui.toast('error', `Upload failed: ${errorText(e)}`)
  }
}

async function downloadSelected() {
  const remote = selectedRemoteEntry.value
  if (!remote || remote.kind !== 'file' || remoteSessionId.value === null) return
  if (!localPath.value) {
    ui.toast('info', 'Open a local folder first to choose where the file goes')
    return
  }
  try {
    const id = await api.sftpDownload(
      remoteSessionId.value,
      join(remotePath.value, remote.name),
      joinLocal(localPath.value, remote.name),
    )
    sessions.trackTransfer({
      id, sessionId: remoteSessionId.value, kind: 'download',
      fileName: remote.name, size: remote.size, transferred: 0, done: false,
    })
  } catch (e) {
    ui.toast('error', `Download failed: ${errorText(e)}`)
  }
}

function joinLocal(dir: string, name: string): string {
  const sep = dir.includes('\\') ? '\\' : '/'
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`
}

// ---- local ----
async function localGo(p: string) {
  localLoading.value = true
  localError.value = null
  try {
    const result = await api.localList(p)
    localPath.value = p
    localEntries.value = result
    localSelected.value = null
  } catch (e) {
    localError.value = errorText(e)
  } finally {
    localLoading.value = false
  }
}

function localUp() {
  const parent = localParent(localPath.value)
  if (parent) void localGo(parent)
}

function localParent(p: string): string | null {
  const norm = p.replace(/\//g, '\\')
  if (/^[A-Z]:\\?$/i.test(norm)) return null // drive root
  const idx = norm.lastIndexOf('\\')
  if (idx <= 2) return norm.slice(0, 3) // "C:\"
  return norm.slice(0, idx)
}

function localOpen(entry: FileEntry) {
  if (entry.kind === 'dir') {
    void localGo(joinLocal(localPath.value, entry.name))
  }
}

// ---- host picking ----
function pickConnected(sessionId: number) {
  remoteSessionId.value = sessionId
  void openRemote()
}

function pickHostAndConnect(hostId: number) {
  // If a connected tab exists for the host, reuse it
  const existing = sessions.tabs.find((t) => t.hostId === hostId && t.status === 'connected')
  if (existing) {
    pickConnected(existing.id)
    return
  }
  // Otherwise watch for its session to come up
  const stop = sessions.$subscribe(() => {
    const tab = sessions.tabs.find((t) => t.hostId === hostId && t.status === 'connected')
    if (tab) {
      remoteSessionId.value = tab.id
      void openRemote()
      stop()
    }
  })
  void sessions.connect(hostId)
}

onMounted(async () => {
  try {
    localDrives.value = await api.localDrives()
  } catch (e) {
    localError.value = errorText(e)
    localDrives.value = []
  }
  const firstDrive = localDrives.value[0]
  if (firstDrive) void localGo(firstDrive)

  // Preselect a connected session if there is exactly one
  if (connectedChoices.value.length === 1) {
    pickConnected(connectedChoices.value[0].id)
  }
})
</script>

<template>
  <div class="sftp-view">
    <!-- Local pane -->
    <section class="pane">
      <header class="pane-header">
        <span class="pane-title">
          <MonitorSmartphone :size="14" :stroke-width="1.5" aria-hidden="true" />
          Local
        </span>
        <input v-model="localFilter" class="pane-filter" type="text" placeholder="Filter" spellcheck="false" aria-label="Filter local files" />
      </header>

      <nav class="crumbs" aria-label="Local path">
        <button v-if="localParent(localPath)" class="crumb-btn" aria-label="Up" title="Up" @click="localUp">
          <ArrowUp :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <template v-for="(c, i) in localCrumbs" :key="c.path">
          <button v-if="i < localCrumbs.length - 1" class="crumb text-ellipsis" @click="localGo(c.path)">
            {{ c.name }}
          </button>
          <span v-else class="crumb current text-ellipsis" aria-current="location">{{ c.name }}</span>
          <span v-if="i < localCrumbs.length - 1" class="crumb-sep">›</span>
        </template>
      </nav>

      <div class="file-list" role="listbox" aria-label="Local files">
        <div v-if="localLoading && !localEntries.length" class="pane-state">
          <span class="spinner" aria-hidden="true"></span>
        </div>
        <div v-else-if="localError" class="pane-state">
          <p class="pane-error mono">{{ localError }}</p>
          <button class="btn btn-secondary" @click="localGo(localPath)">Retry</button>
        </div>

        <template v-else>
          <div
            v-for="entry in localFiltered"
            :key="entry.name"
            class="file-row"
            :class="{ selected: localSelected === entry.name }"
            role="option"
            :aria-selected="localSelected === entry.name"
            @click="localSelected = entry.name"
            @dblclick="localOpen(entry)"
          >
            <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" :class="['file-icon', entry.kind]" aria-hidden="true" />
            <span class="file-name text-ellipsis">{{ entry.name }}</span>
            <span class="file-size">{{ formatSize(entry) }}</span>
            <span class="file-kind">{{ kindLabel(entry) }}</span>
          </div>
        </template>
      </div>
    </section>

    <!-- Remote pane -->
    <section class="pane remote">
      <header class="pane-header">
        <span class="pane-title" :title="remoteHostLabel">
          {{ remoteSessionId !== null ? remoteHostLabel || 'Remote' : 'Remote' }}
        </span>
        <input v-model="remoteFilter" class="pane-filter" type="text" placeholder="Filter" spellcheck="false" aria-label="Filter remote files" />
      </header>

      <!-- No host picked yet -->
      <div v-if="remoteSessionId === null" class="pane-state grow">
        <p class="pick-lead">Connect to a saved host to manage files with SFTP.</p>
        <div class="pick-list">
          <button
            v-for="choice in connectedChoices"
            :key="choice.id"
            class="btn btn-secondary pick-btn"
            @click="pickConnected(choice.id)"
          >
            {{ choice.label }}
          </button>
        </div>
        <div v-if="hosts.hosts.length" class="pick-others">
          <span class="section-label">Or connect a host</span>
          <div class="pick-list">
            <button
              v-for="h in hosts.hosts.slice(0, 8)"
              :key="h.id"
              class="btn btn-ghost pick-btn"
              @click="pickHostAndConnect(h.id)"
            >
              {{ h.label }}
            </button>
          </div>
        </div>
      </div>

      <template v-else>
        <nav class="crumbs" aria-label="Remote path">
          <button class="crumb-btn" aria-label="Up" title="Up" @click="remoteGo(remoteParent(remotePath))">
            <ArrowUp :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="crumb-btn" aria-label="Refresh" title="Refresh" @click="openRemote">
            <RefreshCw :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="crumb-btn" aria-label="New folder" title="New folder" @click="remoteNewFolder = true; remoteNewFolderName = ''">
            <FilePlus2 :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <template v-for="(c, i) in remoteCrumbs" :key="c.path">
            <button v-if="i < remoteCrumbs.length - 1" class="crumb text-ellipsis" @click="remoteGo(c.path)">
              {{ c.name }}
            </button>
            <span v-else class="crumb current text-ellipsis" aria-current="location">{{ c.name }}</span>
            <span v-if="i < remoteCrumbs.length - 1" class="crumb-sep">›</span>
          </template>
        </nav>

        <div v-if="remoteNewFolder" class="inline-form">
          <input
            v-model="remoteNewFolderName"
            class="field-input mono"
            type="text"
            placeholder="folder name"
            spellcheck="false"
            @keydown.enter="remoteMkdir"
            @keydown.esc="remoteNewFolder = false"
          />
          <button class="btn-icon" aria-label="Create folder" @click="remoteMkdir">
            <Check :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="btn-icon" aria-label="Cancel" @click="remoteNewFolder = false">
            <X :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>

        <div class="file-list" role="listbox" aria-label="Remote files">
          <div v-if="remoteLoading && !remoteEntries.length" class="pane-state">
            <span class="spinner" aria-hidden="true"></span>
          </div>
          <div v-else-if="remoteError" class="pane-state">
            <p class="pane-error mono">{{ remoteError }}</p>
            <button class="btn btn-secondary" @click="openRemote">Retry</button>
          </div>
          <div v-else-if="!remoteFiltered.length" class="pane-state">
            <p>Empty folder</p>
          </div>

          <template v-else>
            <div
              v-for="entry in remoteFiltered"
              :key="entry.name"
              class="file-row"
              :class="{ selected: remoteSelected === entry.name }"
              role="option"
              :aria-selected="remoteSelected === entry.name"
              @click="remoteSelected = entry.name"
              @dblclick="entry.kind === 'dir' && remoteGo(join(remotePath, entry.name))"
            >
              <template v-if="remoteRenaming === entry.name">
                <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" aria-hidden="true" />
                <input
                  v-model="remoteRenameValue"
                  class="field-input mono rename-input"
                  spellcheck="false"
                  @keydown.enter="remoteCommitRename"
                  @keydown.esc="remoteRenaming = null"
                  @blur="remoteCommitRename"
                />
              </template>
              <template v-else>
                <component :is="iconFor(entry.kind)" :size="15" :stroke-width="1.5" :class="['file-icon', entry.kind]" aria-hidden="true" />
                <span class="file-name">
                  <span class="text-ellipsis">{{ entry.name }}</span>
                  <span v-if="entry.permissions !== null" class="perm mono">{{ (entry.permissions >>> 0).toString(8).padStart(4, '0') }}</span>
                </span>
                <span class="file-size">{{ formatSize(entry) }}</span>
                <span class="file-kind">{{ kindLabel(entry) }}</span>
                <span class="file-actions" @click.stop>
                  <button v-if="entry.kind === 'file'" class="btn-icon" :aria-label="`Download ${entry.name}`" title="Download to local pane" @click="downloadSelected">
                    <Download :size="13" :stroke-width="1.5" aria-hidden="true" />
                  </button>
                  <button class="btn-icon" :aria-label="`Rename ${entry.name}`" title="Rename" @click="remoteRenaming = entry.name; remoteRenameValue = entry.name">
                    <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
                  </button>
                  <button class="btn-icon file-delete" :aria-label="`Delete ${entry.name}`" title="Delete" @click="remoteRemove(entry)">
                    <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
                  </button>
                </span>
              </template>
            </div>
          </template>
        </div>

        <!-- Transfer action + transfers -->
        <div class="remote-foot">
          <button
            class="btn btn-primary upload-btn"
            :disabled="!selectedLocalEntry || selectedLocalEntry.kind !== 'file'"
            :title="selectedLocalEntry ? `Upload ${selectedLocalEntry.name}` : 'Select a file in the local pane first'"
            @click="uploadSelected"
          >
            <Upload :size="14" :stroke-width="1.5" aria-hidden="true" />
            Upload to {{ remotePath === '/' ? '/' : remotePath.split('/').pop() }}
          </button>

          <div v-if="sessions.transfers.filter((t) => t.sessionId === remoteSessionId).length" class="transfers">
            <div v-for="t in sessions.transfers.filter((t) => t.sessionId === remoteSessionId)" :key="t.id" class="transfer">
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
                <span v-else-if="t.done" class="transfer-done">Done</span>
              </div>
            </div>
          </div>
        </div>
      </template>
    </section>
  </div>
</template>

<style scoped>
.sftp-view {
  display: flex;
  height: 100%;
  min-height: 0;
}

.pane {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  border-right: 1px solid var(--border);
}

.pane.remote {
  border-right: none;
}

.pane-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.pane-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  min-width: 0;
}

.pane-filter {
  margin-left: auto;
  width: 140px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 4px 9px;
  font-size: var(--text-sm);
}

.pane-filter:focus {
  outline: none;
  border-color: var(--accent);
}

.crumbs {
  display: flex;
  align-items: center;
  gap: 3px;
  padding: var(--space-2) var(--space-4);
  border-bottom: 1px solid var(--border);
  overflow: hidden;
  flex-shrink: 0;
  min-height: 38px;
}

.crumb-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.crumb-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.crumb {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  color: var(--text-3);
  padding: 3px 6px;
  border-radius: 4px;
  max-width: 170px;
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
}

.file-list {
  flex: 1;
  overflow-y: auto;
  min-height: 0;
  padding: var(--space-1);
}

.file-row {
  display: grid;
  grid-template-columns: 20px minmax(0, 1fr) 80px 64px auto;
  align-items: center;
  gap: 10px;
  padding: 6px 10px;
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

.pane-state.grow {
  flex: 1;
}

.pane-error {
  font-size: var(--text-xs);
  color: var(--danger);
  word-break: break-all;
  user-select: text;
}

.pick-lead {
  max-width: 40ch;
  line-height: var(--leading-normal);
}

.pick-list {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
  justify-content: center;
  max-width: 480px;
}

.pick-others {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  align-items: center;
  margin-top: var(--space-4);
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

.remote-foot {
  border-top: 1px solid var(--border);
  padding: var(--space-2) var(--space-3);
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  max-height: 200px;
  overflow-y: auto;
}

.upload-btn {
  align-self: flex-start;
}

.transfers {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
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
</style>
