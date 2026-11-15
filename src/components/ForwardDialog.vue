<script setup lang="ts">
import { computed, reactive, ref } from 'vue'
import { ArrowLeft, ArrowRight, AppWindow, Laptop, Server, Globe } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import SearchSelect, { type ComboOption } from './SearchSelect.vue'
import PvText from './PvText.vue'
import { useForwardsStore } from '../stores/extras'
import { useHostsStore } from '../stores/hosts'
import { useTeamsStore } from '../stores/teams'
import { useUiStore } from '../stores/ui'
import { usePrivacyStore } from '../stores/privacy'
import { errorText } from '../stores/sessions'
import { thisComputer, thisComputerLower } from '../utils/platform'
import { DEFAULT_SOCKS_PORT, FORWARD_KINDS } from '../utils/forwards'
import { isSsh } from '../utils/hostProtocol'
import type { ForwardKind, PortForward } from '../types'

const props = defineProps<{ forward: PortForward | null }>()
const emit = defineEmits<{ close: [] }>()

const forwards = useForwardsStore()
const hosts = useHostsStore()
const teams = useTeamsStore()
const ui = useUiStore()
const pv = usePrivacyStore()

const f = props.forward
const kind = ref<ForwardKind>(f?.kind ?? 'local')
// One form for every type. What each port means depends on the type:
// local: local_port here, remote_host:remote_port seen from the server;
// remote: remote_port on the server, remote_host:local_port seen from here;
// dynamic: local_port only.
const form = reactive({
  name: f?.name ?? '',
  host_id: f?.host_id ?? hosts.hosts.find((h) => isSsh(h) && teams.canEdit(h.team_id))?.id ?? 0,
  local_port: f ? String(f.local_port) : '',
  remote_host: f?.remote_host || 'localhost',
  remote_port: f && f.kind !== 'dynamic' ? String(f.remote_port) : '',
  auto_start: f?.auto_start ?? false,
})

// Validate inline, but only once the user has had a chance to type
const touched = reactive({ local_port: false, remote_host: false, remote_port: false })
const submitted = ref(false)
const saving = ref(false)

const hostOptions = computed<ComboOption[]>(() =>
  // Tunnels ride on SSH; a Telnet or serial host can't carry one. A forward
  // is in its host's vault: an existing one keeps to its vault, and a team
  // this account only views gets no new ones.
  hosts.hosts
    .filter((h) => isSsh(h) && (f ? h.team_id === f.team_id : teams.canEdit(h.team_id)))
    .map((h) => ({ value: h.id, label: h.label, hint: h.address })),
)
const hostLabel = computed(() => hosts.hostById(form.host_id)?.label ?? 'host')
const kindInfo = computed(() => FORWARD_KINDS.find((k) => k.kind === kind.value)!)

function portError(raw: string, required: boolean): string | undefined {
  const v = raw.trim()
  if (!v) return required ? 'Required' : undefined
  if (!/^\d+$/.test(v)) return 'Numbers only'
  const n = Number(v)
  if (n < 1 || n > 65535) return 'Use 1–65535'
  return undefined
}

// An empty second port mirrors the first, the way `ssh -L 5432:db:5432`
// usually reads: local mirrors the remote port, remote's destination port
// mirrors the server port. A SOCKS proxy without a port takes 1080.
const effectiveLocal = computed(() => {
  const own = form.local_port.trim()
  if (kind.value === 'dynamic') return own || String(DEFAULT_SOCKS_PORT)
  return own || form.remote_port.trim()
})
const dest = computed(() => form.remote_host.trim() || 'localhost')
const effectiveName = computed(() => {
  if (form.name.trim()) return form.name.trim()
  const port = form.remote_port.trim() || '…'
  if (kind.value === 'dynamic') return `${hostLabel.value} SOCKS :${effectiveLocal.value}`
  if (kind.value === 'remote') return `${hostLabel.value} :${port} to ${dest.value}:${effectiveLocal.value || '…'}`
  return `${hostLabel.value} ${dest.value}:${port}`
})

const errors = computed(() => ({
  host: form.host_id > 0 ? undefined : 'Choose a host',
  local_port: portError(form.local_port, false),
  remote_host: kind.value === 'dynamic' || form.remote_host.trim() ? undefined : 'Required',
  remote_port: kind.value === 'dynamic' ? undefined : portError(form.remote_port, true),
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
    const dynamic = kind.value === 'dynamic'
    await forwards.save({
      id: f?.id,
      host_id: form.host_id,
      name: effectiveName.value,
      kind: kind.value,
      local_port: Number(effectiveLocal.value),
      remote_host: dynamic ? '' : form.remote_host.trim(),
      remote_port: dynamic ? 0 : Number(form.remote_port),
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
  <BaseModal :title="forward ? 'Edit Forward' : 'New Forward'" :width="520" @close="emit('close')">
    <form class="fwd-form" novalidate @submit.prevent="save">
      <div class="field">
        <div class="auth-toggle" role="radiogroup" aria-label="Forward type">
          <button
            v-for="k in FORWARD_KINDS"
            :key="k.kind"
            type="button"
            class="auth-option"
            :class="{ on: kind === k.kind }"
            role="radio"
            :aria-checked="kind === k.kind"
            :title="k.hint"
            @click="kind = k.kind"
          >
            {{ k.label }} <span class="kind-flag mono">{{ k.flag }}</span>
          </button>
        </div>
        <span class="field-hint">{{ kindInfo.hint }}</span>
      </div>

      <!-- Live route preview: the mental model, drawn -->
      <div v-if="kind === 'local'" class="route" aria-hidden="true">
        <span class="route-node">
          <Laptop :size="14" :stroke-width="1.5" />
          <span class="route-label">{{ thisComputer }}</span>
          <span class="route-value mono">:{{ effectiveLocal || '…' }}</span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node via">
          <Server :size="14" :stroke-width="1.5" />
          <span class="route-label">via SSH</span>
          <span class="route-value text-ellipsis"><PvText :text="hostLabel" /></span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node">
          <Globe :size="14" :stroke-width="1.5" />
          <span class="route-label">Destination</span>
          <span class="route-value mono text-ellipsis"><span class="pv">{{ pv.hostText(form.remote_host.trim()) || '…' }}</span>:{{ form.remote_port.trim() || '…' }}</span>
        </span>
      </div>

      <div v-else-if="kind === 'remote'" class="route" aria-hidden="true">
        <span class="route-node via">
          <Server :size="14" :stroke-width="1.5" />
          <span class="route-label">Listens on</span>
          <span class="route-value mono text-ellipsis"><PvText :text="hostLabel" />:{{ form.remote_port.trim() || '…' }}</span>
        </span>
        <ArrowLeft :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node">
          <Laptop :size="14" :stroke-width="1.5" />
          <span class="route-label">via {{ thisComputerLower }}</span>
          <span class="route-value text-ellipsis">SSH</span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node">
          <Globe :size="14" :stroke-width="1.5" />
          <span class="route-label">Destination</span>
          <span class="route-value mono text-ellipsis"><span class="pv">{{ pv.hostText(form.remote_host.trim()) || '…' }}</span>:{{ effectiveLocal || '…' }}</span>
        </span>
      </div>

      <div v-else class="route" aria-hidden="true">
        <span class="route-node">
          <AppWindow :size="14" :stroke-width="1.5" />
          <span class="route-label">SOCKS proxy</span>
          <span class="route-value mono">localhost:{{ effectiveLocal }}</span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node via">
          <Server :size="14" :stroke-width="1.5" />
          <span class="route-label">via SSH</span>
          <span class="route-value text-ellipsis"><PvText :text="hostLabel" /></span>
        </span>
        <ArrowRight :size="14" :stroke-width="1.5" class="route-arrow" />
        <span class="route-node">
          <Globe :size="14" :stroke-width="1.5" />
          <span class="route-label">Destination</span>
          <span class="route-value text-ellipsis">Any address</span>
        </span>
      </div>

      <div class="field">
        <label class="field-label" for="fw-host">Through host</label>
        <SearchSelect id="fw-host" v-model="form.host_id" :options="hostOptions" aria-label="Through host" />
        <span v-if="submitted && errors.host" class="field-error">{{ errors.host }}</span>
      </div>

      <!-- Remote: the port the server opens comes first -->
      <div v-if="kind === 'remote'" class="field">
        <label class="field-label" for="fw-sport">Server port</label>
        <input
          id="fw-sport"
          v-model="form.remote_port"
          class="field-input mono port-input"
          :class="{ invalid: show('remote_port') }"
          type="text"
          inputmode="numeric"
          placeholder="8080"
          @blur="touched.remote_port = true"
        />
        <span v-if="show('remote_port')" class="field-error">{{ show('remote_port') }}</span>
        <span v-else class="field-hint">Programs on <PvText :text="hostLabel" /> connect to localhost on this port</span>
      </div>

      <div v-if="kind !== 'dynamic'" class="dest-grid">
        <div class="field">
          <label class="field-label" for="fw-rhost">Destination host</label>
          <input
            id="fw-rhost"
            v-model="form.remote_host"
            class="field-input mono pv-field"
            :class="{ invalid: show('remote_host') }"
            type="text"
            placeholder="localhost"
            spellcheck="false"
            autocapitalize="off"
            @blur="touched.remote_host = true"
          />
          <span v-if="show('remote_host')" class="field-error">{{ show('remote_host') }}</span>
          <span v-else class="field-hint">{{ kind === 'remote' ? `As seen from ${thisComputerLower}` : 'As seen from the SSH host' }}</span>
        </div>
        <div v-if="kind === 'local'" class="field">
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
        <div v-else class="field">
          <label class="field-label" for="fw-dport">Port</label>
          <input
            id="fw-dport"
            v-model="form.local_port"
            class="field-input mono"
            :class="{ invalid: show('local_port') }"
            type="text"
            inputmode="numeric"
            :placeholder="form.remote_port.trim() || '3000'"
            @blur="touched.local_port = true"
          />
          <span v-if="show('local_port')" class="field-error">{{ show('local_port') }}</span>
          <span v-else class="field-hint">Empty = same</span>
        </div>
      </div>

      <div class="dest-grid">
        <div class="field">
          <label class="field-label" for="fw-name">Name</label>
          <input
            id="fw-name"
            v-model="form.name"
            class="field-input pv-field"
            type="text"
            :placeholder="pv.t(effectiveName)"
            spellcheck="false"
          />
        </div>
        <div v-if="kind !== 'remote'" class="field">
          <label class="field-label" for="fw-lport">Local port</label>
          <input
            id="fw-lport"
            v-model="form.local_port"
            class="field-input mono"
            :class="{ invalid: show('local_port') }"
            type="text"
            inputmode="numeric"
            :placeholder="kind === 'dynamic' ? String(DEFAULT_SOCKS_PORT) : form.remote_port.trim() || 'same'"
            @blur="touched.local_port = true"
          />
          <span v-if="show('local_port')" class="field-error">{{ show('local_port') }}</span>
          <span v-else class="field-hint">{{ kind === 'dynamic' ? `Empty = ${DEFAULT_SOCKS_PORT}` : 'Empty = same' }}</span>
        </div>
      </div>

      <p v-if="kind === 'dynamic'" class="note">
        Point a browser or app at the SOCKS5 proxy <code class="mono">localhost:{{ effectiveLocal }}</code>. Its connections leave
        from <PvText :text="hostLabel" />, so sites see that server's address.
      </p>
      <p v-else-if="kind === 'remote'" class="note">
        The server's sshd must allow forwarding (<code class="mono">AllowTcpForwarding</code>, the default). Only programs on the
        server itself can reach the port.
      </p>

      <label class="check">
        <input v-model="form.auto_start" type="checkbox" />
        <span>
          Start automatically
          <span class="check-hint">whenever <PvText :text="hostLabel" /> connects</span>
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

.port-input {
  max-width: 160px;
}

.kind-flag {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.auth-option.on .kind-flag {
  color: var(--text-3);
}

.note {
  margin: 0;
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-3);
}

.note code {
  color: var(--text-2);
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
