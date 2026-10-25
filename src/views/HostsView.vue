<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  ClipboardCopy,
  Copy,
  Download,
  Folder,
  FolderPlus,
  FolderTree,
  LayoutGrid,
  MoreHorizontal,
  Pencil,
  Play,
  Plus,
  PlugZap,
  Rows3,
  Search,
  Server,
  SquareTerminal,
  Trash2,
  Upload,
  X,
} from 'lucide-vue-next'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import HostDetailsPanel from '../components/HostDetailsPanel.vue'
import PlanetAvatar from '../components/PlanetAvatar.vue'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { useSmartConnect } from '../utils/smartConnect'
import { targetDisplay } from '../utils/parseTarget'
import { fuzzy, highlight } from '../utils/fuzzy'
import { copyText, plural, relativeAge } from '../utils/pageHelpers'
import { comboFor } from '../services/shortcuts'
import type { Host, HostGroup } from '../types'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

type GroupFilter = 'all' | 'ungrouped' | number
type Layout = 'grid' | 'list'

const query = ref('')
const selectedId = ref<number | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)
const searchEl = ref<HTMLInputElement | null>(null)
const bodyEl = ref<HTMLElement | null>(null)
const focusKey = ref<number | null>(null)

const layout = ref<Layout>(localStorage.getItem('hosts.layout') === 'list' ? 'list' : 'grid')
watch(layout, (v) => localStorage.setItem('hosts.layout', v))

const groupFilter = ref<GroupFilter>(readGroupFilter())
function readGroupFilter(): GroupFilter {
  const raw = localStorage.getItem('hosts.group')
  if (raw === 'ungrouped') return 'ungrouped'
  const n = Number(raw)
  return raw && Number.isInteger(n) ? n : 'all'
}
watch(groupFilter, (v) => localStorage.setItem('hosts.group', String(v)))

// A remembered group that no longer exists falls back to All
watch(
  () => hosts.groups.map((g) => g.id).join(','),
  () => {
    const g = groupFilter.value
    if (typeof g === 'number' && hosts.loaded && !hosts.groups.some((x) => x.id === g)) groupFilter.value = 'all'
  },
  { immediate: true },
)

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

// ---- status ----

type Status = 'connected' | 'connecting' | 'error' | 'idle'

function statusFor(hostId: number): Status {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  if (!tab) return 'idle'
  if (tab.status === 'connected') return 'connected'
  if (tab.status === 'connecting' || tab.status === 'hostkey') return 'connecting'
  if (tab.status === 'error') return 'error'
  return 'idle'
}

const statusLabel: Record<Status, string> = {
  connected: 'Connected',
  connecting: 'Connecting…',
  error: 'Connection failed',
  idle: '',
}

// ---- groups ----

const groupCounts = computed(() => {
  const m = new Map<number | null, number>()
  for (const h of hosts.hosts) m.set(h.group_id, (m.get(h.group_id) ?? 0) + 1)
  return m
})
const ungroupedCount = computed(() => groupCounts.value.get(null) ?? 0)
const currentGroup = computed(() =>
  typeof groupFilter.value === 'number' ? hosts.groups.find((g) => g.id === groupFilter.value) ?? null : null,
)

function groupName(id: number | null): string {
  if (id === null) return ''
  return hosts.groups.find((g) => g.id === id)?.name ?? ''
}

// ---- what's listed ----

interface Row {
  host: Host
  status: Status
  label: { text: string; hit: boolean }[]
  score: number
}

const scoped = computed(() => {
  const g = groupFilter.value
  if (g === 'all') return hosts.hosts
  if (g === 'ungrouped') return hosts.hosts.filter((h) => h.group_id === null)
  return hosts.hosts.filter((h) => h.group_id === g)
})

const rows = computed<Row[]>(() => {
  const q = query.value.trim()
  const out: Row[] = []
  for (const host of scoped.value) {
    let score = 0
    let hits: number[] = []
    if (q) {
      const l = fuzzy(q, host.label)
      const a = fuzzy(q, `${host.username}@${host.address}`)
      const best = Math.max(l?.score ?? -Infinity, (a?.score ?? -Infinity) * 0.85, (fuzzy(q, groupName(host.group_id))?.score ?? -Infinity) * 0.5)
      if (best === -Infinity) continue
      score = best
      hits = l && l.score >= (a?.score ?? -Infinity) * 0.85 ? l.hits : []
    }
    out.push({ host, status: statusFor(host.id), label: highlight(host.label, hits), score })
  }
  return out.sort((a, b) =>
    q
      ? b.score - a.score
      : (b.host.last_used_at ?? 0) - (a.host.last_used_at ?? 0) || a.host.label.localeCompare(b.host.label),
  )
})

// Live sessions get their own section in the unfiltered view
const showActiveSection = computed(() => !searching.value && rows.value.some((r) => r.status !== 'idle'))
const activeRows = computed(() => (showActiveSection.value ? rows.value.filter((r) => r.status !== 'idle') : []))
const restRows = computed(() => (showActiveSection.value ? rows.value.filter((r) => r.status === 'idle') : rows.value))
const ordered = computed(() => [...activeRows.value, ...restRows.value])

const sectionTitle = computed(() => {
  if (searching.value) return 'Matching hosts'
  if (currentGroup.value) return currentGroup.value.name
  if (groupFilter.value === 'ungrouped') return 'Ungrouped'
  return showActiveSection.value ? 'All hosts' : 'Hosts'
})

const selectedHost = computed(() => hosts.hosts.find((h) => h.id === selectedId.value) ?? null)

// ---- actions ----

function connect(host: Host) {
  void sessions.connect(host.id)
}

function openSftp(host: Host) {
  ui.openSftpFor(host.id)
}

function sshCommand(h: Host): string {
  return `ssh ${h.username}@${h.address}${h.port !== 22 ? ` -p ${h.port}` : ''}`
}

async function duplicate(h: Host) {
  try {
    const copy = await hosts.saveHost({
      group_id: h.group_id,
      label: `${h.label} copy`,
      address: h.address,
      port: h.port,
      username: h.username,
      auth_method: h.auth_method,
      key_path: h.key_path,
      credential_id: h.credential_id,
      save_password: false,
    })
    selectedId.value = copy.id
    ui.toast('success', `Duplicated “${h.label}”${h.has_password && !h.credential_id ? ' — the password isn’t copied' : ''}`)
  } catch (e) {
    ui.toast('error', `Could not duplicate: ${errorText(e)}`)
  }
}

async function removeHost(host: Host) {
  const ok = await dialogs.confirm({
    title: `Delete “${host.label}”?`,
    message: 'The saved connection is removed. Open sessions stay open.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  if (selectedId.value === host.id) selectedId.value = null
  await hosts.removeHost(host.id)
  ui.toast('info', `Deleted “${host.label}”`)
}

function newHost() {
  dialogs.openHostDialog(null, typeof groupFilter.value === 'number' ? groupFilter.value : null)
}

/** Enter in the search box: a quick-connect target wins, else the top match. */
function connectFirst() {
  if (smartMatches.value.length) return connect(smartMatches.value[0].host)
  if (smartTarget.value) return openQuickAdd()
  const first = ordered.value[0]
  if (first) connect(first.host)
}

function openQuickAdd() {
  const t = smartTarget.value
  if (!t) return
  dialogs.openHostDialog(
    null,
    typeof groupFilter.value === 'number' ? groupFilter.value : null,
    { address: t.host, port: t.port ?? 22, username: t.username, ips: t.kind === 'domain' ? smartIps.value : [] },
    true,
  )
}

// ---- menus ----

function hostMenu(host: Host, e: MouseEvent) {
  selectedId.value = host.id
  const live = statusFor(host.id) === 'connected'
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: live ? 'Open terminal' : 'Connect', icon: Play, action: () => connect(host) },
      { label: 'Browse files (SFTP)', icon: FolderTree, action: () => openSftp(host) },
      { label: 'Edit', icon: Pencil, action: () => dialogs.openHostDialog(host) },
      { label: 'Duplicate', icon: Copy, action: () => void duplicate(host) },
      { label: 'Copy SSH command', icon: ClipboardCopy, action: () => void copyText(sshCommand(host), 'SSH command copied') },
      { label: 'Delete', icon: Trash2, danger: true, action: () => void removeHost(host) },
    ],
  }
}

function menuAt(e: MouseEvent, items: MenuItem[], alignRight = false) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu.value = { x: alignRight ? r.right - 200 : r.left, y: r.bottom + 6, items }
}

function moreMenu(e: MouseEvent) {
  menuAt(
    e,
    [
      { label: 'New group', icon: FolderPlus, action: () => dialogs.openGroupDialog(null) },
      { label: 'Import hosts…', icon: Download, action: () => (dialogs.showImport = true) },
      { label: 'Export hosts…', icon: Upload, action: () => (dialogs.showExport = true) },
    ],
    true,
  )
}

function groupMenu(g: HostGroup, e: MouseEvent) {
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
  const n = groupCounts.value.get(g.id) ?? 0
  const ok = await dialogs.confirm({
    title: `Delete group “${g.name}”?`,
    message: n ? `Its ${plural(n, 'host')} are kept and become ungrouped.` : 'The group is empty.',
    confirmLabel: 'Delete group',
    danger: true,
  })
  if (!ok) return
  if (groupFilter.value === g.id) groupFilter.value = 'all'
  await hosts.removeGroup(g.id)
  ui.toast('info', `Deleted group “${g.name}”`)
}

// ---- keyboard: arrows move across the grid, Enter connects ----

function cards(): HTMLElement[] {
  return [...(bodyEl.value?.querySelectorAll<HTMLElement>('[data-host-id]') ?? [])]
}

function focusCard(el: HTMLElement | undefined) {
  if (!el) return
  focusKey.value = Number(el.dataset.hostId)
  el.focus()
  el.scrollIntoView({ block: 'nearest' })
}

function columns(list: HTMLElement[]): number {
  if (layout.value === 'list' || list.length < 2) return 1
  const top = list[0].offsetTop
  const n = list.findIndex((c) => c.offsetTop !== top)
  return n === -1 ? list.length : n
}

function onCardKey(e: KeyboardEvent, host: Host) {
  const list = cards()
  const i = list.findIndex((c) => Number(c.dataset.hostId) === host.id)
  const cols = columns(list)
  const go = (j: number) => {
    e.preventDefault()
    focusCard(list[Math.max(0, Math.min(list.length - 1, j))])
  }
  switch (e.key) {
    case 'ArrowRight': return go(i + 1)
    case 'ArrowLeft': return go(i - 1)
    case 'ArrowDown': return go(i + cols)
    case 'ArrowUp':
      if (i - cols < 0) {
        e.preventDefault()
        return searchEl.value?.focus()
      }
      return go(i - cols)
    case 'Home': return go(0)
    case 'End': return go(list.length - 1)
    case 'Enter':
      e.preventDefault()
      return connect(host)
    case ' ':
      e.preventDefault()
      selectedId.value = selectedId.value === host.id ? null : host.id
      return
    case 'Delete':
      e.preventDefault()
      return void removeHost(host)
    case 'F2':
      e.preventDefault()
      return dialogs.openHostDialog(host)
  }
}

function onSearchKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    connectFirst()
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    focusCard(cards()[0])
  } else if (e.key === 'Escape' && query.value) {
    e.stopPropagation()
    query.value = ''
  }
}

// "/" or Ctrl+F jumps to search from anywhere on the page
function onGlobalKey(e: KeyboardEvent) {
  if (ui.activeView !== 'hosts') return
  const t = e.target as HTMLElement | null
  const typing = t?.closest('input, textarea, [contenteditable="true"]')
  if ((e.key === '/' && !typing) || (e.ctrlKey && !e.shiftKey && e.key.toLowerCase() === 'f')) {
    if (document.querySelector('[role="dialog"]')) return
    e.preventDefault()
    searchEl.value?.focus()
    searchEl.value?.select()
  }
}

onMounted(() => window.addEventListener('keydown', onGlobalKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onGlobalKey))

// Filtering never strands keyboard focus on a card that vanished
watch(ordered, () => {
  if (focusKey.value !== null && !ordered.value.some((r) => r.host.id === focusKey.value)) focusKey.value = null
})

function tabIndexFor(id: number, i: number): number {
  if (focusKey.value !== null) return focusKey.value === id ? 0 : -1
  return i === 0 ? 0 : -1
}

function pickGroup(g: GroupFilter) {
  groupFilter.value = g
  void nextTick(() => bodyEl.value?.scrollTo({ top: 0 }))
}
</script>

<template>
  <div class="hosts-view">
    <div class="hosts-main">
      <!-- ===== Header ===== -->
      <header class="hv-header">
        <div class="hv-heading">
          <h2 class="hv-title">
            Hosts
            <span v-if="hosts.hosts.length" class="hv-count">{{ hosts.hosts.length }}</span>
          </h2>
        </div>
        <div class="hv-actions">
          <button class="btn btn-secondary" :title="`Open a terminal on this PC (${comboFor('newLocal', ui.settings)})`" @click="sessions.connectLocal()">
            <SquareTerminal :size="14" :stroke-width="1.5" aria-hidden="true" />
            Local terminal
          </button>
          <button class="btn btn-primary" @click="newHost">
            <Plus :size="14" :stroke-width="2" aria-hidden="true" />
            {{ currentGroup ? `New host in ${currentGroup.name}` : 'New host' }}
          </button>
          <button class="btn-icon hv-more" aria-label="More host actions" title="Groups, import & export" @click="moreMenu">
            <MoreHorizontal :size="16" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
      </header>

      <!-- ===== Search = filter + quick connect ===== -->
      <div class="hv-toolbar">
        <label class="find">
          <Search :size="15" :stroke-width="1.5" class="find-icon" aria-hidden="true" />
          <input
            ref="searchEl"
            v-model="query"
            class="find-input"
            type="text"
            placeholder="Search hosts, or type user@host to connect"
            spellcheck="false"
            autocomplete="off"
            data-host-search
            aria-label="Search hosts or quick connect"
            @keydown="onSearchKey"
          />
          <button v-if="query" class="find-clear" aria-label="Clear search" @click="query = ''; searchEl?.focus()">
            <X :size="11" :stroke-width="2" aria-hidden="true" />
          </button>
          <kbd v-else class="find-kbd" aria-hidden="true">/</kbd>
        </label>

        <div class="seg" role="group" aria-label="Layout">
          <button :class="{ on: layout === 'grid' }" :aria-pressed="layout === 'grid'" aria-label="Grid" title="Grid" @click="layout = 'grid'">
            <LayoutGrid :size="14" :stroke-width="1.75" aria-hidden="true" />
          </button>
          <button :class="{ on: layout === 'list' }" :aria-pressed="layout === 'list'" aria-label="List" title="List" @click="layout = 'list'">
            <Rows3 :size="14" :stroke-width="1.75" aria-hidden="true" />
          </button>
        </div>
      </div>

      <!-- ===== Group filter chips ===== -->
      <nav v-if="hosts.groups.length" class="chips" aria-label="Groups">
        <button class="chip-btn" :class="{ on: groupFilter === 'all' }" :aria-pressed="groupFilter === 'all'" @click="pickGroup('all')">
          All <span class="chip-n">{{ hosts.hosts.length }}</span>
        </button>
        <button
          v-for="g in hosts.groups"
          :key="g.id"
          class="chip-btn"
          :class="{ on: groupFilter === g.id }"
          :aria-pressed="groupFilter === g.id"
          @click="pickGroup(g.id)"
          @contextmenu.prevent="groupMenu(g, $event)"
        >
          <Folder :size="12" :stroke-width="1.75" aria-hidden="true" />
          {{ g.name }}
          <span class="chip-n">{{ groupCounts.get(g.id) ?? 0 }}</span>
        </button>
        <button
          v-if="ungroupedCount"
          class="chip-btn"
          :class="{ on: groupFilter === 'ungrouped' }"
          :aria-pressed="groupFilter === 'ungrouped'"
          @click="pickGroup('ungrouped')"
        >
          Ungrouped <span class="chip-n">{{ ungroupedCount }}</span>
        </button>
        <button class="chip-btn add" title="New group" @click="dialogs.openGroupDialog(null)">
          <Plus :size="12" :stroke-width="2" aria-hidden="true" />
          Group
        </button>
      </nav>

      <div ref="bodyEl" class="hv-scroll">
        <!-- Quick connect: the search box parsed as a target -->
        <div v-if="smartTarget" class="smart">
          <span class="smart-icon"><PlugZap :size="16" :stroke-width="1.6" aria-hidden="true" /></span>
          <div class="smart-text">
            <span class="smart-title">
              {{ smartMatches.length ? 'Connect to' : 'New connection to' }}
              <span class="mono">{{ smartDisplay }}</span>
            </span>
            <span class="smart-sub">
              <template v-if="smartMatches.length">
                Saved as “{{ smartMatches[0].host.label }}”<template v-if="smartMatches[0].reason === 'ip'"> · resolves to {{ smartMatches[0].ip }}</template><template v-if="smartMatches.length > 1"> · +{{ smartMatches.length - 1 }} more</template>
              </template>
              <template v-else-if="smartResolving">Resolving address…</template>
              <template v-else-if="smartResolveFailed">Address didn’t resolve — you can still try</template>
              <template v-else>Not saved yet — it will be added to your hosts</template>
            </span>
          </div>
          <button class="btn btn-primary" @click="connectFirst">
            {{ smartMatches.length ? 'Connect' : 'Add & connect' }}
            <kbd class="btn-kbd" aria-hidden="true">↵</kbd>
          </button>
        </div>

        <!-- ===== Empty: no hosts at all ===== -->
        <div v-if="!hosts.hosts.length" class="hero">
          <span class="hero-icon"><Server :size="24" :stroke-width="1.5" aria-hidden="true" /></span>
          <p class="hero-title">Add your first host</p>
          <p class="hero-sub">
            Save a server once and connect with a double-click. Or just type
            <code class="mono">user@host</code> in the search box and press Enter.
          </p>
          <div class="hero-actions">
            <button class="btn btn-primary" @click="newHost">
              <Plus :size="14" :stroke-width="2" aria-hidden="true" />
              New host
            </button>
            <button class="btn btn-secondary" @click="dialogs.showImport = true">
              <Download :size="14" :stroke-width="1.5" aria-hidden="true" />
              Import from ~/.ssh or Termius
            </button>
          </div>
        </div>

        <!-- ===== Empty: group / search ===== -->
        <div v-else-if="!ordered.length && !searching" class="hero small">
          <p class="hero-title">No hosts in {{ currentGroup ? `“${currentGroup.name}”` : 'this view' }}</p>
          <button class="btn btn-secondary" @click="newHost">
            <Plus :size="14" :stroke-width="2" aria-hidden="true" />
            {{ currentGroup ? `New host in ${currentGroup.name}` : 'New host' }}
          </button>
        </div>
        <p v-else-if="!ordered.length && !smartTarget" class="no-match">
          No hosts match “{{ query.trim() }}”<template v-if="groupFilter !== 'all'">
            in this group · <button class="link" @click="pickGroup('all')">search all</button></template>
        </p>

        <template v-else>
          <section v-for="sec in (showActiveSection ? [{ key: 'active', title: 'Active', rows: activeRows }, { key: 'rest', title: sectionTitle, rows: restRows }] : [{ key: 'rest', title: sectionTitle, rows: restRows }])" :key="sec.key" class="sec">
            <h3 v-if="sec.rows.length" class="sec-title">
              {{ sec.title }}
              <span class="sec-n">{{ sec.rows.length }}</span>
            </h3>

            <div class="host-grid" :class="layout">
              <div
                v-for="(r, i) in sec.rows"
                :key="r.host.id"
                class="host"
                :class="[r.status, { selected: selectedId === r.host.id }]"
                role="button"
                :tabindex="tabIndexFor(r.host.id, sec.key === 'rest' && activeRows.length ? i + activeRows.length : i)"
                :data-host-id="r.host.id"
                :aria-label="`${r.host.label}, ${r.host.username}@${r.host.address}${r.status !== 'idle' ? ', ' + statusLabel[r.status] : ''}. Enter to connect`"
                @click="selectedId = r.host.id; focusKey = r.host.id"
                @dblclick="connect(r.host)"
                @keydown.self="onCardKey($event, r.host)"
                @contextmenu.prevent="hostMenu(r.host, $event)"
              >
                <span class="avatar">
                  <PlanetAvatar :host="r.host" :size="layout === 'grid' ? 40 : 30" />
                  <span v-if="r.status !== 'idle'" class="presence" :class="r.status" :title="statusLabel[r.status]"></span>
                </span>

                <span class="host-text">
                  <span class="host-name text-ellipsis">
                    <template v-for="(seg, k) in r.label" :key="k"><mark v-if="seg.hit">{{ seg.text }}</mark><template v-else>{{ seg.text }}</template></template>
                  </span>
                  <span class="host-sub mono text-ellipsis">{{ r.host.username }}@{{ r.host.address }}<template v-if="r.host.port !== 22">:{{ r.host.port }}</template></span>
                </span>

                <span v-if="layout === 'list'" class="host-col group-col text-ellipsis">
                  <template v-if="r.host.group_id">
                    <Folder :size="12" :stroke-width="1.75" aria-hidden="true" />
                    {{ groupName(r.host.group_id) }}
                  </template>
                </span>
                <span v-if="layout === 'list'" class="host-col used-col">
                  {{ r.status !== 'idle' ? statusLabel[r.status] : r.host.last_used_at ? relativeAge(new Date(r.host.last_used_at)) : 'Never' }}
                </span>

                <span class="host-trail">
                  <span v-if="layout === 'grid'" class="trail-meta">
                    <template v-if="r.status !== 'idle'">{{ statusLabel[r.status] }}</template>
                    <template v-else-if="groupFilter === 'all' && r.host.group_id">{{ groupName(r.host.group_id) }}</template>
                  </span>
                  <span class="trail-actions">
                    <button
                      class="act"
                      :aria-label="`Browse files on ${r.host.label}`"
                      title="Browse files (SFTP)"
                      tabindex="-1"
                      @click.stop="openSftp(r.host)"
                    >
                      <FolderTree :size="14" :stroke-width="1.6" aria-hidden="true" />
                    </button>
                    <button
                      class="act"
                      :aria-label="`More for ${r.host.label}`"
                      title="More"
                      tabindex="-1"
                      @click.stop="hostMenu(r.host, $event)"
                    >
                      <MoreHorizontal :size="15" :stroke-width="1.6" aria-hidden="true" />
                    </button>
                    <button
                      class="act go"
                      :aria-label="r.status === 'connected' ? `Open terminal for ${r.host.label}` : `Connect to ${r.host.label}`"
                      :title="r.status === 'connected' ? 'Open terminal' : 'Connect (Enter)'"
                      tabindex="-1"
                      @click.stop="connect(r.host)"
                    >
                      <Play :size="12" :stroke-width="2.25" aria-hidden="true" />
                    </button>
                  </span>
                </span>
              </div>
            </div>
          </section>

          <p v-if="ordered.length > 3 && !searching" class="hint">
            Double-click or press <kbd>Enter</kbd> to connect · right-click for more
          </p>
        </template>
      </div>
    </div>

    <!-- Details panel -->
    <Transition name="panel">
      <HostDetailsPanel v-if="selectedHost" :host="selectedHost" @close="selectedId = null">
        <template #actions>
          <button class="btn-icon" aria-label="Edit host" title="Edit host" @click="dialogs.openHostDialog(selectedHost!)">
            <Pencil :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </template>
        <template #danger>
          <button class="btn btn-danger panel-delete" @click="removeHost(selectedHost!)">
            <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
            Delete host
          </button>
        </template>
      </HostDetailsPanel>
    </Transition>

    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
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

/* ---- header ---- */

.hv-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-5) var(--space-5) var(--space-3);
}

.hv-title {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  font-size: var(--text-xl);
  font-weight: 650;
  letter-spacing: var(--tracking-tight);
  color: var(--text-1);
}

.hv-count {
  font-size: var(--text-md);
  font-weight: 500;
  letter-spacing: 0;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}

.hv-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.hv-more {
  width: 32px;
  height: 32px;
}

/* ---- search ---- */

.hv-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 0 var(--space-5) var(--space-3);
}

.find {
  position: relative;
  display: flex;
  align-items: center;
  flex: 1;
  height: 38px;
  padding: 0 10px 0 12px;
  gap: 9px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: 10px;
  color: var(--text-4);
  cursor: text;
  transition: border-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.find:hover {
  border-color: var(--border-strong);
}

.find:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
  color: var(--text-3);
}

.find-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: none;
  outline: none;
  font-size: var(--text-md);
  color: var(--text-1);
}

.find-input::placeholder {
  color: var(--text-4);
}

.find-kbd,
.btn-kbd,
.hint kbd {
  font-family: var(--font-mono);
  font-size: 10.5px;
  line-height: 1;
  padding: 3px 6px;
  border-radius: 5px;
  border: 1px solid var(--border);
  background: var(--bg-surface-2);
  color: var(--text-3);
}

.btn-kbd {
  margin-left: 2px;
  border-color: transparent;
  background: oklch(1 0 0 / 0.16);
  color: inherit;
}

.find-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  background: var(--bg-surface-3);
  color: var(--text-2);
  transition: background-color var(--dur-hover) ease, transform var(--dur-press) ease-out;
}

.find-clear:hover {
  background: var(--bg-active);
  color: var(--text-1);
}

.find-clear:active {
  transform: scale(0.9);
}

.seg {
  display: flex;
  padding: 2px;
  gap: 2px;
  border-radius: 9px;
  background: var(--bg-input);
  border: 1px solid var(--border);
}

.seg button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: 7px;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.seg button:hover {
  color: var(--text-1);
}

.seg button.on {
  background: var(--bg-surface-3);
  color: var(--text-1);
  box-shadow: var(--shadow-1);
}

/* ---- group chips ---- */

.chips {
  display: flex;
  gap: 6px;
  padding: 0 var(--space-5) var(--space-3);
  overflow-x: auto;
  scrollbar-width: none;
  flex-shrink: 0;
  mask-image: linear-gradient(to right, black calc(100% - 24px), transparent);
}

.chips::-webkit-scrollbar {
  display: none;
}

.chip-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 11px;
  flex-shrink: 0;
  border-radius: 999px;
  border: 1px solid var(--border);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-2);
  white-space: nowrap;
  transition:
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease,
    color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.chip-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.chip-btn:active {
  transform: scale(0.96);
}

.chip-btn.on {
  background: var(--accent-bg);
  border-color: color-mix(in oklch, var(--accent) 40%, transparent);
  color: var(--accent);
}

.chip-btn.add {
  border-style: dashed;
  color: var(--text-3);
}

.chip-n {
  font-size: var(--text-xs);
  font-variant-numeric: tabular-nums;
  opacity: 0.65;
}

/* ---- body ---- */

.hv-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-1) var(--space-5) var(--space-6);
  min-height: 0;
}

.smart {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 12px 12px 14px;
  margin-bottom: var(--space-5);
  border-radius: var(--radius-lg);
  background: var(--bg-surface);
  border: 1px solid color-mix(in oklch, var(--accent) 35%, var(--border));
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.smart-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border-radius: 10px;
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.smart-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.smart-title {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.smart-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.sec + .sec {
  margin-top: var(--space-5);
}

.sec-title {
  display: flex;
  align-items: baseline;
  gap: 6px;
  margin-bottom: var(--space-3);
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--text-1);
}

.sec-n {
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-4);
  font-variant-numeric: tabular-nums;
}

/* ---- host cards: grid ---- */

.host-grid.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: var(--space-3);
}

.host {
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
  padding: 12px 12px 12px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  cursor: default;
  outline: none;
  transition:
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease,
    box-shadow var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

@media (hover: hover) and (pointer: fine) {
  .host:hover {
    background: var(--bg-hover);
    border-color: var(--border-strong);
  }
}

.host:active:not(:has(.act:active)) {
  transform: scale(0.985);
}

.host:focus-visible {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.host.selected {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.avatar {
  position: relative;
  display: flex;
  flex-shrink: 0;
}

/* Presence dot with a ring cut from the card colour */
.presence {
  position: absolute;
  right: -3px;
  bottom: -3px;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  border: 2px solid var(--bg-surface);
  transition: border-color var(--dur-hover) ease;
}

.host:hover .presence {
  border-color: var(--bg-hover);
}

.presence.connected { background: var(--success); }
.presence.connecting { background: var(--warning); }
.presence.error { background: var(--danger); }

.host-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex: 1;
  min-width: 0;
  line-height: 1.35;
}

.host-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.host-name mark {
  background: none;
  color: var(--accent);
  border-radius: 2px;
}

.host-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

/* Trailing area: quiet metadata at rest, actions on hover/focus/selection */
.host-trail {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex-shrink: 0;
  min-width: 28px;
  height: 30px;
}

.trail-meta {
  max-width: 96px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--text-4);
  transition: opacity var(--dur-hover) ease;
}

.host.connected .trail-meta { color: var(--success); }
.host.connecting .trail-meta { color: var(--warning); }
.host.error .trail-meta { color: var(--danger); }

.trail-actions {
  position: absolute;
  right: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  opacity: 0;
  pointer-events: none;
  transform: translateX(4px);
  transition: opacity var(--dur-hover) ease, transform 180ms var(--ease-out);
}

.host:hover .trail-actions,
.host:focus-visible .trail-actions,
.host.selected .trail-actions {
  opacity: 1;
  pointer-events: auto;
  transform: none;
}

.host:hover .trail-meta,
.host:focus-visible .trail-meta,
.host.selected .trail-meta {
  opacity: 0;
}

@media (hover: none) {
  .trail-actions { opacity: 1; pointer-events: auto; transform: none; }
  .trail-meta { display: none; }
}

.act {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--radius-sm);
  color: var(--text-3);
  transition:
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.act:hover {
  background: var(--bg-active);
  color: var(--text-1);
}

.act:active {
  transform: scale(0.92);
}

.act.go {
  width: 30px;
  height: 30px;
  margin-left: 2px;
  border-radius: 999px;
  background: var(--accent);
  color: var(--accent-text);
  box-shadow: var(--shadow-1);
}

.act.go:hover {
  background: var(--accent-hover);
  color: var(--accent-text);
}

.act.go svg {
  margin-left: 1px; /* optical centre of the play triangle */
}

/* ---- host rows: list ---- */

.host-grid.list {
  display: flex;
  flex-direction: column;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  overflow: hidden;
}

.host-grid.list .host {
  display: grid;
  grid-template-columns: 30px minmax(0, 1.4fr) minmax(0, 0.8fr) 110px 104px;
  gap: 12px;
  min-height: 50px;
  padding: 9px 10px 9px 14px;
  background: transparent;
  border: none;
  border-radius: 0;
  box-shadow: none;
}

.host-grid.list .host + .host::before {
  content: '';
  position: absolute;
  top: 0;
  left: 56px;
  right: 0;
  border-top: 1px solid var(--border);
}

.host-grid.list .host:active {
  transform: none;
}

.host-grid.list .host.selected,
.host-grid.list .host:focus-visible {
  background: var(--accent-bg);
  box-shadow: none;
}

.host-grid.list .presence {
  border-color: var(--bg-surface);
}

.host-col {
  display: flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.used-col {
  font-variant-numeric: tabular-nums;
}

.host.connected .used-col { color: var(--success); }
.host.connecting .used-col { color: var(--warning); }

@container (max-width: 640px) {
  .host-grid.list .host {
    grid-template-columns: 30px minmax(0, 1fr) 104px;
  }
  .host-grid.list .host-col {
    display: none;
  }
}

.hosts-main {
  container-type: inline-size;
}

/* ---- empty / hints ---- */

.hero {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-7) var(--space-5);
  text-align: center;
}

.hero.small {
  padding-top: var(--space-6);
}

.hero-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 52px;
  height: 52px;
  margin-bottom: var(--space-2);
  border-radius: 15px;
  background: var(--accent-bg);
  color: var(--accent);
}

.hero-title {
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  color: var(--text-1);
}

.hero-sub {
  max-width: 46ch;
  font-size: var(--text-sm);
  color: var(--text-3);
}

.hero-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: var(--space-2);
  margin-top: var(--space-3);
}

.hero.small .btn {
  margin-top: var(--space-2);
}

.no-match {
  padding: var(--space-6) 0;
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
}

.link {
  color: var(--accent);
  font-size: inherit;
}

.link:hover {
  text-decoration: underline;
}

.hint {
  margin-top: var(--space-5);
  text-align: center;
  font-size: var(--text-xs);
  color: var(--text-4);
}

code {
  font-size: 0.95em;
  color: var(--text-2);
}

/* ---- details panel ---- */

.hosts-view > :deep(.details-panel) {
  border-left: 1px solid var(--border);
}

.panel-delete {
  margin-top: var(--space-3);
  width: 100%;
}

@media (prefers-reduced-motion: reduce) {
  .trail-actions {
    transform: none;
  }
}
</style>
