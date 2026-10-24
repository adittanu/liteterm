<script setup lang="ts">
import { reactive, watch } from 'vue'
import { X } from 'lucide-vue-next'
import { useHostsStore } from '../stores/hosts'
import { useCredentialsStore } from '../stores/credentials'
import { useSessionsStore } from '../stores/sessions'
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
})

watch(
  () => props.host.id,
  () => {
    form.label = props.host.label
    form.port = String(props.host.port)
    form.group_id = props.host.group_id
    form.credential_id = props.host.credential_id
  },
)

async function saveField() {
  // Skip until something actually changed
  if (
    form.label === props.host.label &&
    form.port === String(props.host.port) &&
    form.group_id === props.host.group_id &&
    form.credential_id === props.host.credential_id
  ) {
    return
  }
  const usingCred = form.credential_id !== null ? credentials.byId(form.credential_id) : undefined
  await hosts.saveHost({
    id: props.host.id,
    group_id: form.group_id,
    label: form.label.trim() || props.host.label,
    address: props.host.address,
    port: Number(form.port) || props.host.port,
    username: usingCred ? usingCred.username : props.host.username,
    auth_method: usingCred ? usingCred.auth_method : props.host.auth_method,
    credential_id: form.credential_id,
  })
}

function initials(label: string): string {
  const words = label.trim().split(/[\s._-]+/).filter(Boolean)
  if (!words.length) return '?'
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase()
  return (words[0][0] + words[1][0]).toUpperCase()
}

function avatarClass(label: string): string {
  let hash = 0
  for (let i = 0; i < label.length; i++) hash = (hash * 31 + label.charCodeAt(i)) >>> 0
  return `avatar-h${hash % 8}`
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
          <span class="host-avatar" :class="avatarClass(form.label)">{{ initials(form.label) }}</span>
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
            <option :value="null">No credential</option>
            <option v-for="c in credentials.credentials" :key="c.id" :value="c.id">
              {{ c.name }} ({{ c.username }})
            </option>
          </select>
          <span v-if="!credentials.credentials.length" class="field-hint">
            Create reusable credentials in Keychain.
          </span>
        </div>
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

.host-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 700;
  color: white;
  flex-shrink: 0;
}

.avatar-h0 { background: #4a7ae8; }
.avatar-h1 { background: #2a9d6e; }
.avatar-h2 { background: #c2703d; }
.avatar-h3 { background: #b0568f; }
.avatar-h4 { background: #5c8ecf; }
.avatar-h5 { background: #8a63c9; }
.avatar-h6 { background: #3f97a8; }
.avatar-h7 { background: #a85e56; }

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
