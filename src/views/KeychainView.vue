<script setup lang="ts">
import { computed, ref } from 'vue'
import { KeyRound, LockKeyhole, MoreHorizontal, Pencil, Plus, Trash2, IdCard } from 'lucide-vue-next'
import { useCredentialsStore } from '../stores/credentials'
import { useHostsStore } from '../stores/hosts'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import CredentialDialog from '../components/CredentialDialog.vue'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import SearchField from '../components/SearchField.vue'
import { matchesQuery, plural } from '../utils/pageHelpers'
import type { Credential } from '../types'

const credentials = useCredentialsStore()
const hosts = useHostsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const editorOpen = ref(false)
const editing = ref<Credential | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)

const usage = computed(() => {
  const m = new Map<number, number>()
  for (const h of hosts.hosts) {
    if (h.credential_id !== null) m.set(h.credential_id, (m.get(h.credential_id) ?? 0) + 1)
  }
  return m
})

const sorted = computed(() =>
  [...credentials.credentials].sort((a, b) => a.name.localeCompare(b.name)),
)

const visible = computed(() =>
  sorted.value.filter((c) => matchesQuery(query.value, c.name, c.username, keyFile(c))),
)

function usedBy(c: Credential): number {
  return usage.value.get(c.id) ?? 0
}

function keyFile(c: Credential): string {
  return c.key_path ? (c.key_path.split(/[\\/]/).pop() ?? c.key_path) : ''
}

function secretState(c: Credential): { label: string; tone: 'success' | 'warning' | '' } {
  if (c.auth_method === 'key') return { label: keyFile(c) || 'Private key', tone: '' }
  return c.has_password
    ? { label: 'Password saved', tone: 'success' }
    : { label: 'Asks on connect', tone: 'warning' }
}

function openNew() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(cred: Credential) {
  editing.value = cred
  editorOpen.value = true
}

function openMenu(cred: Credential, e: MouseEvent) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: 'Edit', icon: Pencil, action: () => openEdit(cred) },
      { label: 'Delete', icon: Trash2, danger: true, action: () => void remove(cred) },
    ],
  }
}

async function remove(cred: Credential) {
  const n = usedBy(cred)
  const ok = await dialogs.confirm({
    title: `Delete “${cred.name}”?`,
    message: n
      ? `${plural(n, 'host')} use this credential and will fall back to their own login details.`
      : 'No hosts use this credential. The saved secret is removed from this machine.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  try {
    await credentials.remove(cred.id)
    ui.toast('info', `Deleted “${cred.name}”`)
  } catch (e) {
    ui.toast('error', `Could not delete: ${errorText(e)}`)
  }
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-heading">
        <h2 class="page-title">
          Keychain
          <span v-if="credentials.credentials.length" class="page-count">{{ credentials.credentials.length }}</span>
        </h2>
        <p class="page-sub">Save a login once, reuse it on any host. Secrets stay in the Windows Credential Manager.</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New credential
        </button>
      </div>
    </header>

    <div v-if="credentials.credentials.length" class="page-toolbar">
      <SearchField v-model="query" label="Search credentials" placeholder="Search by name, user or key" />
    </div>

    <div class="page-scroll">
      <div v-if="!credentials.credentials.length" class="empty-hero">
        <span class="empty-icon"><IdCard :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
        <p class="empty-title">No credentials yet</p>
        <p class="empty-sub">
          A credential is a username plus a password or private key. Attach it to many hosts and
          update it in one place.
        </p>
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New credential
        </button>
      </div>

      <p v-else-if="!visible.length" class="no-results">No credentials match “{{ query.trim() }}”</p>

      <div v-else class="card-grid">
        <div
          v-for="cred in visible"
          :key="cred.id"
          class="cred-card"
          role="button"
          tabindex="0"
          :aria-label="`Edit credential ${cred.name}`"
          @click="openEdit(cred)"
          @keydown.enter.self="openEdit(cred)"
          @keydown.space.self.prevent="openEdit(cred)"
          @contextmenu.prevent="openMenu(cred, $event)"
        >
          <div class="cred-top">
            <span class="cred-tile" :class="cred.auth_method">
              <KeyRound v-if="cred.auth_method === 'key'" :size="17" :stroke-width="1.5" aria-hidden="true" />
              <LockKeyhole v-else :size="17" :stroke-width="1.5" aria-hidden="true" />
            </span>
            <span class="cred-text">
              <span class="cred-name text-ellipsis">{{ cred.name }}</span>
              <span class="cred-user mono text-ellipsis">{{ cred.username }}</span>
            </span>
            <button
              class="btn-icon cred-more"
              :aria-label="`Options for ${cred.name}`"
              @click.stop="openMenu(cred, $event)"
            >
              <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>

          <div class="cred-foot">
            <span
              class="chip text-ellipsis"
              :class="secretState(cred).tone"
              :title="cred.auth_method === 'key' ? cred.key_path ?? '' : undefined"
            >
              <span v-if="cred.auth_method === 'key'" class="mono">{{ secretState(cred).label }}</span>
              <template v-else>{{ secretState(cred).label }}</template>
            </span>
            <span class="cred-usage" :class="{ unused: !usedBy(cred) }">
              {{ usedBy(cred) ? `Used by ${plural(usedBy(cred), 'host')}` : 'Not in use' }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <CredentialDialog v-if="editorOpen" :credential="editing" @close="editorOpen = false" />
    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.cred-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: 14px 14px 12px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  cursor: pointer;
  transition:
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

@media (hover: hover) and (pointer: fine) {
  .cred-card:hover {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }
}

.cred-card:active:not(:has(.cred-more:active)) {
  transform: scale(0.985);
}

.cred-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

.cred-top {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

/* 40px tile, radius 10 = card radius 12 minus the visual inset — concentric */
.cred-tile {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 10px;
  flex-shrink: 0;
  background: var(--accent-bg);
  color: var(--accent);
}

.cred-tile.key {
  background: var(--warning-bg);
  color: var(--warning);
}

.cred-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
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
  font-size: var(--text-sm);
  color: var(--text-2);
}

.cred-more {
  align-self: flex-start;
  margin: -4px -6px 0 0;
  opacity: 0;
  transition:
    opacity var(--dur-hover) ease,
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.cred-card:hover .cred-more,
.cred-card:focus-within .cred-more {
  opacity: 1;
}

@media (hover: none) {
  .cred-more { opacity: 1; }
}

.cred-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  min-width: 0;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}

.cred-foot .chip {
  min-width: 0;
  max-width: 60%;
  display: inline-block;
}

.cred-usage {
  font-size: var(--text-xs);
  color: var(--text-3);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

.cred-usage.unused {
  color: var(--text-4);
}
</style>
