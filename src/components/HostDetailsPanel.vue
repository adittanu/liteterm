<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { X, KeyRound, IdCard } from 'lucide-vue-next'
import { useHostsStore } from '../stores/hosts'
import { useCredentialsStore } from '../stores/credentials'
import { useSessionsStore } from '../stores/sessions'
import { api } from '../services/api'
import OsAvatar from './OsAvatar.vue'
import type { Host } from '../types'

const props = defineProps<{ host: Host }>()
const emit = defineEmits<{ close: [] }>()

const hosts = useHostsStore()
const credentials = useCredentialsStore()
const sessions = useSessionsStore()

const form = reactive({
  label: props.host.label,
  port: String(props.host.port),
  group_id: props.host.group_id,
  credential_id: props.host.credential_id,
  // inline auth (used when credential_id is null)
  username: props.host.username,
  auth_method: props.host.auth_method,
  password: '',
  save_password: props.host.password_saved,
  key_path: props.host.key_path ?? '',
  key_passphrase: '',
})

const inlineMode = computed(() => form.credential_id === null)

watch(
  () => props.host.id,
  () => {
    form.label = props.host.label
    form.port = String(props.host.port)
    form.group_id = props.host.group_id
    form.credential_id = props.host.credential_id
    form.username = props.host.username
    form.auth_method = props.host.auth_method
    form.password = ''
    form.save_password = props.host.password_saved
    form.key_path = props.host.key_path ?? ''
    form.key_passphrase = ''
  },
)

async function saveField() {
  if (
    form.label === props.host.label &&
    form.port === String(props.host.port) &&
    form.group_id === props.host.group_id &&
    form.credential_id === props.host.credential_id &&
    (form.credential_id !== null ||
      (form.username === props.host.username &&
        form.auth_method === props.host.auth_method &&
        (form.auth_method === 'password' || form.key_path === (props.host.key_path ?? ''))))
  ) {
    return
  }
  await hosts.saveHost(buildInput())
}

function buildInput() {
  const usingCred =
    form.credential_id !== null ? credentials.byId(form.credential_id) : undefined
  return {
    id: props.host.id,
    group_id: form.group_id,
    label: form.label.trim() || props.host.label,
    address: props.host.address,
    port: Number(form.port) || props.host.port,
    username: usingCred ? usingCred.username : form.username.trim() || props.host.username,
    auth_method: usingCred ? usingCred.auth_method : form.auth_method,
    password: inlineMode.value && form.auth_method === 'password' && form.password ? form.password : null,
    save_password: inlineMode.value ? form.save_password : false,
    key_path: inlineMode.value && form.auth_method === 'key' ? form.key_path.trim() || null : null,
    key_passphrase: inlineMode.value && form.auth_method === 'key' && form.key_passphrase ? form.key_passphrase : null,
    credential_id: form.credential_id,
  }
}

async function browseKey() {
  const path = await api.pickFile('Select private key', [
    { name: 'Private keys', extensions: ['pem', 'key', 'ppk', 'id_rsa', 'ed25519', 'ecdsa'] },
    { name: 'All files', extensions: ['*'] },
  ])
  if (path) {
    form.key_path = path
    await saveField()
  }
}

function connect() {
  void sessions.connect(props.host.id)
}
</script>

<template>
  <aside class="details-panel">
    <header class="panel-header">
      <h3 class="panel-title">Host Details</h3>
      <div class="panel-header-actions">
        <slot name="actions" />
        <button class="btn-icon" aria-label="Close details" @click="emit('close')">
          <X :size="15" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>
    </header>

    <div class="panel-scroll">
      <!-- Address -->
      <div class="panel-card">
        <div class="panel-card-title">Address</div>
        <div class="address-row">
          <OsAvatar :label="host.label" :os="host.os" :size="40" />
          <code class="address mono">{{ host.address }}</code>
        </div>
      </div>

      <!-- General -->
      <div class="panel-card">
        <div class="panel-card-title">General</div>
        <div class="field">
          <label class="field-label" for="hp-label">Name</label>
          <input id="hp-label" v-model="form.label" class="field-input" type="text" spellcheck="false" @blur="saveField" @keydown.enter="saveField" />
        </div>
        <div class="field">
          <label class="field-label" for="hp-group">Group</label>
          <select id="hp-group" v-model.number="form.group_id" class="field-input" @change="saveField">
            <option :value="null">No group</option>
            <option v-for="g in hosts.groups" :key="g.id" :value="g.id">{{ g.name }}</option>
          </select>
        </div>
        <div class="field">
          <label class="field-label" for="hp-cred">Credentials</label>
          <select id="hp-cred" v-model.number="form.credential_id" class="field-input" @change="saveField">
            <option :value="null">No credential (set below)</option>
            <option v-for="c in credentials.credentials" :key="c.id" :value="c.id">
              {{ c.name }} ({{ c.username }})
            </option>
          </select>
          <span v-if="!credentials.credentials.length" class="field-hint">
            Create reusable credentials in Keychain.
          </span>
        </div>
      </div>

      <!-- Inline auth: shown when no credential is linked -->
      <div v-if="inlineMode" class="panel-card">
        <div class="panel-card-title">Login</div>
        <div class="field">
          <label class="field-label" for="hp-user">Username</label>
          <input id="hp-user" v-model="form.username" class="field-input mono" type="text" spellcheck="false" autocapitalize="off" @blur="saveField" @keydown.enter="saveField" />
        </div>

        <div class="field">
          <span class="field-label">Authentication</span>
          <div class="auth-toggle" role="radiogroup" aria-label="Authentication method">
            <button type="button" class="auth-option" :class="{ on: form.auth_method === 'password' }" role="radio" :aria-checked="form.auth_method === 'password'" @click="form.auth_method = 'password'; saveField()">
              <IdCard :size="13" :stroke-width="1.5" aria-hidden="true" />
              Password
            </button>
            <button type="button" class="auth-option" :class="{ on: form.auth_method === 'key' }" role="radio" :aria-checked="form.auth_method === 'key'" @click="form.auth_method = 'key'; saveField()">
              <KeyRound :size="13" :stroke-width="1.5" aria-hidden="true" />
              Private key
            </button>
          </div>
        </div>

        <template v-if="form.auth_method === 'password'">
          <div class="field">
            <label class="field-label" for="hp-pass">Password</label>
            <input id="hp-pass" v-model="form.password" class="field-input" type="password" :placeholder="host.has_password ? '•••••••• (saved)' : 'Leave empty to type on connect'" autocomplete="new-password" @blur="saveField" @keydown.enter="saveField" />
            <span class="field-hint">
              <label class="check">
                <input v-model="form.save_password" type="checkbox" @change="saveField" />
                Remember password (Windows Credential Manager)
              </label>
            </span>
          </div>
        </template>

        <template v-else>
          <div class="field">
            <label class="field-label" for="hp-key">Private key file</label>
            <div class="key-row">
              <input id="hp-key" v-model="form.key_path" class="field-input mono" type="text" placeholder="C:\Users\you\.ssh\id_ed25519" spellcheck="false" @blur="saveField" @keydown.enter="saveField" />
              <button type="button" class="btn btn-secondary" @click="browseKey">Browse…</button>
            </div>
          </div>
          <div class="field">
            <label class="field-label" for="hp-keypass">Key passphrase</label>
            <input id="hp-keypass" v-model="form.key_passphrase" class="field-input" type="password" placeholder="Leave empty if the key has none" autocomplete="new-password" @blur="saveField" @keydown.enter="saveField" />
          </div>
        </template>
      </div>

      <!-- Port -->
      <div class="panel-card">
        <div class="port-row">
          <span class="panel-card-title">SSH on</span>
          <input
            v-model="form.port"
            class="field-input mono port-input"
            type="text"
            inputmode="numeric"
            aria-label="SSH port"
            @blur="saveField"
            @keydown.enter="saveField"
          />
          <span class="panel-card-title">port</span>
        </div>
      </div>

      <slot name="danger" />
    </div>

    <footer class="panel-footer">
      <button class="btn btn-primary connect-btn" @click="connect">Connect</button>
    </footer>
  </aside>
</template>

<style scoped>
.details-panel {
  display: flex;
  flex-direction: column;
  width: 340px;
  flex-shrink: 0;
  background: var(--bg-surface);
  min-height: 0;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-3) var(--space-4);
  border-bottom: 1px solid var(--border);
}

.panel-title {
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--text-1);
}

.panel-header-actions {
  display: flex;
  gap: 2px;
}

.panel-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-4);
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  min-height: 0;
}

.panel-card {
  background: var(--bg-surface-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  padding: var(--space-3);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.panel-card-title {
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-2);
}

.address-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.address {
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 8px 10px;
  font-size: var(--text-sm);
  color: var(--text-1);
  user-select: text;
  word-break: break-all;
}

.auth-toggle {
  display: flex;
  gap: 4px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 3px;
}

.auth-option {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  justify-content: center;
  padding: 6px 10px;
  border-radius: 6px;
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.auth-option:hover {
  color: var(--text-1);
}

.auth-option.on {
  background: var(--bg-selected);
  color: var(--text-1);
  box-shadow: var(--shadow-1);
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

.port-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.port-input {
  width: 76px;
  padding: 6px 8px;
  text-align: center;
}

.panel-footer {
  padding: var(--space-3) var(--space-4);
  border-top: 1px solid var(--border);
}

.connect-btn {
  width: 100%;
  padding: 10px;
  font-size: var(--text-md);
}
</style>
