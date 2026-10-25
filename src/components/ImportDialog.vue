<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  FolderOpen,
  KeyRound,
  FileWarning,
  FileJson,
  FileUp,
  Lock,
  RotateCcw,
  Terminal,
} from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { api } from '../services/api'
import { errorText } from '../stores/sessions'
import type { ParsedHost } from '../types'

const dialogs = useDialogsStore()
const hosts = useHostsStore()
const ui = useUiStore()

type Source = 'ssh' | 'file'
const source = ref<Source>('ssh')

// ---- ~/.ssh/config source ----
const loading = ref(false)
const loadError = ref<string | null>(null)
const configFound = ref(true)
const knownHostCount = ref(0)
const importKnown = ref(true)
const allHosts = ref<ParsedHost[]>([])
const selected = ref<Set<string>>(new Set())

const grouped = computed(() => {
  const groups = new Map<string, ParsedHost[]>()
  for (const h of allHosts.value) {
    const key = h.group ?? 'Ungrouped'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(h)
  }
  return [...groups.entries()]
})

const selectedHosts = computed(() => allHosts.value.filter((h) => selected.value.has(h.alias)))
const newCount = computed(() => selectedHosts.value.filter((h) => !h.exists).length)

function toggle(alias: string) {
  const next = new Set(selected.value)
  if (next.has(alias)) next.delete(alias)
  else next.add(alias)
  selected.value = next
}

function open() {
  source.value = 'ssh'
  loading.value = true
  loadError.value = null
  api
    .importSshPreview()
    .then((preview) => {
      configFound.value = preview.config_found
      knownHostCount.value = preview.known_host_count
      allHosts.value = preview.hosts
      selected.value = new Set(preview.hosts.map((h) => h.alias))
    })
    .catch((e) => {
      loadError.value = errorText(e)
    })
    .finally(() => {
      loading.value = false
    })
}

async function apply() {
  if (!selectedHosts.value.length && !importKnown.value) return
  try {
    const result = await api.importSshApply(selectedHosts.value, importKnown.value)
    dialogs.showImport = false
    await hosts.load()

    const hostLabel = result.host_ids.length === 1 ? 'host' : 'hosts'
    ui.toast('success', `Imported ${result.host_ids.length} ${hostLabel} and ${result.known_imported} known host keys`, {
      actionLabel: 'Undo',
      action: async () => {
        for (const id of result.host_ids) {
          await hosts.removeHost(id).catch(() => {})
        }
      },
      duration: 8000,
    })
  } catch (e) {
    ui.toast('error', `Import failed: ${errorText(e)}`)
  }
}

// ---- file source (Termius / LiteTerm / ssh config) ----
const fileState = ref<'idle' | 'loading' | 'error' | 'ready'>('idle')
const fileError = ref('')
const fileName = ref('')
const fileFormat = ref('')
const fileHosts = ref<ParsedHost[]>([])
const fileSelected = ref<Set<string>>(new Set())
const savePasswords = ref(true)
const dragging = ref(false)

const keyOf = (h: ParsedHost) => `${h.alias}|${h.hostname}|${h.port}`

const formatLabel = computed(
  () => ({ termius: 'Termius export', liteterm: 'LiteTerm export', 'ssh-config': 'SSH config' })[fileFormat.value] ?? 'File',
)

const fileGrouped = computed(() => {
  const groups = new Map<string, ParsedHost[]>()
  for (const h of fileHosts.value) {
    const key = h.group ?? 'Ungrouped'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(h)
  }
  return [...groups.entries()]
})

const fileSelectedHosts = computed(() => fileHosts.value.filter((h) => fileSelected.value.has(keyOf(h))))
const fileNewCount = computed(() => fileSelectedHosts.value.filter((h) => !h.exists).length)
const hasPasswords = computed(() => fileSelectedHosts.value.some((h) => h.password))
const hasKeyImports = computed(() => fileSelectedHosts.value.some((h) => h.key_pem))

function toggleFile(key: string) {
  const next = new Set(fileSelected.value)
  if (next.has(key)) next.delete(key)
  else next.add(key)
  fileSelected.value = next
}

function resetFile() {
  fileState.value = 'idle'
  fileError.value = ''
  fileName.value = ''
  fileHosts.value = []
  fileSelected.value = new Set()
}

async function loadFile(path: string) {
  source.value = 'file'
  fileState.value = 'loading'
  fileError.value = ''
  try {
    const preview = await api.importFilePreview(path)
    fileName.value = path.split(/[\\/]/).pop() ?? path
    fileFormat.value = preview.format
    fileHosts.value = preview.hosts
    fileSelected.value = new Set(preview.hosts.map(keyOf))
    fileState.value = 'ready'
  } catch (e) {
    fileError.value = errorText(e)
    fileState.value = 'error'
  }
}

async function chooseFile() {
  const path = await api.pickFile('Choose a host export', [
    { name: 'Host exports', extensions: ['json', 'txt'] },
    { name: 'All files', extensions: ['*'] },
  ])
  if (path) loadFile(path)
}

async function applyFile() {
  if (!fileSelectedHosts.value.length) return
  try {
    const result = await api.importFileApply(fileSelectedHosts.value, savePasswords.value)
    dialogs.showImport = false
    await hosts.load()

    const hostLabel = result.host_ids.length === 1 ? 'host' : 'hosts'
    const keyNote = hasKeyImports.value ? ' · keys saved to ~/.ssh/liteterm-keys' : ''
    ui.toast('success', `Imported ${result.host_ids.length} ${hostLabel}${keyNote}`, {
      actionLabel: 'Undo',
      action: async () => {
        for (const id of result.host_ids) {
          await hosts.removeHost(id).catch(() => {})
        }
      },
      duration: 8000,
    })
  } catch (e) {
    ui.toast('error', `Import failed: ${errorText(e)}`)
  }
}

// ---- drag & drop (webview-level, app-wide while the dialog is open) ----
let unlistenDrop: (() => void) | null = null

async function attachDrop() {
  if (unlistenDrop) return
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload as { type: string; paths?: string[] }
      if (source.value !== 'file') return
      if (payload.type === 'enter' || payload.type === 'over') {
        if (payload.paths?.length) dragging.value = true
      } else if (payload.type === 'leave') {
        dragging.value = false
      } else if (payload.type === 'drop') {
        dragging.value = false
        const path = payload.paths?.[0]
        if (path) loadFile(path)
      }
    })
  } catch {
    // drag-drop events unavailable — the file picker still works
  }
}

function detachDrop() {
  unlistenDrop?.()
  unlistenDrop = null
  dragging.value = false
}

watch(
  () => dialogs.showImport,
  (open_) => {
    if (open_) {
      open()
      attachDrop()
    } else {
      detachDrop()
      resetFile()
    }
  },
)

onMounted(() => {
  if (dialogs.showImport) {
    open()
    attachDrop()
  }
})

onBeforeUnmount(detachDrop)
</script>

<template>
  <BaseModal
    v-if="dialogs.showImport"
    title="Import hosts"
    :width="560"
    @close="dialogs.showImport = false"
  >
    <div class="import">
      <div class="seg" role="tablist" aria-label="Import source">
        <button
          role="tab"
          class="seg-item"
          :class="{ on: source === 'ssh' }"
          :aria-selected="source === 'ssh'"
          @click="source = 'ssh'"
        >
          <Terminal :size="13" :stroke-width="1.5" aria-hidden="true" />
          SSH config
        </button>
        <button
          role="tab"
          class="seg-item"
          :class="{ on: source === 'file' }"
          :aria-selected="source === 'file'"
          @click="source = 'file'"
        >
          <FileJson :size="13" :stroke-width="1.5" aria-hidden="true" />
          File
        </button>
      </div>

      <!-- ~/.ssh/config -->
      <div v-if="source === 'ssh'" class="pane">
        <div v-if="loading" class="import-state">
          <span class="spinner" aria-hidden="true"></span>
        </div>

        <div v-else-if="loadError" class="import-state">
          <p class="import-error mono">{{ loadError }}</p>
          <button class="btn btn-secondary" @click="open">Retry</button>
        </div>

        <div v-else-if="!configFound" class="import-state">
          <FileWarning :size="20" :stroke-width="1.5" aria-hidden="true" />
          <p>
            No SSH config found at <code class="mono">~/.ssh/config</code>.<br />
            Create one (or connect with <code class="mono">ssh</code> once) and come back — or
            switch to <strong>File</strong> to import an export from Termius or LiteTerm.
          </p>
        </div>

        <template v-else>
          <div v-if="!allHosts.length" class="import-state">
            <p>No hosts found in your config.</p>
          </div>

          <template v-else>
            <ul class="import-list">
              <li v-for="[group, groupHosts] in grouped" :key="group" class="import-group">
                <div class="section-label group-label">
                  <FolderOpen :size="12" :stroke-width="1.5" aria-hidden="true" />
                  {{ group }}
                </div>
                <label v-for="h in groupHosts" :key="h.alias" class="import-row">
                  <input
                    type="checkbox"
                    :checked="selected.has(h.alias)"
                    @change="toggle(h.alias)"
                  />
                  <span class="row-text">
                    <span class="row-alias">
                      {{ h.alias }}
                      <span v-if="h.exists" class="badge badge-neutral">saved</span>
                    </span>
                    <span class="row-addr mono">
                      {{ h.user ? `${h.user}@` : '' }}{{ h.hostname }}{{ h.port !== 22 ? `:${h.port}` : '' }}
                    </span>
                  </span>
                  <span v-if="h.identity_file" class="row-auth" title="Uses this private key">
                    <KeyRound :size="12" :stroke-width="1.5" aria-hidden="true" />
                  </span>
                </label>
              </li>
            </ul>

            <label class="check-row">
              <input v-model="importKnown" type="checkbox" />
              Trust {{ knownHostCount }} host key{{ knownHostCount === 1 ? '' : 's' }} from
              <code class="mono">known_hosts</code> (skips the first-connect prompt)
            </label>

            <footer class="import-actions">
              <button class="btn btn-ghost" @click="dialogs.showImport = false">Cancel</button>
              <button
                class="btn btn-primary"
                :disabled="(!selectedHosts.length && !importKnown) || (!newCount && !importKnown)"
                @click="apply"
              >
                Import
                <template v-if="newCount">{{ newCount }} host{{ newCount === 1 ? '' : 's' }}</template>
                <template v-else-if="!selectedHosts.length && importKnown">known hosts only</template>
              </button>
            </footer>
          </template>
        </template>
      </div>

      <!-- File -->
      <div v-else class="pane">
        <div
          v-if="fileState === 'idle'"
          class="dropzone"
          :class="{ drag: dragging }"
          role="button"
          tabindex="0"
          aria-label="Choose or drop a host export file"
          @click="chooseFile"
          @keydown.enter="chooseFile"
        >
          <FileUp :size="22" :stroke-width="1.5" aria-hidden="true" />
          <p class="dz-title">{{ dragging ? 'Drop to import' : 'Drop a host export here' }}</p>
          <p class="dz-sub">Termius or LiteTerm JSON · SSH config file</p>
          <button class="btn btn-secondary" @click.stop="chooseFile">Choose file</button>
        </div>

        <div v-else-if="fileState === 'loading'" class="import-state">
          <span class="spinner" aria-hidden="true"></span>
        </div>

        <div v-else-if="fileState === 'error'" class="import-state">
          <FileWarning :size="20" :stroke-width="1.5" aria-hidden="true" />
          <p class="import-error mono">{{ fileError }}</p>
          <button class="btn btn-secondary" @click="resetFile">Choose another file</button>
        </div>

        <template v-else>
          <div class="file-meta">
            <span class="badge badge-neutral">{{ formatLabel }}</span>
            <span class="file-name mono text-ellipsis">{{ fileName }}</span>
            <button class="btn-icon" aria-label="Choose another file" @click="resetFile">
              <RotateCcw :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>

          <div v-if="!fileHosts.length" class="import-state">
            <p>No hosts found in this file.</p>
          </div>

          <template v-else>
            <ul class="import-list">
              <li v-for="[group, groupHosts] in fileGrouped" :key="group" class="import-group">
                <div class="section-label group-label">
                  <FolderOpen :size="12" :stroke-width="1.5" aria-hidden="true" />
                  {{ group }}
                </div>
                <label v-for="h in groupHosts" :key="keyOf(h)" class="import-row">
                  <input
                    type="checkbox"
                    :checked="fileSelected.has(keyOf(h))"
                    @change="toggleFile(keyOf(h))"
                  />
                  <span class="row-text">
                    <span class="row-alias">
                      {{ h.alias }}
                      <span v-if="h.exists" class="badge badge-neutral">saved</span>
                    </span>
                    <span class="row-addr mono">
                      {{ h.user ? `${h.user}@` : '' }}{{ h.hostname }}{{ h.port !== 22 ? `:${h.port}` : '' }}
                    </span>
                  </span>
                  <span class="row-auth">
                    <KeyRound
                      v-if="h.key_pem || h.identity_file"
                      :size="12"
                      :stroke-width="1.5"
                      aria-hidden="true"
                      :title="h.key_pem ? 'Private key embedded — saved to ~/.ssh/liteterm-keys' : 'Uses this private key'"
                    />
                    <Lock
                      v-if="h.password"
                      :size="12"
                      :stroke-width="1.5"
                      class="row-lock"
                      aria-hidden="true"
                      title="Includes a saved password"
                    />
                  </span>
                </label>
              </li>
            </ul>

            <label v-if="hasPasswords" class="check-row">
              <input v-model="savePasswords" type="checkbox" />
              Save {{ fileSelectedHosts.filter((h) => h.password).length }} password{{ fileSelectedHosts.filter((h) => h.password).length === 1 ? '' : 's' }} from the file to your OS keychain
            </label>

            <footer class="import-actions">
              <button class="btn btn-ghost" @click="dialogs.showImport = false">Cancel</button>
              <button class="btn btn-primary" :disabled="!fileNewCount" @click="applyFile">
                Import
                <template v-if="fileNewCount">{{ fileNewCount }} host{{ fileNewCount === 1 ? '' : 's' }}</template>
              </button>
            </footer>
          </template>
        </template>
      </div>
    </div>
  </BaseModal>
</template>

<style scoped>
.import {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  min-height: 220px;
}

/* Segmented source picker */
.seg {
  display: flex;
  gap: 2px;
  padding: 3px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}

.seg-item {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 10px;
  border: none;
  border-radius: calc(var(--radius-md) - 3px);
  background: transparent;
  color: var(--text-3);
  font-size: var(--text-base);
  font-weight: 500;
  cursor: pointer;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.seg-item:hover {
  color: var(--text-2);
}

.seg-item.on {
  background: var(--bg-surface-3);
  color: var(--text-1);
}

.pane {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.import-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  padding: var(--space-6) var(--space-4);
  text-align: center;
  color: var(--text-2);
  font-size: var(--text-base);
  line-height: var(--leading-normal);
}

.import-error {
  color: var(--danger);
  font-size: var(--text-xs);
  word-break: break-all;
  user-select: text;
}

/* Drop zone */
.dropzone {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: var(--space-6) var(--space-4);
  border: 1.5px dashed var(--border-strong);
  border-radius: var(--radius-lg);
  color: var(--text-3);
  text-align: center;
  cursor: pointer;
  transition: border-color var(--dur-hover) ease, background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.dropzone:hover,
.dropzone.drag {
  color: var(--text-2);
}

.dropzone.drag {
  border-color: var(--border-focus);
  background: var(--accent-bg);
}

.dz-title {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
  margin-top: 4px;
}

.dz-sub {
  font-size: var(--text-xs);
  margin-bottom: 6px;
}

/* Parsed-file meta row */
.file-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.file-name {
  flex: 1;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
  direction: rtl;
  text-align: left;
}

.import-list {
  max-height: 320px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding-right: 2px;
}

.group-label {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0 var(--space-1) var(--space-1);
}

.import-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px var(--space-2);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: background-color var(--dur-hover) ease;
}

.import-row:hover {
  background: var(--bg-hover);
}

.import-row input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.row-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
  flex: 1;
}

.row-alias {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
}

.row-addr {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.row-auth {
  display: flex;
  align-items: center;
  gap: 5px;
  color: var(--text-4);
  flex-shrink: 0;
}

.row-lock {
  color: var(--text-4);
}

.check-row {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
  padding: var(--space-1) var(--space-1);
}

.check-row input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.check-row .mono {
  font-size: var(--text-xs);
}

.import-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
