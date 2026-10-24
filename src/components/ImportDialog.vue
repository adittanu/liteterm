<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { FolderOpen, KeyRound, FileWarning } from 'lucide-vue-next'
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

watch(
  () => dialogs.showImport,
  (open_) => {
    if (open_) open()
  },
)

onMounted(() => {
  if (dialogs.showImport) open()
})
</script>

<template>
  <BaseModal
    v-if="dialogs.showImport"
    title="Import from ~/.ssh/config"
    :width="520"
    @close="dialogs.showImport = false"
  >
    <div class="import">
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
          Create one (or connect with <code class="mono">ssh</code> once) and come back.
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

          <label class="check known-check">
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
  </BaseModal>
</template>

<style scoped>
.import {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  min-height: 200px;
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
  color: var(--text-4);
  flex-shrink: 0;
}

.known-check {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
  padding: var(--space-1) var(--space-1);
}

.known-check input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
}

.known-check .mono {
  font-size: var(--text-xs);
}

.import-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
