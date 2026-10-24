<script setup lang="ts">
import { ref } from 'vue'
import { IdCard, KeyRound, Plus, Trash2 } from 'lucide-vue-next'
import { useCredentialsStore } from '../stores/credentials'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import CredentialDialog from '../components/CredentialDialog.vue'
import type { Credential } from '../types'

const credentials = useCredentialsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const editorOpen = ref(false)
const editing = ref<Credential | null>(null)

function openNew() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(cred: Credential) {
  editing.value = cred
  editorOpen.value = true
}

async function remove(cred: Credential) {
  const ok = await dialogs.confirm({
    title: `Delete credential “${cred.name}”?`,
    message: 'Hosts using this credential will fall back to their own login details.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  await credentials.remove(cred.id)
  ui.toast('info', `Deleted “${cred.name}”`)
}

function authIcon(c: Credential) {
  return c.auth_method === 'key' ? KeyRound : IdCard
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <h2 class="page-title">Keychain</h2>
      <button class="btn btn-primary" @click="openNew">
        <Plus :size="14" :stroke-width="2" aria-hidden="true" />
        New credential
      </button>
    </header>

    <div class="page-scroll">
      <div v-if="!credentials.credentials.length" class="empty-state page-empty">
        <IdCard :size="22" :stroke-width="1.5" aria-hidden="true" />
        <p>No credentials yet. Save a username + password or key once, then reuse it on any host.</p>
        <button class="btn btn-secondary" @click="openNew">Create your first credential</button>
      </div>

      <div v-else class="card-grid">
        <div v-for="cred in credentials.credentials" :key="cred.id" class="cred-card">
          <span class="cred-icon">
            <component :is="authIcon(cred)" :size="16" :stroke-width="1.5" aria-hidden="true" />
          </span>
          <span class="cred-text">
            <span class="cred-name text-ellipsis">{{ cred.name }}</span>
            <span class="cred-user mono text-ellipsis">{{ cred.username }}</span>
            <span class="cred-meta">
              {{ cred.auth_method === 'key' ? 'Private key' : 'Password' }}
              · {{ cred.has_password || cred.auth_method === 'key' ? 'saved' : 'asked on connect' }}
            </span>
          </span>
          <span class="cred-actions">
            <button class="btn-icon" :aria-label="`Edit ${cred.name}`" @click="openEdit(cred)">
              <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Delete ${cred.name}`" @click="remove(cred)">
              <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </span>
        </div>
      </div>
    </div>

    <CredentialDialog v-if="editorOpen" :credential="editing" @close="editorOpen = false" />
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.cred-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  transition: background-color var(--dur-hover) ease, border-color var(--dur-hover) ease;
}

.cred-card:hover {
  background: var(--bg-hover);
}

.cred-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: var(--radius-md);
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.cred-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.35;
}

.cred-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.cred-user {
  font-size: var(--text-xs);
  color: var(--text-2);
}

.cred-meta {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.cred-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.cred-card:hover .cred-actions,
.cred-card:focus-within .cred-actions {
  opacity: 1;
}
</style>
