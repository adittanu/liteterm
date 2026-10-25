<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { KeyRound, IdCard, ChevronDown } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import SearchSelect, { type ComboOption } from './SearchSelect.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'
import { useCredentialsStore } from '../stores/credentials'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { api } from '../services/api'
import type { AuthMethod } from '../types'

const dialogs = useDialogsStore()
const hosts = useHostsStore()
const credentials = useCredentialsStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const open = computed(() => dialogs.hostDialog.open)
const editing = computed(() => dialogs.hostDialog.host)
const connectAfter = computed(() => dialogs.hostDialog.connectAfter && !editing.value)
const resolveHint = computed(() => dialogs.hostDialog.prefill?.ips ?? [])

// '' = inline (no credential) · 'new' = create one · '<id>' = use existing
const credentialChoice = ref<string>('')
const showCredDetails = ref(false)

const form = reactive({
  label: '',
  address: '',
  port: '22',
  group_id: null as number | null,
  // inline (legacy / no credential)
  username: '',
  auth_method: 'password' as AuthMethod,
  password: '',
  save_password: true,
  key_path: '',
  key_passphrase: '',
  // new credential
  cred_name: '',
})

const errors = reactive<{ label?: string; address?: string; port?: string; username?: string; key_path?: string; cred_name?: string }>({})
const saving = ref(false)

const selectedCred = computed(() => {
  const id = Number(credentialChoice.value)
  return Number.isInteger(id) && id > 0 ? credentials.byId(id) : undefined
})

const groupOptions = computed<ComboOption[]>(() => [
  { value: null, label: 'No group', muted: true },
  ...hosts.groups.map((g) => ({ value: g.id, label: g.name })),
])

const credOptions = computed<ComboOption[]>(() => [
  { value: '', label: 'No credential', hint: 'fill in below', muted: true },
  { value: 'new', label: '+ New credential', hint: 'save & reuse' },
  ...credentials.credentials.map((c) => ({
    value: String(c.id),
    label: c.name,
    hint: c.username,
  })),
])

// Inline validation — validate as the user types, not on submit
watch(
  () => [form.label, form.address, form.port, form.username, form.key_path, form.cred_name, credentialChoice.value],
  () => {
    if (form.address.trim()) errors.address = undefined
    const p = Number(form.port)
    if (!form.port || (Number.isInteger(p) && p >= 1 && p <= 65535)) errors.port = undefined
    if (form.username.trim() || credentialChoice.value !== '') errors.username = undefined
    if (form.auth_method !== 'key' || form.key_path.trim() || credentialChoice.value !== '') errors.key_path = undefined
    if (form.cred_name.trim() || credentialChoice.value !== 'new') errors.cred_name = undefined
  },
)

function validate(): boolean {
  let ok = true
  if (!form.label.trim()) { errors.label = 'Give this connection a name'; ok = false }
  if (!form.address.trim()) { errors.address = 'Hostname or IP is required'; ok = false }
  const p = Number(form.port)
  if (!form.port || !Number.isInteger(p) || p < 1 || p > 65535) { errors.port = 'Port must be 1–65535'; ok = false }
  if (credentialChoice.value === 'new') {
    if (!form.cred_name.trim()) { errors.cred_name = 'Name this credential'; ok = false }
    if (!form.username.trim()) { errors.username = 'Username is required'; ok = false }
    if (form.auth_method === 'key' && !form.key_path.trim()) { errors.key_path = 'Choose a private key file'; ok = false }
  } else if (credentialChoice.value === '') {
    if (!form.username.trim()) { errors.username = 'Username is required'; ok = false }
    if (form.auth_method === 'key' && !form.key_path.trim()) { errors.key_path = 'Choose a private key file'; ok = false }
  }
  return ok
}

watch(open, (o) => {
  if (!o) return
  const host = dialogs.hostDialog.host
  showCredDetails.value = false
  if (host) {
    form.label = host.label
    form.address = host.address
    form.port = String(host.port)
    form.group_id = host.group_id
    form.username = host.username
    form.auth_method = host.auth_method
    form.password = ''
    form.save_password = host.password_saved
    form.key_path = host.key_path ?? ''
    form.key_passphrase = ''
    form.cred_name = ''
    credentialChoice.value = host.credential_id ? String(host.credential_id) : ''
  } else {
    const prefill = dialogs.hostDialog.prefill
    // Name defaults to the hostname; username/address stay in their own
    // fields so they don't get merged into the label
    form.label = prefill?.address ?? ''
    form.address = prefill?.address ?? ''
    form.port = prefill ? String(prefill.port) : '22'
    form.group_id = dialogs.hostDialog.groupId
    form.username = prefill?.username ?? ''
    form.auth_method = 'password'
    form.password = ''
    form.save_password = true
    form.key_path = ''
    form.key_passphrase = ''
    form.cred_name = ''
    credentialChoice.value = ''
  }
  errors.label = errors.address = errors.port = errors.username = errors.key_path = errors.cred_name = undefined
})

async function browseKey() {
  const path = await api.pickFile('Select private key', [
    { name: 'Private keys', extensions: ['pem', 'key', 'ppk', 'id_rsa', 'ed25519', 'ecdsa'] },
    { name: 'All files', extensions: ['*'] },
  ])
  if (path) {
    form.key_path = path
    if (!form.label.trim()) form.label = path.split(/[\\/]/).pop() ?? ''
  }
}

async function save() {
  if (!validate()) return
  saving.value = true
  try {
    const credId = Number(credentialChoice.value)
    const usingExisting = Number.isInteger(credId) && credId > 0 ? credentials.byId(credId) : undefined

    let input
    if (usingExisting) {
      input = {
        id: editing.value?.id,
        group_id: form.group_id,
        label: form.label.trim(),
        address: form.address.trim(),
        port: Number(form.port),
        username: usingExisting.username,
        auth_method: usingExisting.auth_method,
        credential_id: usingExisting.id,
      }
    } else if (credentialChoice.value === 'new') {
      input = {
        id: editing.value?.id,
        group_id: form.group_id,
        label: form.label.trim(),
        address: form.address.trim(),
        port: Number(form.port),
        username: form.username.trim(),
        auth_method: form.auth_method,
        create_credential: {
          name: form.cred_name.trim(),
          username: form.username.trim(),
          auth_method: form.auth_method,
          password: form.password ? form.password : null,
          save_password: form.save_password,
          key_path: form.auth_method === 'key' ? form.key_path.trim() : null,
          key_passphrase: form.key_passphrase ? form.key_passphrase : null,
        },
      }
    } else {
      input = {
        id: editing.value?.id,
        group_id: form.group_id,
        label: form.label.trim(),
        address: form.address.trim(),
        port: Number(form.port),
        username: form.username.trim(),
        auth_method: form.auth_method,
        password: form.password ? form.password : null,
        save_password: form.save_password,
        key_path: form.auth_method === 'key' ? form.key_path.trim() : null,
        key_passphrase: form.key_passphrase ? form.key_passphrase : null,
        credential_id: null,
      }
    }

    const saved = await hosts.saveHost(input)
    const shouldConnect = connectAfter.value
    // Use the just-typed secret for the first connection even if "Remember"
    // is off — the backend prefers a passed secret over the keyring
    const pending =
      credentialChoice.value === '' || credentialChoice.value === 'new'
        ? (form.auth_method === 'password' ? form.password : form.key_passphrase) || undefined
        : undefined
    dialogs.closeHostDialog()
    if (shouldConnect) void sessions.connect(saved.id, pending)
  } catch (e) {
    ui.toast('error', `Could not save host: ${String(e)}`)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <BaseModal
    v-if="open"
    :title="editing ? 'Edit Host' : 'New Host'"
    :width="500"
    @close="dialogs.closeHostDialog()"
  >
    <form class="host-form" @submit.prevent="save">
      <div class="field">
        <label class="field-label" for="hf-label">Name</label>
        <input
          id="hf-label"
          v-model="form.label"
          class="field-input"
          :class="{ invalid: errors.label }"
          type="text"
          placeholder="My server"
          spellcheck="false"
          autofocus
        />
        <span v-if="errors.label" class="field-error">{{ errors.label }}</span>
      </div>

      <div class="row">
        <div class="field grow">
          <label class="field-label" for="hf-address">Host</label>
          <input
            id="hf-address"
            v-model="form.address"
            class="field-input mono"
            :class="{ invalid: errors.address }"
            type="text"
            placeholder="example.com or 192.168.1.10"
            spellcheck="false"
            autocapitalize="off"
          />
          <span v-if="errors.address" class="field-error">{{ errors.address }}</span>
          <span v-if="!errors.address && resolveHint.length" class="field-hint mono">
            Resolves to {{ resolveHint.join(', ') }}
          </span>
        </div>
        <div class="field port">
          <label class="field-label" for="hf-port">Port</label>
          <input
            id="hf-port"
            v-model="form.port"
            class="field-input mono"
            :class="{ invalid: errors.port }"
            type="text"
            inputmode="numeric"
            placeholder="22"
          />
          <span v-if="errors.port" class="field-error">{{ errors.port }}</span>
        </div>
      </div>

      <div class="field">
        <label class="field-label" for="hf-group">Group</label>
        <SearchSelect id="hf-group" v-model="form.group_id" :options="groupOptions" aria-label="Group" />
      </div>

      <div class="divider"></div>

      <!-- Credentials -->
      <div class="field">
        <label class="field-label" for="hf-cred">Credentials</label>
        <SearchSelect id="hf-cred" v-model="credentialChoice" :options="credOptions" aria-label="Credentials" />
      </div>

      <!-- Existing credential chip -->
      <div v-if="selectedCred" class="cred-chip">
        <IdCard :size="14" :stroke-width="1.5" class="cred-chip-icon" aria-hidden="true" />
        <div class="cred-chip-text">
          <span class="cred-chip-name">{{ selectedCred.name }}</span>
          <span class="cred-chip-user mono">{{ selectedCred.username }}</span>
        </div>
        <button
          type="button"
          class="cred-more"
          :aria-expanded="showCredDetails"
          @click="showCredDetails = !showCredDetails"
        >
          {{ showCredDetails ? 'Show less' : 'Show more' }}
          <ChevronDown :size="12" :stroke-width="1.5" class="cred-more-chev" :class="{ open: showCredDetails }" aria-hidden="true" />
        </button>
      </div>
      <dl v-if="selectedCred && showCredDetails" class="cred-details">
        <div class="cred-detail-row">
          <dt>Username</dt>
          <dd class="mono">{{ selectedCred.username }}</dd>
        </div>
        <div class="cred-detail-row">
          <dt>Auth</dt>
          <dd>{{ selectedCred.auth_method === 'key' ? 'Private key' : 'Password' }}<template v-if="selectedCred.auth_method === 'key'"> · <span class="mono">{{ selectedCred.key_path }}</span></template></dd>
        </div>
        <div class="cred-detail-row">
          <dt>Password</dt>
          <dd>{{ selectedCred.has_password ? 'Saved in Credential Manager' : 'Asked on connect' }}</dd>
        </div>
      </dl>

      <!-- New credential: inline creation -->
      <template v-if="credentialChoice === 'new'">
        <div class="field">
          <label class="field-label" for="hf-credname">Credential name</label>
          <input
            id="hf-credname"
            v-model="form.cred_name"
            class="field-input"
            :class="{ invalid: errors.cred_name }"
            type="text"
            placeholder="e.g. admindqli"
            spellcheck="false"
          />
          <span v-if="errors.cred_name" class="field-error">{{ errors.cred_name }}</span>
          <span class="field-hint">Saved so other hosts with the same login can reuse it.</span>
        </div>
      </template>

      <!-- Inline auth: hidden when an existing credential is selected -->
      <template v-if="!selectedCred">
        <div class="field">
          <label class="field-label" for="hf-user">Username</label>
          <input
            id="hf-user"
            v-model="form.username"
            class="field-input mono"
            :class="{ invalid: errors.username }"
            type="text"
            placeholder="root"
            spellcheck="false"
            autocapitalize="off"
          />
          <span v-if="errors.username" class="field-error">{{ errors.username }}</span>
        </div>

        <div class="field">
          <span class="field-label">Authentication</span>
          <div class="auth-toggle" role="radiogroup" aria-label="Authentication method">
            <button
              type="button"
              class="auth-option"
              :class="{ on: form.auth_method === 'password' }"
              role="radio"
              :aria-checked="form.auth_method === 'password'"
              @click="form.auth_method = 'password'"
            >
              Password
            </button>
            <button
              type="button"
              class="auth-option"
              :class="{ on: form.auth_method === 'key' }"
              role="radio"
              :aria-checked="form.auth_method === 'key'"
              @click="form.auth_method = 'key'"
            >
              <KeyRound :size="13" :stroke-width="1.5" aria-hidden="true" />
              Private key
            </button>
          </div>
        </div>

        <template v-if="form.auth_method === 'password'">
          <div class="field">
            <label class="field-label" for="hf-pass">Password</label>
            <input
              id="hf-pass"
              v-model="form.password"
              class="field-input"
              type="password"
              :placeholder="editing?.has_password ? '•••••••• (saved)' : 'Leave empty to type on connect'"
              autocomplete="new-password"
            />
            <span class="field-hint">
              <label class="check">
                <input v-model="form.save_password" type="checkbox" />
                Remember password (Windows Credential Manager)
              </label>
            </span>
          </div>
        </template>

        <template v-else>
          <div class="field">
            <label class="field-label" for="hf-key">Private key file</label>
            <div class="key-row">
              <input
                id="hf-key"
                v-model="form.key_path"
                class="field-input mono"
                :class="{ invalid: errors.key_path }"
                type="text"
                placeholder="C:\Users\you\.ssh\id_ed25519"
                spellcheck="false"
              />
              <button type="button" class="btn btn-secondary" @click="browseKey">Browse…</button>
            </div>
            <span v-if="errors.key_path" class="field-error">{{ errors.key_path }}</span>
          </div>
          <div class="field">
            <label class="field-label" for="hf-keypass">Key passphrase</label>
            <input
              id="hf-keypass"
              v-model="form.key_passphrase"
              class="field-input"
              type="password"
              placeholder="Leave empty if the key has none"
              autocomplete="new-password"
            />
          </div>
        </template>
      </template>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="dialogs.closeHostDialog()">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="saving">
          {{ saving ? 'Saving…' : editing ? 'Save changes' : connectAfter ? 'Add & connect' : 'Add host' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.host-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.row {
  display: flex;
  gap: var(--space-3);
}

.grow {
  flex: 1;
  min-width: 0;
}

.port {
  width: 96px;
  flex-shrink: 0;
}

.field-input.invalid {
  border-color: var(--danger);
}

.field-input.invalid:focus {
  box-shadow: 0 0 0 3px var(--danger-bg);
}

.cred-chip {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  background: var(--accent-bg);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}

.cred-chip-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.cred-chip-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.3;
}

.cred-chip-name {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
}

.cred-chip-user {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.cred-more {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-xs);
  color: var(--text-3);
  padding: 3px 6px;
  border-radius: var(--radius-sm);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.cred-more:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.cred-more-chev {
  transition: transform var(--dur-hover) var(--ease-out);
}

.cred-more-chev.open {
  transform: rotate(180deg);
}

.cred-details {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: calc(-1 * var(--space-2)) 0 0;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-surface-2);
}

.cred-detail-row {
  display: flex;
  gap: var(--space-3);
  font-size: var(--text-sm);
}

.cred-detail-row dt {
  width: 80px;
  flex-shrink: 0;
  color: var(--text-3);
}

.cred-detail-row dd {
  color: var(--text-1);
  word-break: break-all;
  user-select: text;
}

.key-row {
  display: flex;
  gap: var(--space-2);
}

.key-row .field-input {
  flex: 1;
  min-width: 0;
}

.check {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-2);
}
</style>
