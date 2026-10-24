<script setup lang="ts">
import { computed, nextTick, ref } from 'vue'
import {
  Search,
  Plus,
  ChevronRight,
  Pencil,
  Trash2,
  ScrollText,
  ArrowLeftRight,
  Settings,
  FileDown,
  Server,
  SquareTerminal,
} from 'lucide-vue-next'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import type { Host } from '../types'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const collapsed = ref<Set<number>>(new Set())
const renamingGroup = ref<number | null>(null)
const groupNameValue = ref('')
const groupInput = ref<HTMLInputElement | null>(null)

const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return hosts.hosts
  return hosts.hosts.filter(
    (h) =>
      h.label.toLowerCase().includes(q) ||
      h.address.toLowerCase().includes(q) ||
      h.username.toLowerCase().includes(q),
  )
})

const ungrouped = computed(() => filtered.value.filter((h) => h.group_id === null))

const groupsWithHosts = computed(() =>
  hosts.groups.map((g) => ({
    ...g,
    hosts: filtered.value.filter((h) => h.group_id === g.id),
  })).filter((g) => g.hosts.length > 0 || !query.value.trim()),
)

function statusFor(hostId: number): 'connected' | 'connecting' | 'error' | 'idle' {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  if (!tab) return 'idle'
  if (tab.status === 'connected') return 'connected'
  if (tab.status === 'connecting' || tab.status === 'hostkey') return 'connecting'
  if (tab.status === 'error') return 'error'
  return 'idle'
}

function statusLabel(hostId: number): string {
  const s = statusFor(hostId)
  return s === 'connected' ? 'Connected' : s === 'connecting' ? 'Connecting' : s === 'error' ? 'Error' : ''
}

function toggleGroup(id: number) {
  const next = new Set(collapsed.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  collapsed.value = next
}

function startGroupRename(id: number, current: string) {
  renamingGroup.value = id
  groupNameValue.value = current
  void nextTick(() => groupInput.value?.select())
}

async function commitGroupRename(id: number) {
  const name = groupNameValue.value.trim()
  const editing = renamingGroup.value === id
  renamingGroup.value = null
  if (!editing || !name) return
  await hosts.saveGroup(id, name)
}

function cancelGroupRename() {
  renamingGroup.value = null
}

function editHost(host: Host, e: Event) {
  e.stopPropagation()
  dialogs.openHostDialog(host)
}

async function deleteHost(host: Host, e: Event) {
  e.stopPropagation()
  const ok = await dialogs.confirm({
    title: `Delete “${host.label}”?`,
    message: 'The saved connection will be removed. Active sessions stay open.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  await hosts.removeHost(host.id)
  ui.toast('info', `Deleted “${host.label}”`)
}

async function deleteGroup(id: number, name: string, e: Event) {
  e.stopPropagation()
  const ok = await dialogs.confirm({
    title: `Delete group “${name}”?`,
    message: 'Hosts in this group stay saved — they move to the ungrouped list.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (ok) await hosts.removeGroup(id)
}

function isActive(hostId: number): boolean {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  return !!tab && tab.id === sessions.activeId
}
</script>

<template>
  <aside class="sidebar material">
    <div class="sidebar-top">
      <div class="new-host-row">
        <button class="btn btn-primary new-host" @click="dialogs.openHostDialog(null, null)">
          <Plus :size="15" :stroke-width="2" aria-hidden="true" />
          New Host
        </button>
        <button
          class="btn btn-secondary import-btn"
          aria-label="Import from SSH config"
          title="Import from ~/.ssh/config"
          @click="dialogs.showImport = true"
        >
          <FileDown :size="15" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>

      <div class="search-wrap">
        <Search :size="14" :stroke-width="1.5" class="search-icon" aria-hidden="true" />
        <input
          v-model="query"
          class="search-input"
          type="text"
          placeholder="Search hosts"
          spellcheck="false"
          aria-label="Search hosts"
        />
      </div>
    </div>

    <nav class="host-list" aria-label="Hosts">
      <!-- Pinned local terminal -->
      <button class="host-item local-item" @click="sessions.connectLocal()">
        <span class="local-chip" aria-hidden="true">
          <SquareTerminal :size="13" :stroke-width="1.5" />
        </span>
        <span class="host-text">
          <span class="host-label">Local</span>
          <span class="host-addr">this PC</span>
        </span>
      </button>

      <!-- Groups -->
      <section v-for="group in groupsWithHosts" :key="group.id" class="group">
        <div class="group-header">
          <button class="group-toggle" @click="toggleGroup(group.id)">
            <ChevronRight
              :size="12"
              :stroke-width="2"
              class="chev"
              :class="{ open: !collapsed.has(group.id) }"
              aria-hidden="true"
            />
            <span v-if="renamingGroup !== group.id" class="group-name text-ellipsis">{{ group.name }}</span>
            <form v-else class="group-rename" @submit.prevent="commitGroupRename(group.id)">
              <input
                ref="groupInput"
                v-model="groupNameValue"
                class="group-rename-input"
                type="text"
                spellcheck="false"
                @keydown.esc="cancelGroupRename"
                @blur="commitGroupRename(group.id)"
              />
            </form>
            <span class="group-count">{{ group.hosts.length }}</span>
          </button>
          <div class="group-actions">
            <button class="btn-icon" :aria-label="`Rename group ${group.name}`" @click.stop="startGroupRename(group.id, group.name)">
              <Pencil :size="12" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Delete group ${group.name}`" @click.stop="deleteGroup(group.id, group.name, $event)">
              <Trash2 :size="12" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
        </div>

        <div v-show="!collapsed.has(group.id)" class="group-items">
          <button
            v-for="host in group.hosts"
            :key="host.id"
            class="host-item grouped"
            :class="{ selected: isActive(host.id) }"
            :title="statusLabel(host.id) || undefined"
            @click="sessions.connect(host.id)"
          >
            <span class="status-dot" :class="statusFor(host.id)"></span>
            <span class="host-text">
              <span class="host-label text-ellipsis">{{ host.label }}</span>
              <span class="host-addr text-ellipsis">{{ host.address }}</span>
            </span>
            <span class="host-actions" @click.stop>
              <button class="btn-icon" :aria-label="`Edit ${host.label}`" @click="editHost(host, $event)">
                <Pencil :size="12" :stroke-width="1.5" aria-hidden="true" />
              </button>
              <button class="btn-icon host-delete" :aria-label="`Delete ${host.label}`" @click="deleteHost(host, $event)">
                <Trash2 :size="12" :stroke-width="1.5" aria-hidden="true" />
              </button>
            </span>
          </button>
        </div>
      </section>

      <!-- Ungrouped -->
      <section v-if="ungrouped.length" class="group">
        <div class="section-label list-label">Ungrouped</div>
        <div class="group-items">
          <button
            v-for="host in ungrouped"
            :key="host.id"
            class="host-item"
            :class="{ selected: isActive(host.id) }"
            :title="statusLabel(host.id) || undefined"
            @click="sessions.connect(host.id)"
          >
            <span class="status-dot" :class="statusFor(host.id)"></span>
            <span class="host-text">
              <span class="host-label text-ellipsis">{{ host.label }}</span>
              <span class="host-addr text-ellipsis">{{ host.address }}</span>
            </span>
            <span class="host-actions" @click.stop>
              <button class="btn-icon" :aria-label="`Edit ${host.label}`" @click="editHost(host, $event)">
                <Pencil :size="12" :stroke-width="1.5" aria-hidden="true" />
              </button>
              <button class="btn-icon host-delete" :aria-label="`Delete ${host.label}`" @click="deleteHost(host, $event)">
                <Trash2 :size="12" :stroke-width="1.5" aria-hidden="true" />
              </button>
            </span>
          </button>
        </div>
      </section>

      <!-- Empty search -->
      <div v-if="hosts.loaded && !filtered.length" class="list-empty">
        <Server :size="20" :stroke-width="1.5" aria-hidden="true" />
        <p>{{ query ? 'No hosts match your search' : 'No hosts yet' }}</p>
        <button v-if="!query" class="link-btn" @click="dialogs.showImport = true">
          Import from ~/.ssh/config
        </button>
      </div>
    </nav>

    <footer class="sidebar-footer">
      <button class="footer-btn" @click="dialogs.showSnippets = true">
        <ScrollText :size="15" :stroke-width="1.5" aria-hidden="true" />
        Snippets
      </button>
      <button class="footer-btn" @click="dialogs.showForwards = true">
        <ArrowLeftRight :size="15" :stroke-width="1.5" aria-hidden="true" />
        Forwards
      </button>
      <button class="footer-btn" aria-label="Settings" @click="dialogs.showSettings = true">
        <Settings :size="15" :stroke-width="1.5" aria-hidden="true" />
        Settings
      </button>
    </footer>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  width: var(--sidebar-width);
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  min-height: 0;
}

.sidebar-top {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  border-bottom: 1px solid var(--border);
}

.new-host-row {
  display: flex;
  gap: var(--space-2);
}

.new-host {
  flex: 1;
  min-width: 0;
}

.import-btn {
  padding: 7px 10px;
  color: var(--text-2);
}

.search-wrap {
  position: relative;
  display: flex;
  align-items: center;
}

.search-icon {
  position: absolute;
  left: 9px;
  color: var(--text-4);
  pointer-events: none;
}

.search-input {
  width: 100%;
  background: transparent;
  border: none;
  padding: 5px 8px 5px 27px;
  font-size: var(--text-base);
  transition: background-color var(--dur-hover) ease;
}

.search-input:hover {
  background: var(--bg-hover);
}

.search-input:focus {
  outline: none;
  background: var(--bg-input);
  box-shadow: 0 0 0 1px var(--border-strong);
}

.search-input::placeholder {
  color: var(--text-4);
}

.host-list {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-2);
  min-height: 0;
}

.list-label {
  padding: var(--space-3) var(--space-1) var(--space-1);
  font-size: 0.625rem;
}

.group + .group {
  margin-top: var(--space-2);
}

/* Group header: quiet text row, chevron + name + count */
.group-header {
  display: flex;
  align-items: center;
  height: 26px;
  border-radius: var(--radius-sm);
}

.group-header:hover {
  background: var(--bg-hover);
}

.group-toggle {
  display: flex;
  align-items: center;
  gap: 5px;
  flex: 1;
  min-width: 0;
  height: 100%;
  padding: 0 var(--space-1) 0 4px;
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: 0.02em;
  color: var(--text-3);
  text-align: left;
  border-radius: var(--radius-sm);
}

.chev {
  transition: transform var(--dur-hover) var(--ease-out);
  flex-shrink: 0;
  color: var(--text-4);
}

.chev.open {
  transform: rotate(90deg);
}

.group-name {
  min-width: 0;
}

.group-count {
  margin-left: auto;
  font-size: 0.625rem;
  color: var(--text-4);
  font-weight: 400;
  font-variant-numeric: tabular-nums;
}

.group-actions {
  display: flex;
  gap: 1px;
  padding-right: 2px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.group-header:hover .group-actions,
.group-header:focus-within .group-actions {
  opacity: 1;
}

.group-rename {
  display: flex;
  flex: 1;
  min-width: 0;
  padding: 1px var(--space-1);
}

.group-rename-input {
  width: 100%;
  background: var(--bg-input);
  border: 1px solid var(--accent);
  border-radius: var(--radius-sm);
  padding: 2px 6px;
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--text-1);
}

.group-rename-input:focus {
  outline: none;
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.group-items {
  display: flex;
  flex-direction: column;
  gap: 1px;
}

/* Host rows */
.host-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 5px var(--space-2);
  border-radius: var(--radius-md);
  text-align: left;
  transition: background-color var(--dur-hover) ease;
}

.host-item.grouped {
  margin-left: 10px;
  width: calc(100% - 10px);
}

.host-item:hover {
  background: var(--bg-hover);
}

.host-item.selected {
  background: var(--bg-selected);
}

.host-item:active {
  transform: scale(0.96);
  transition: transform var(--dur-press) ease-out, background-color var(--dur-hover) ease;
}

.local-chip {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border-radius: var(--radius-sm);
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.host-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.25;
}

.host-label {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
}

.host-addr {
  font-size: 0.6875rem;
  color: var(--text-4);
}

.host-actions {
  display: flex;
  gap: 1px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.host-item:hover .host-actions,
.host-item:focus-within .host-actions {
  opacity: 1;
}

.host-delete:hover {
  color: var(--danger);
}

.list-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-6) var(--space-4);
  color: var(--text-4);
  font-size: var(--text-sm);
}

.link-btn {
  font-size: var(--text-sm);
  color: var(--accent);
  text-decoration: underline;
  padding: 2px 4px;
  border-radius: 4px;
}

.sidebar-footer {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: var(--space-2);
  border-top: 1px solid var(--border);
}

.footer-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-2);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.footer-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}
</style>
