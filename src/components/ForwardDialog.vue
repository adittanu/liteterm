<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { ArrowRight, Laptop, Server, Globe } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import SearchSelect, { type ComboOption } from './SearchSelect.vue'
import { useForwardsStore } from '../stores/extras'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { PortForward } from '../types'

const props = defineProps<{ forward: PortForward | null }>()
const emit = defineEmits<{ close: [] }>()

const forwards = useForwardsStore()
const hosts = useHostsStore()
const ui = useUiStore()

const f = props.forward
const form = reactive({
  name: f?.name ?? '',
  host_id: f?.host_id ?? hosts.hosts[0]?.id ?? 0,
  local_port: f ? String(f.local_port) : '',
  remote_host: f?.remote_host ?? 'localhost',
  remote_port: f ? String(f.remote_port) : '',
  auto_start: f?.auto_start ?? false,
})

// Validate inline, but only once the user has had a chance to type
const touched = reactive({ local_port: false, remote_host: false, remote_port: false })
const submitted = ref(false)
const saving = ref(false)

const hostOptions = computed<ComboOption[]>(() =>
  hosts.hosts.map((h) => ({ value: h.id, label: h.label, hint: h.address })),
)
const hostLabel = computed(() => hosts.hostById(form.host_id)?.label ?? 'host')

function portError(raw: string, required: boolean): string | undefined {
  const v = raw.trim()
  if (!v) return required ? 'Required' : undefined
  if (!/^\d+$/.test(v)) return 'Numbers only'
  const n = Number(v)
  if (n < 1 || n > 65535) return 'Use 1–65535'
  return undefined
}

// Local port may stay empty: it then mirrors the remote port
const effectiveLocal = computed(() => form.local_port.trim() || form.remote_port.trim())
const effectiveName = computed(
  () => form.name.trim() || `${hostLabel.value} ${form.remote_host.trim() || 'localhost'}:${form.remote_port.trim() || '…'}`,
)

const errors = computed(() => ({
  host: form.host_id > 0 ? undefined : 'Choose a host',
  local_port: portError(form.local_port, false),
  remote_host: form.remote_host.trim() ? undefined : 'Required',
  remote_port: portError(form.remote_port, true),
}))

const valid = computed(() => Object.values(errors.value).every((e) => !e))

function show(field: keyof typeof touched): string | undefined {
  return touched[field] || submitted.value ? errors.value[field] : undefined
}

async function save() {
  submitted.value = true
  if (!valid.value || saving.value) return
  saving.value = true
  try {
    await forwards.save({
      id: f?.id,
      host_id: form.host_id,
      name: effectiveName.value,
      local_port: Number(effectiveLocal.value),
      remote_host: form.remote_host.trim(),
      remote_port: Number(form.remote_port),
      auto_start: form.auto_start,
    })
    emit('close')
  } catch (e) {
    ui.toast('error', `Could not save: ${errorText(e)}`)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <BaseModal :title="forward ? 'Edit Forward' : 'New Forward'" :width="500" @close="emit('close')">
    <form class="fwd-form" novalidate @submit.prevent="save">
      <!-- Live route preview: the mental model, drawn -->
      <div class="route" aria-hidden="true">
        <span class="route-node">
          <Laptop :size="14" :stroke-width="1.5" />
          <span class="route-label">This PC</span>
          <span class="route-value mono">:{{ effectiveLocal || '…' }}</span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node via">
          <Server :size="14" :stroke-width="1.5" />
          <span class="route-label">via SSH</span>
          <span class="route-value text-ellipsis">{{ hostLabel }}</span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node">
          <Globe :size="14" :stroke-width="1.5" />
          <span class="route-label">Destination</span>
          <span class="route-value mono text-ellipsis">{{ form.remote_host.trim() || '…' }}:{{ form.remote_port.trim() || '…' }}</span>
        </span>
      </div>

      <div class="field">
        <label class="field-label" for="fw-host">Through host</label>
        <SearchSelect id="fw-host" v-model="form.host_id" :options="hostOptions" aria-label="Through host" />
        <span v-if="submitted && errors.host" class="field-error">{{ errors.host }}</span>
      </div>

      <div class="dest-grid">
        <div class="field">
          <label class="field-label" for="fw-rhost">Destination host</label>
          <input
            id="fw-rhost"
            v-model="form.remote_host"
            class="field-input mono"
            :class="{ invalid: show('remote_host') }"
            type="text"
            placeholder="localhost"
            spellcheck="false"
            autocapitalize="off"
            @blur="touched.remote_host = true"
          />
          <span v-if="show('remote_host')" class="field-error">{{ show('remote_host') }}</span>
          <span v-else class="field-hint">As seen from the SSH host</span>
        </div>
        <div class="field">
          <label class="field-label" for="fw-rport">Port</label>
          <input
            id="fw-rport"
            v-model="form.remote_port"
            class="field-input mono"
            :class="{ invalid: show('remote_port') }"
            type="text"
            inputmode="numeric"
            placeholder="5432"
            @blur="touched.remote_port = true"
          />
          <span v-if="show('remote_port')" class="field-error">{{ show('remote_port') }}</span>
        </div>
      </div>

      <div class="dest-grid">
        <div class="field">
          <label class="field-label" for="fw-name">Name</label>
          <input
            id="fw-name"
            v-model="form.name"
            class="field-input"
            type="text"
            :placeholder="effectiveName"
            spellcheck="false"
          />
        </div>
        <div class="field">
          <label class="field-label" for="fw-lport">Local port</label>
          <input
            id="fw-lport"
            v-model="form.local_port"
            class="field-input mono"
            :class="{ invalid: show('local_port') }"
            type="text"
            inputmode="numeric"
            :placeholder="form.remote_port.trim() || 'same'"
            @blur="touched.local_port = true"
          />
          <span v-if="show('local_port')" class="field-error">{{ show('local_port') }}</span>
          <span v-else class="field-hint">Empty = same</span>
        </div>
      </div>

      <label class="check">
        <input v-model="form.auto_start" type="checkbox" />
        <span>
          Start automatically
          <span class="check-hint">whenever {{ hostLabel }} connects</span>
        </span>
      </label>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="saving">
          {{ saving ? 'Saving…' : forward ? 'Save changes' : 'Add forward' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.fwd-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.route {
  display: flex;
  align-items: stretch;
  gap: 6px;
  padding: 10px;
  border-radius: var(--radius-md);
  background: var(--bg-input);
  border: 1px solid var(--border);
}

.route-node {
  display: grid;
  grid-template-columns: auto 1fr;
  grid-template-rows: auto auto;
  column-gap: 6px;
  align-items: center;
  flex: 1;
  min-width: 0;
  color: var(--text-3);
}

.route-node > svg {
  grid-row: 1 / span 2;
}

.route-node.via {
  color: var(--accent);
}

.route-label {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: var(--tracking-wide);
  text-transform: uppercase;
  color: var(--text-4);
}

.route-value {
  font-size: var(--text-sm);
  color: var(--text-1);
  min-width: 0;
}

.route-arrow {
  align-self: center;
  color: var(--text-4);
  flex-shrink: 0;
}

.dest-grid {
  display: grid;
  grid-template-columns: 1fr 120px;
  gap: var(--space-3);
}

.field-input.invalid {
  border-color: var(--danger);
}

.field-input.invalid:focus {
  box-shadow: 0 0 0 3px var(--danger-bg);
}

.check {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  font-size: var(--text-base);
  color: var(--text-1);
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
  margin-top: 2px;
}

.check-hint {
  color: var(--text-3);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
