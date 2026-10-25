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
  PlugZap,
  Download,
  Upload,
} from 'lucide-vue-next'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import HostDetailsPanel from '../components/HostDetailsPanel.vue'
import PlanetAvatar from '../components/PlanetAvatar.vue'
import { useSmartConnect } from '../utils/smartConnect'
import { targetDisplay } from '../utils/parseTarget'
import type { Host, HostGroup } from '../types'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const selectedId = ref<number | null>(null)
// null = root (folders + all hosts) · a group id = inside that folder
const currentGroupId = ref<number | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)
// Direction of the last folder navigation, so the transition mirrors it:
// drilling in slides forward, going back slides backward
const navDir = ref<'fwd' | 'back'>('fwd')

const searching = computed(() => query.value.trim().length > 0)

// Quick connect: the query parsed as user@host:port / domain / IP, matched
// against saved hosts (by exact address or resolved IP)
const {
  target: smartTarget,
  ips: smartIps,
  resolving: smartResolving,
  resolveFailed: smartResolveFailed,
  matches: smartMatches,
} = useSmartConnect(query)
const smartDisplay = computed(() => (smartTarget.value ? targetDisplay(smartTarget.value) : ''))

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

// Root shows every host (folders are just a way in); inside a folder, only
// its hosts. Search filters whichever scope is active.
const visibleHosts = computed(() => {
  const q = query.value.trim().toLowerCase()
  let list = hosts.hosts
  if (currentGroupId.value !== null) {
    list = list.filter((h) => h.group_id === currentGroupId.value)
  }
  if (q) list = list.filter((h) => hostMatches(h, q))
  return [...list].sort(byRecent)
})

// Most recently accessed first; hosts never connected stay alphabetical
function byRecent(a: Host, b: Host): number {
  return (b.last_used_at ?? 0) - (a.last_used_at ?? 0) || a.label.localeCompare(b.label)
}

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
  navDir.value = 'fwd'
  currentGroupId.value = g.id
  query.value = ''
  selectedId.value = null
}

function goRoot() {
  navDir.value = 'back'
  currentGroupId.value = null
  query.value = ''
}

function connectFirst() {
  // A parsed quick-connect target wins over the filtered list
  if (smartMatches.value.length) {
    void sessions.connect(smartMatches.value[0].host.id)
    return
  }
  if (smartTarget.value) {
    openQuickAdd()
    return
  }
  const first = visibleHosts.value[0]
  if (first) void sessions.connect(first.id)
}

function openQuickAdd() {
  const t = smartTarget.value
  if (!t) return
  dialogs.openHostDialog(
    null,
    currentGroupId.value,
    {
      address: t.host,
      port: t.port ?? 22,
      username: t.username,
      ips: t.kind === 'domain' ? smartIps.value : [],
    },
    true,
  )
}

function statusFor(hostId: number): string {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  if (!tab) return 'idle'
  if (tab.status === 'connected') return 'connected'
  if (tab.status === 'connecting' || tab.status === 'hostkey' || tab.status === 'reconnecting') return 'connecting'
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
        <button
          class="btn btn-primary"
          :disabled="!visibleHosts.length && !smartTarget"
          @click="connectFirst"
        >
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
        <span class="toolbar-spacer"></span>
        <button class="btn btn-secondary" @click="dialogs.showImport = true">
          <Download :size="14" :stroke-width="1.5" aria-hidden="true" />
          Import
        </button>
        <button class="btn btn-secondary" @click="dialogs.showExport = true">
          <Upload :size="14" :stroke-width="1.5" aria-hidden="true" />
          Export
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
          <span class="crumb-current" aria-current="true">
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

        <Transition :name="navDir === 'fwd' ? 'nav-fwd' : 'nav-back'" mode="out-in">
          <div :key="currentGroupId ?? 'root'" class="dash-body">
            <!-- Quick connect: target parsed from the search box -->
            <div v-if="smartTarget" class="smart-card">
              <PlugZap :size="15" :stroke-width="1.5" class="smart-icon" aria-hidden="true" />
              <div class="smart-text">
                <span class="smart-title">
                  Connect to <span class="mono">{{ smartDisplay }}</span>
                </span>
                <span class="smart-sub">
                  <template v-if="smartMatches.length">
                    Matches “{{ smartMatches[0].host.label }}”<template v-if="smartMatches[0].reason === 'ip'"> · resolves to {{ smartMatches[0].ip }}</template><template v-if="smartMatches.length > 1"> · +{{ smartMatches.length - 1 }} more</template>
                  </template>
                  <template v-else-if="smartResolving">Resolving address…</template>
                  <template v-else-if="smartResolveFailed">Address didn’t resolve</template>
                  <template v-else>Not saved yet</template>
                </span>
              </div>
              <button class="btn btn-primary smart-btn" @click="connectFirst">
                <template v-if="smartMatches.length">Connect</template>
                <template v-else>
                  <Plus :size="14" :stroke-width="2" aria-hidden="true" />
                  Add &amp; connect
                </template>
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
                  @keydown.space.prevent="openGroup(g)"
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
                <p>No hosts yet — add your first one or import from ~/.ssh/config or a Termius export.</p>
                <button class="btn btn-secondary" @click="dialogs.showImport = true">
                  Import hosts
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
              <p
                v-else-if="searching && !visibleHosts.length && !visibleFolders.length"
                class="quiet-hint"
              >
                No matches for “{{ query }}”
              </p>
              <div v-else-if="visibleHosts.length" class="card-grid">
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
                  <PlanetAvatar :host="host" :size="40" />
                  <span class="host-card-text">
                    <span class="host-card-name text-ellipsis">{{ host.label }}</span>
                    <span class="host-card-sub text-ellipsis">
                      ssh, {{ host.username }}<template v-if="host.group_id && host.group_id !== currentGroupId"> · {{ groupName(host.group_id) }}</template>
                    </span>
                  </span>
                  <span class="host-card-status" :class="statusFor(host.id)" :title="statusFor(host.id)"></span>
                </div>
              </div>
            </section>
          </div>
        </Transition>
      </div>
    </div>

    <!-- Details panel -->
    <Transition name="panel">
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
    </Transition>

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
  /* the details panel slides in from the right edge — keep it clipped */
  overflow: hidden;
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

.toolbar-spacer {
  flex: 1;
}

.dash-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 0 var(--space-4) var(--space-5);
  min-height: 0;
}

.smart-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  margin-bottom: var(--space-5);
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}

.smart-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.smart-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.35;
}

.smart-title {
  font-size: var(--text-sm);
  color: var(--text-1);
}

.smart-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.smart-btn {
  flex-shrink: 0;
}

.dash-section + .dash-section {
  margin-top: var(--space-5);
}

.crumbs {
  position: sticky;
  top: 0;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: var(--space-2) 0;
  margin: 0 calc(-1 * var(--space-4)) var(--space-2);
  padding-left: var(--space-4);
  padding-right: var(--space-4);
  background: var(--bg);
}

/* Folder drill-in / go-back: content slides along the navigation
   direction. Enter ease-out; exits softer and faster, per motion contract. */
.nav-fwd-enter-active,
.nav-back-enter-active {
  transition: transform 180ms var(--ease-out), opacity 180ms var(--ease-out);
}

.nav-fwd-leave-active,
.nav-back-leave-active {
  transition: transform 120ms var(--ease-out), opacity 120ms var(--ease-out);
}

.nav-fwd-enter-from {
  transform: translateX(14px);
  opacity: 0;
}

.nav-fwd-leave-to {
  transform: translateX(-10px);
  opacity: 0;
}

.nav-back-enter-from {
  transform: translateX(-14px);
  opacity: 0;
}

.nav-back-leave-to {
  transform: translateX(10px);
  opacity: 0;
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
