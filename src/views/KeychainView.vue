<script setup lang="ts">
import { computed, ref } from 'vue'
import { KeyRound, LockKeyhole, MoreHorizontal, Pencil, Plus, Trash2, IdCard, Fingerprint, Copy, Upload } from 'lucide-vue-next'
import { useCredentialsStore } from '../stores/credentials'
import { useHostsStore } from '../stores/hosts'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { usePrivacyStore } from '../stores/privacy'
import { errorText } from '../stores/sessions'
import CredentialDialog from '../components/CredentialDialog.vue'
import GenerateKeyDialog from '../components/GenerateKeyDialog.vue'
import InstallKeyDialog from '../components/InstallKeyDialog.vue'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import SearchField from '../components/SearchField.vue'
import PvText from '../components/PvText.vue'
import VaultChip from '../components/VaultChip.vue'
import { useTeamsStore } from '../stores/teams'
import { api } from '../services/api'
import { copyText, matchesQuery, plural } from '../utils/pageHelpers'
import type { KeySource } from '../utils/installKey'
import { secretStore } from '../utils/platform'
import type { Credential } from '../types'

const credentials = useCredentialsStore()
const hosts = useHostsStore()
const dialogs = useDialogsStore()
const teams = useTeamsStore()
const ui = useUiStore()
const pv = usePrivacyStore()

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
  if (c.auth_method === 'key' && c.key_stored) return { label: 'Key stored in LiteTerm', tone: 'success' }
  if (c.auth_method === 'key') return { label: keyFile(c) || 'Private key', tone: '' }
  if (c.auth_method === 'agent') return { label: 'SSH agent', tone: '' }
  return c.has_password
    ? { label: 'Password saved', tone: 'success' }
    : { label: 'Asks on connect', tone: 'warning' }
}

function openNew() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(cred: Credential) {
  // A team credential this account only views can't be changed here
  if (!teams.canEdit(cred.team_id)) return
  editing.value = cred
  editorOpen.value = true
}

function openMenu(cred: Credential, e: MouseEvent) {
  const readOnly = !teams.canEdit(cred.team_id)
  const keyItems: MenuItem[] =
    cred.auth_method === 'key' && (cred.key_path || cred.key_stored)
      ? [
          { label: 'Copy public key', icon: Copy, action: () => void copyPublicKey(cred) },
          { label: 'Install on a host…', icon: Upload, action: () => void installCredential(cred) },
        ]
      : []
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: 'Edit', icon: Pencil, disabled: readOnly, action: () => openEdit(cred) },
      ...keyItems,
      { label: 'Delete', icon: Trash2, danger: true, disabled: readOnly, action: () => void remove(cred) },
    ],
  }
}

// ---- keys: generate, copy the public half, install on a host ----

const keygenOpen = ref(false)
const installing = ref<{ source: KeySource; keyName: string } | null>(null)

function openInstall(source: KeySource, keyName: string) {
  keygenOpen.value = false
  installing.value = { source, keyName }
}

async function publicKeyFor(cred: Credential): Promise<string | null> {
  try {
    return cred.key_stored ? await api.credentialPublicKey(cred.id) : await api.publicKeyOf(cred.key_path!)
  } catch (e) {
    ui.toast('error', errorText(e))
    return null
  }
}

async function copyPublicKey(cred: Credential) {
  const key = await publicKeyFor(cred)
  if (key) await copyText(key, 'Public key copied')
}

async function installCredential(cred: Credential) {
  const key = await publicKeyFor(cred)
  if (!key) return
  // A stored key has no file: hosts switch to it through the credential
  const keyPath = cred.key_stored ? '' : cred.key_path!
  openInstall({ publicKey: key, keyPath, credentialId: cred.id }, keyFile(cred) || cred.name)
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
        <p class="page-sub">Save a login once, reuse it on any host. Secrets stay in the {{ secretStore }}.</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-secondary" @click="keygenOpen = true">
          <KeyRound :size="14" :stroke-width="1.75" aria-hidden="true" />
          Generate key
        </button>
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
          :aria-label="pv.t(`Edit credential ${cred.name}`)"
          @click="openEdit(cred)"
          @keydown.enter.self="openEdit(cred)"
          @keydown.space.self.prevent="openEdit(cred)"
          @contextmenu.prevent="openMenu(cred, $event)"
        >
          <div class="cred-top">
            <span class="cred-tile" :class="cred.auth_method">
              <KeyRound v-if="cred.auth_method === 'key'" :size="17" :stroke-width="1.5" aria-hidden="true" />
              <Fingerprint v-else-if="cred.auth_method === 'agent'" :size="17" :stroke-width="1.5" aria-hidden="true" />
              <LockKeyhole v-else :size="17" :stroke-width="1.5" aria-hidden="true" />
            </span>
            <span class="cred-text">
              <span class="cred-name text-ellipsis"><PvText :text="cred.name" /></span>
              <span class="cred-user mono text-ellipsis"><PvText :text="cred.username" /></span>
            </span>
            <button
              class="btn-icon cred-more"
              :aria-label="pv.t(`Options for ${cred.name}`)"
              @click.stop="openMenu(cred, $event)"
            >
              <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>

          <div class="cred-foot">
            <span
              class="chip text-ellipsis"
              :class="secretState(cred).tone"
              :title="cred.auth_method === 'key' && !cred.key_stored && !pv.blur ? pv.t(cred.key_path ?? '') : undefined"
            >
              <span v-if="cred.auth_method === 'key' && !cred.key_stored" class="mono pv">{{ secretState(cred).label }}</span>
              <template v-else>{{ secretState(cred).label }}</template>
            </span>
            <VaultChip :team-id="cred.team_id" />
            <span class="cred-usage" :class="{ unused: !usedBy(cred) }">
              {{ usedBy(cred) ? `Used by ${plural(usedBy(cred), 'host')}` : 'Not in use' }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <CredentialDialog v-if="editorOpen" :credential="editing" @close="editorOpen = false" />
    <GenerateKeyDialog v-if="keygenOpen" @close="keygenOpen = false" @install="openInstall" />
    <InstallKeyDialog v-if="installing" :source="installing.source" :key-name="installing.keyName" @close="installing = null" />
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
