<script setup lang="ts">
import { computed, ref } from 'vue'
import { Search, Plus, SquareTerminal, Pencil, Trash2 } from 'lucide-vue-next'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import HostDetailsPanel from '../components/HostDetailsPanel.vue'
import OsAvatar from '../components/OsAvatar.vue'
import type { Host } from '../types'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const selectedId = ref<number | null>(null)
const activeGroup = ref<number | 'all'>('all')

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  let list = hosts.hosts
  if (activeGroup.value !== 'all') {
    list = list.filter((h) => h.group_id === activeGroup.value)
  }
  if (!q) return list
  return list.filter(
    (h) =>
      h.label.toLowerCase().includes(q) ||
      h.address.toLowerCase().includes(q) ||
      h.username.toLowerCase().includes(q),
  )
})

const groupCards = computed(() =>
  hosts.groups.map((g) => ({
    ...g,
    count: hosts.hosts.filter((h) => h.group_id === g.id).length,
  })),
)

const selectedHost = computed(
  () => hosts.hosts.find((h) => h.id === selectedId.value) ?? null,
)

function select(host: Host) {
  selectedId.value = host.id
}

function connectFirst() {
  const first = filtered.value[0]
  if (first) void sessions.connect(first.id)
}

function statusFor(hostId: number): string {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  if (!tab) return 'idle'
  if (tab.status === 'connected') return 'connected'
  if (tab.status === 'connecting' || tab.status === 'hostkey') return 'connecting'
  if (tab.status === 'error') return 'error'
  return 'idle'
}

function initials(label: string): string {
  const words = label.trim().split(/[\s._-]+/).filter(Boolean)
  if (!words.length) return '?'
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase()
  return (words[0][0] + words[1][0]).toUpperCase()
}

async function deleteSelected() {
  const host = selectedHost.value
  if (!host) return
  const ok = await dialogs.confirm({
    title: `Delete “${host.label}”?`,
    message: 'The saved connection will be removed. Active sessions stay open.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  selectedId.value = null
  await hosts.removeHost(host.id)
  ui.toast('info', 'Host deleted')
}
</script>

<template>
  <div class="hosts-view">
    <div class="hosts-main">
      <!-- Search + connect bar -->
      <div class="find-bar">
        <div class="find-wrap">
          <Search :size="15" :stroke-width="1.5" class="find-icon" aria-hidden="true" />
          <input
            v-model="query"
            class="find-input"
            type="text"
            placeholder="Find a host or ssh user@hostname…"
            spellcheck="false"
            data-host-search
            aria-label="Find a host"
            @keydown.enter="connectFirst"
          />
        </div>
        <button class="btn btn-primary" :disabled="!filtered.length" @click="connectFirst">
          Connect
        </button>
      </div>

      <!-- Toolbar -->
      <div class="toolbar">
        <button class="btn btn-secondary" @click="dialogs.openHostDialog(null, null)">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New host
        </button>
        <button class="btn btn-secondary" @click="sessions.connectLocal()">
          <SquareTerminal :size="14" :stroke-width="1.5" aria-hidden="true" />
          Local terminal
        </button>
      </div>

      <div class="dash-scroll">
        <!-- Groups -->
        <section v-if="hosts.groups.length" class="dash-section">
          <h3 class="section-title">Groups</h3>
          <div class="card-grid">
            <button
              class="group-card"
              :class="{ on: activeGroup === 'all' }"
              @click="activeGroup = 'all'"
            >
              <span class="group-avatar all">All</span>
              <span class="group-text">
                <span class="group-name">All hosts</span>
                <span class="group-sub">{{ hosts.hosts.length }} {{ hosts.hosts.length === 1 ? "Host" : "Hosts" }}</span>
              </span>
            </button>
            <button
              v-for="g in groupCards"
              :key="g.id"
              class="group-card"
              :class="{ on: activeGroup === g.id }"
              @click="activeGroup = activeGroup === g.id ? 'all' : g.id"
            >
              <span class="group-avatar">{{ initials(g.name) }}</span>
              <span class="group-text">
                <span class="group-name text-ellipsis">{{ g.name }}</span>
                <span class="group-sub">{{ g.count }} {{ g.count === 1 ? "Host" : "Hosts" }}</span>
              </span>
            </button>
          </div>
        </section>

        <!-- Hosts -->
        <section class="dash-section">
          <h3 class="section-title">Hosts</h3>
          <div v-if="!filtered.length" class="empty-state dash-empty">
            <p v-if="query">No hosts match “{{ query }}”</p>
            <p v-else>No hosts yet — add your first one or import from ~/.ssh/config.</p>
            <button class="btn btn-secondary" @click="dialogs.showImport = true">
              Import from ~/.ssh/config
            </button>
          </div>
          <div v-else class="card-grid">
            <div
              v-for="host in filtered"
              :key="host.id"
              class="host-card"
              :class="{ selected: selectedId === host.id }"
              role="button"
              tabindex="0"
              @click="select(host)"
              @dblclick="sessions.connect(host.id)"
              @keydown.enter="sessions.connect(host.id)"
            >
              <OsAvatar :label="host.label" :os="host.os" :size="40" />
              <span class="host-card-text">
                <span class="host-card-name text-ellipsis">{{ host.label }}</span>
                <span class="host-card-sub text-ellipsis">ssh, {{ host.username }}</span>
              </span>
              <span class="host-card-status" :class="statusFor(host.id)" :title="statusFor(host.id)"></span>
            </div>
          </div>
        </section>
      </div>
    </div>

    <!-- Details panel -->
    <HostDetailsPanel
      v-if="selectedHost"
      :host="selectedHost"
      @close="selectedId = null"
    >
      <template #actions>
        <button class="btn-icon" aria-label="Edit host" title="Edit host" @click="dialogs.openHostDialog(selectedHost!)">
          <Pencil :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </template>
      <template #danger>
        <button class="btn btn-danger panel-delete" @click="deleteSelected">
          <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
          Delete host
        </button>
      </template>
    </HostDetailsPanel>
  </div>
</template>

<style scoped>
.hosts-view {
  display: flex;
  height: 100%;
  min-height: 0;
}

.hosts-main {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.find-bar {
  display: flex;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
}

.find-wrap {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1;
}

.find-icon {
  position: absolute;
  left: 12px;
  color: var(--text-4);
  pointer-events: none;
}

.find-input {
  width: 100%;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 9px 12px 9px 34px;
  font-size: var(--text-md);
  transition: border-color var(--dur-hover) ease;
}

.find-input:hover {
  border-color: var(--border-strong);
}

.find-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.find-input::placeholder {
  color: var(--text-4);
}

.toolbar {
  display: flex;
  gap: var(--space-2);
  padding: 0 var(--space-4) var(--space-3);
}

.dash-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 0 var(--space-4) var(--space-5);
  min-height: 0;
}

.dash-section + .dash-section {
  margin-top: var(--space-5);
}

.section-title {
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--text-1);
  margin-bottom: var(--space-3);
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(250px, 1fr));
  gap: var(--space-3);
}

.group-card,
.host-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  text-align: left;
  cursor: pointer;
  transition: background-color var(--dur-hover) ease, border-color var(--dur-hover) ease;
}

.group-card:hover,
.host-card:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.group-card.on {
  border-color: var(--accent);
  background: var(--accent-bg);
}

.host-card.selected {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.group-avatar {
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
  letter-spacing: 0.02em;
}

.group-avatar {
  background: var(--accent);
}

.group-avatar.all {
  background: var(--bg-surface-3);
  color: var(--text-1);
}

.group-text,
.host-card-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.3;
}

.group-name,
.host-card-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.group-sub,
.host-card-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.host-card-status {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.host-card-status.connected { background: var(--success); }
.host-card-status.connecting { background: var(--warning); }
.host-card-status.error { background: var(--danger); }
.host-card-status.idle { background: transparent; }

.dash-empty {
  padding: var(--space-7);
}

/* Details panel */
.hosts-view > :deep(.details-panel) {
  border-left: 1px solid var(--border);
}

.panel-delete {
  margin-top: var(--space-3);
  width: 100%;
}
</style>
