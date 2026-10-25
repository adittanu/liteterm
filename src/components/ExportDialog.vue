<script setup lang="ts">
import { computed, ref } from 'vue'
import { FileDown, KeyRound, ShieldAlert } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { api } from '../services/api'
import { errorText } from '../stores/sessions'

const dialogs = useDialogsStore()
const hosts = useHostsStore()
const ui = useUiStore()

const includePasswords = ref(false)
const busy = ref(false)

const hostCount = computed(() => hosts.hosts.length)
const groupCount = computed(() => hosts.groups.length)
const savedPasswordCount = computed(() => hosts.hosts.filter((h) => h.has_password).length)
const keyCount = computed(() => hosts.hosts.filter((h) => h.auth_method === 'key').length)

async function run() {
  busy.value = true
  try {
    const path = await api.pickSaveFile('Export hosts', 'liteterm-hosts.json')
    if (!path) return
    const result = await api.exportHosts(path, includePasswords.value)
    dialogs.showExport = false
    ui.toast('success', `Exported ${result.hostCount} hosts to ${result.path.split(/[\\/]/).pop() ?? result.path}`)
  } catch (e) {
    ui.toast('error', `Export failed: ${errorText(e)}`)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <BaseModal
    v-if="dialogs.showExport"
    title="Export hosts"
    :width="460"
    @close="dialogs.showExport = false"
  >
    <div class="export">
      <p class="summary">
        Exports <strong>{{ hostCount }} {{ hostCount === 1 ? 'host' : 'hosts' }}</strong> across
        {{ groupCount }} {{ groupCount === 1 ? 'group' : 'groups' }} as a LiteTerm JSON file you
        can import on another machine.
      </p>

      <div v-if="savedPasswordCount" class="option" :class="{ warn: includePasswords }">
        <label class="check-row">
          <input v-model="includePasswords" type="checkbox" />
          Include {{ savedPasswordCount }} saved password{{ savedPasswordCount === 1 ? '' : 's' }}
        </label>
        <p class="option-note">
          <ShieldAlert :size="12" :stroke-width="1.5" aria-hidden="true" />
          <template v-if="includePasswords">
            Passwords are written to the file in plain text — share it carefully.
          </template>
          <template v-else>
            Without this, passwords stay only in your OS keychain.
          </template>
        </p>
      </div>

      <p v-if="keyCount" class="key-note">
        <KeyRound :size="12" :stroke-width="1.5" aria-hidden="true" />
        {{ keyCount }} key-based {{ keyCount === 1 ? 'host' : 'hosts' }} — key files are referenced
        by path, not copied into the export.
      </p>

      <footer class="export-actions">
        <button class="btn btn-ghost" :disabled="busy" @click="dialogs.showExport = false">Cancel</button>
        <button class="btn btn-primary" :disabled="busy || !hostCount" @click="run">
          <FileDown v-if="!busy" :size="14" :stroke-width="1.5" aria-hidden="true" />
          <span v-else class="spinner" aria-hidden="true"></span>
          Export
        </button>
      </footer>
    </div>
  </BaseModal>
</template>

<style scoped>
.export {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.summary {
  color: var(--text-2);
  font-size: var(--text-base);
  line-height: var(--leading-normal);
}

.summary strong {
  color: var(--text-1);
}

.option {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  transition: border-color var(--dur-hover) ease, background-color var(--dur-hover) ease;
}

.option.warn {
  background: var(--danger-bg);
}

.option-note {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  padding-left: 21px;
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: var(--leading-normal);
}

.option.warn .option-note {
  color: var(--danger);
}

.key-note {
  display: flex;
  align-items: flex-start;
  gap: 6px;
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: var(--leading-normal);
}

.check-row {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
}

.check-row input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
  flex-shrink: 0;
}

.export-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
