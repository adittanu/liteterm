<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  Search,
  Plus,
  SquareTerminal,
  Pencil,
  Trash2,
  Folder,
  FolderPlus,
  ChevronLeft,
  ChevronRight,
  MoreHorizontal,
} from 'lucide-vue-next'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import HostDetailsPanel from '../components/HostDetailsPanel.vue'
import OsAvatar from '../components/OsAvatar.vue'
import type { Host, HostGroup } from '../types'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const selectedId = ref<number | null>(null)
// null = root (folders + ungrouped hosts) · a group id = inside that folder
const currentGroupId = ref<number | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)

const searching = computed(() => query.value.trim().length > 0)

const currentGroup = computed(
  () => hosts.groups.find((g) => g.id === currentGroupId.value) ?? null,
)

const groupCards = computed(() =>
  hosts.groups.map((g) => ({
    ...g,
    count: hosts.hosts.filter((h) => h.group_id === g.id).length,
  })),
)

function hostMatches(h: Host, q: string): boolean {
  return (
    h.label.toLowerCase().includes(q) ||
    h.address.toLowerCase().includes(q) ||
    h.username.toLowerCase().includes(q)
  )
}

// Root: folders (searched by name). Hidden entirely while inside a group.
const visibleFolders = computed(() => {
  if (currentGroupId.value !== null) return []
  const q = query.value.trim().toLowerCase()
  if (!q) return groupCards.value
  return groupCards.value.filter((g) => g.name.toLowerCase().includes(q))
})

// Root search spans the whole library; browsing root shows ungrouped hosts only.
// Inside a folder, only that folder's hosts (searched within it).
const visibleHosts = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (currentGroupId.value !== null) {
    const list = hosts.hosts.filter((h) => h.group_id === currentGroupId.value)
    return q ? list.filter((h) => hostMatches(h, q)) : list
  }
  if (q) return hosts.hosts.filter((h) => hostMatches(h, q))
  return hosts.hosts.filter((h) => h.group_id === null)
})

function groupName(id: number | null): string {
  if (id === null) return ''
  return hosts.groups.find((g) => g.id === id)?.name ?? ''
}

const selectedHost = computed(
  () => hosts.hosts.find((h) => h.id === selectedId.value) ?? null,
)

function select(host: Host) {
  selectedId.value = host.id
}

// Entering or leaving a folder ends the search — intent shifts to browsing.
function openGroup(g: HostGroup) {
  currentGroupId.value = g.id
  query.value = ''
  selectedId.value = null
}

function goRoot() {
  currentGroupId.value = null
  query.value = ''
}

function connectFirst() {
  const first = visibleHosts.value[0]
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

function openFolderMenu(g: HostGroup, e: MouseEvent) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: 'New host here', icon: Plus, action: () => dialogs.openHostDialog(null, g.id) },
      { label: 'Rename group', icon: Pencil, action: () => dialogs.openGroupDialog(g) },
      { label: 'Delete group', icon: Trash2, danger: true, action: () => void deleteGroup(g) },
    ],
  }
}

async function deleteGroup(g: HostGroup) {
  const ok = await dialogs.confirm({
    title: `Delete “${g.name}”?`,
    message: 'The group is removed. Its hosts are kept and become ungrouped.',
    confirmLabel: 'Delete group',
    danger: true,
  })
  if (!ok) return
  if (currentGroupId.value === g.id) currentGroupId.value = null
  await hosts.removeGroup(g.id)
  ui.toast('info', 'Group deleted')
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
        <button class="btn btn-primary" :disabled="!visibleHosts.length" @click="connectFirst">
          Connect
        </button>
      </div>

      <!-- Toolbar -->
      <div class="toolbar">
        <button class="btn btn-secondary" @click="dialogs.openHostDialog(null, currentGroupId)">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          {{ currentGroup ? 'New host in group' : 'New host' }}
        </button>
        <button class="btn btn-secondary" @click="sessions.connectLocal()">
          <SquareTerminal :size="14" :stroke-width="1.5" aria-hidden="true" />
          Local terminal
        </button>
      </div>

      <div class="dash-scroll">
        <!-- Inside a group: breadcrumb back to root -->
        <div v-if="currentGroup" class="crumbs">
          <button class="btn-icon" aria-label="Back to all hosts" @click="goRoot">
            <ChevronLeft :size="16" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="crumb-link" @click="goRoot">Hosts</button>
          <ChevronRight :size="13" :stroke-width="1.5" class="crumb-sep" aria-hidden="true" />
          <span class="crumb-current">
            <Folder :size="14" :stroke-width="1.5" aria-hidden="true" />
            <span class="text-ellipsis">{{ currentGroup.name }}</span>
          </span>
          <span class="crumb-count">
            {{ visibleHosts.length }} {{ visibleHosts.length === 1 ? 'host' : 'hosts' }}
          </span>
          <span class="crumbs-spacer"></span>
          <button
            class="btn-icon"
            aria-label="Group options"
            @click="openFolderMenu(currentGroup, $event)"
          >
            <MoreHorizontal :size="16" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>

        <!-- Root: group folders -->
        <section
          v-if="!currentGroup && (!searching || visibleFolders.length)"
          class="dash-section"
        >
          <h3 class="section-title">{{ searching ? 'Matching groups' : 'Groups' }}</h3>
          <div class="card-grid">
            <div
              v-for="g in visibleFolders"
              :key="g.id"
              class="folder-card"
              role="button"
              tabindex="0"
              :aria-label="`Open group ${g.name}`"
              @click="openGroup(g)"
              @keydown.enter="openGroup(g)"
              @contextmenu.prevent="openFolderMenu(g, $event)"
            >
              <span class="folder-avatar">
                <Folder :size="19" :stroke-width="1.5" aria-hidden="true" />
              </span>
              <span class="folder-text">
                <span class="folder-name text-ellipsis">{{ g.name }}</span>
                <span class="folder-sub">{{ g.count }} {{ g.count === 1 ? 'host' : 'hosts' }}</span>
              </span>
              <span class="folder-go">
                <ChevronRight :size="15" :stroke-width="1.5" class="folder-chev" aria-hidden="true" />
                <button
                  class="btn-icon folder-more"
                  aria-label="Group options"
                  @click.stop="openFolderMenu(g, $event)"
                >
                  <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
                </button>
              </span>
            </div>
            <button
              v-if="!searching"
              class="folder-card new-folder"
              @click="dialogs.openGroupDialog(null)"
            >
              <span class="folder-avatar new">
                <FolderPlus :size="19" :stroke-width="1.5" aria-hidden="true" />
              </span>
              <span class="folder-text">
                <span class="folder-name">New group</span>
                <span class="folder-sub">Organize hosts into folders</span>
              </span>
            </button>
          </div>
        </section>

        <!-- Hosts -->
        <section class="dash-section">
          <h3 class="section-title">
            <template v-if="currentGroup">{{ searching ? 'Matches in this group' : 'Hosts' }}</template>
            <template v-else>{{ searching ? 'Matching hosts' : 'Hosts' }}</template>
          </h3>

          <div
            v-if="!hosts.hosts.length && !searching"
            class="empty-state dash-empty"
          >
            <p>No hosts yet — add your first one or import from ~/.ssh/config.</p>
            <button class="btn btn-secondary" @click="dialogs.showImport = true">
              Import from ~/.ssh/config
            </button>
          </div>
          <div
            v-else-if="currentGroup && !visibleHosts.length && !searching"
            class="empty-state dash-empty"
          >
            <p>Nothing in “{{ currentGroup.name }}” yet.</p>
            <button class="btn btn-secondary" @click="dialogs.openHostDialog(null, currentGroupId)">
              <Plus :size="14" :stroke-width="2" aria-hidden="true" />
              Add host to this group
            </button>
          </div>
          <p v-else-if="!visibleHosts.length && searching" class="quiet-hint">
            No matches for “{{ query }}”
          </p>
          <p
            v-else-if="!visibleHosts.length && !currentGroup && !searching"
            class="quiet-hint"
          >
            Hosts without a group appear here — everything else lives inside a folder.
          </p>
          <div v-else class="card-grid">
            <div
              v-for="host in visibleHosts"
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
                <span class="host-card-sub text-ellipsis">
                  ssh, {{ host.username }}<template v-if="searching && host.group_id"> · {{ groupName(host.group_id) }}</template>
                </span>
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

    <ContextMenu
      v-if="menu"
      :x="menu.x"
      :y="menu.y"
      :items="menu.items"
      @close="menu = null"
    />
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

.crumbs + .dash-section {
  margin-top: var(--space-4);
}

.crumbs {
  position: sticky;
  top: 0;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: var(--space-2) 0;
  margin: 0 calc(-1 * var(--space-4));
  padding-left: var(--space-4);
  padding-right: var(--space-4);
  background: var(--bg);
}

.crumb-link {
  font-size: var(--text-base);
  color: var(--text-3);
  padding: 3px 6px;
  border-radius: var(--radius-sm);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.crumb-link:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.crumb-sep {
  color: var(--text-4);
  flex-shrink: 0;
}

.crumb-current {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  padding: 3px 6px;
}

.crumb-current svg {
  color: var(--accent);
  flex-shrink: 0;
}

.crumb-count {
  font-size: var(--text-xs);
  color: var(--text-4);
  margin-left: 4px;
}

.crumbs-spacer {
  flex: 1;
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

.folder-card,
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

.folder-card:hover,
.host-card:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.folder-card:focus-visible,
.host-card:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

.host-card.selected {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.folder-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: var(--radius-md);
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.folder-avatar.new {
  background: var(--bg-surface-3);
  color: var(--text-3);
}

.folder-card.new-folder {
  border-style: dashed;
  background: transparent;
  box-shadow: none;
  color: var(--text-3);
}

.folder-card.new-folder:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text-1);
}

.folder-text,
.host-card-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.3;
}

.folder-name,
.host-card-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.new-folder .folder-name {
  color: inherit;
}

.folder-sub,
.host-card-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.folder-go {
  position: relative;
  width: 28px;
  height: 28px;
  flex-shrink: 0;
}

.folder-chev,
.folder-more {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: opacity var(--dur-hover) ease;
}

.folder-chev {
  color: var(--text-4);
}

.folder-more {
  opacity: 0;
  border-radius: var(--radius-sm);
}

.folder-more:hover {
  background: var(--bg-active);
}

.folder-card:hover .folder-chev {
  opacity: 0;
}

.folder-card:hover .folder-more {
  opacity: 1;
}

.folder-card:focus-visible .folder-more,
.folder-card:focus-within .folder-more {
  opacity: 1;
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

.quiet-hint {
  font-size: var(--text-sm);
  color: var(--text-4);
  padding: var(--space-2) 0;
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
