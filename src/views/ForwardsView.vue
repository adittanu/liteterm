<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { ArrowLeftRight, ArrowRight, Copy, MoreHorizontal, Pencil, Plus, Trash2 } from 'lucide-vue-next'
import { useForwardsStore } from '../stores/extras'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import ForwardDialog from '../components/ForwardDialog.vue'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import SearchField from '../components/SearchField.vue'
import { copyText, matchesQuery } from '../utils/pageHelpers'
import { waitForConnection } from '../utils/sessionWait'
import type { PortForward } from '../types'

const forwards = useForwardsStore()
const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const query = ref('')
const editorOpen = ref(false)
const editing = ref<PortForward | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)
// Forwards mid start/stop — the switch shows busy and ignores repeat clicks
const pending = ref(new Map<number, 'connecting' | 'starting' | 'stopping'>())

const activeCount = computed(() => forwards.forwards.filter((f) => isActive(f.id)).length)

const visible = computed(() =>
  [...forwards.forwards]
    .sort((a, b) => a.name.localeCompare(b.name))
    .filter((f) =>
      matchesQuery(query.value, f.name, hostLabel(f.host_id), f.remote_host, String(f.local_port), String(f.remote_port)),
    ),
)

function isActive(id: number): boolean {
  return forwards.activeIds.includes(id)
}

function hostLabel(id: number): string {
  return hosts.hostById(id)?.label ?? `host #${id}`
}

function hostStatus(hostId: number): 'connected' | 'connecting' | 'idle' {
  const tab = sessions.tabs.find((t) => t.hostId === hostId)
  if (!tab) return 'idle'
  if (tab.status === 'connected') return 'connected'
  if (tab.status === 'connecting' || tab.status === 'hostkey' || tab.status === 'reconnecting') return 'connecting'
  return 'idle'
}

function statusText(f: PortForward): string {
  const p = pending.value.get(f.id)
  if (p === 'connecting') return `Connecting to ${hostLabel(f.host_id)}…`
  if (p === 'starting') return 'Starting…'
  if (p === 'stopping') return 'Stopping…'
  if (isActive(f.id)) return 'Listening'
  return hostStatus(f.host_id) === 'connected' ? 'Stopped' : 'Host offline'
}

function setPending(id: number, state: 'connecting' | 'starting' | 'stopping' | null) {
  const next = new Map(pending.value)
  if (state) next.set(id, state)
  else next.delete(id)
  pending.value = next
}

async function toggle(f: PortForward) {
  if (pending.value.has(f.id)) return
  const turnOn = !isActive(f.id)
  try {
    if (!turnOn) {
      setPending(f.id, 'stopping')
      await forwards.stop(f.id)
      return
    }
    if (hostStatus(f.host_id) !== 'connected') {
      setPending(f.id, 'connecting')
      await sessions.connect(f.host_id, undefined, { silent: true })
      if ((await waitForConnection(f.host_id)) === null) {
        ui.toast('error', `Couldn’t connect to ${hostLabel(f.host_id)} — the tunnel was not started`)
        return
      }
    }
    setPending(f.id, 'starting')
    await forwards.start(f.id)
    ui.toast('success', `Listening on localhost:${f.local_port}`, {
      actionLabel: 'Copy',
      action: () => void copyText(`localhost:${f.local_port}`, 'Address copied'),
    })
  } catch (e) {
    ui.toast('error', errorText(e))
  } finally {
    setPending(f.id, null)
  }
}

function openNew() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(f: PortForward) {
  editing.value = f
  editorOpen.value = true
}

function openMenu(f: PortForward, e: MouseEvent) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: 'Edit', icon: Pencil, action: () => openEdit(f) },
      { label: 'Copy local address', icon: Copy, action: () => void copyText(`localhost:${f.local_port}`, 'Address copied') },
      { label: 'Delete', icon: Trash2, danger: true, action: () => void remove(f) },
    ],
  }
}

// Forgiveness over confirmation: delete immediately, offer Undo
async function remove(f: PortForward) {
  try {
    await forwards.remove(f.id)
  } catch (e) {
    ui.toast('error', `Could not delete: ${errorText(e)}`)
    return
  }
  const restore = {
    host_id: f.host_id,
    name: f.name,
    local_port: f.local_port,
    remote_host: f.remote_host,
    remote_port: f.remote_port,
    auto_start: f.auto_start,
  }
  ui.toast('info', `Deleted “${f.name}”`, {
    actionLabel: 'Undo',
    duration: 6000,
    action: () => void forwards.save(restore).catch((e) => ui.toast('error', errorText(e))),
  })
}

// Keep switches honest: auto-start tunnels come up on connect and die with
// their session, both outside this page's control
const sessionKey = computed(() => sessions.tabs.map((t) => `${t.hostId}:${t.status}`).join('|'))
let refreshTimer: ReturnType<typeof setTimeout> | undefined
watch(sessionKey, () => {
  clearTimeout(refreshTimer)
  refreshTimer = setTimeout(() => void forwards.refreshActive().catch(() => {}), 400)
})

onMounted(() => {
  if (!hosts.loaded) void hosts.load()
  if (!forwards.loaded) void forwards.load()
  else void forwards.refreshActive().catch(() => {})
})
onBeforeUnmount(() => clearTimeout(refreshTimer))
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-heading">
        <h2 class="page-title">
          Port Forwarding
          <span v-if="forwards.forwards.length" class="page-count">{{ forwards.forwards.length }}</span>
        </h2>
        <p class="page-sub">Reach a service behind an SSH host as if it ran on this PC.</p>
      </div>
      <div v-if="hosts.hosts.length" class="page-actions">
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New forward
        </button>
      </div>
    </header>

    <div v-if="forwards.forwards.length" class="page-toolbar">
      <SearchField v-model="query" label="Search forwards" placeholder="Search by name, host or port" />
      <span class="toolbar-spacer"></span>
      <span class="toolbar-label">
        <span class="status-dot" :class="activeCount ? 'connected' : 'idle'" aria-hidden="true"></span>
        {{ activeCount ? `${activeCount} active` : 'None active' }}
      </span>
    </div>

    <div class="page-scroll">
      <div v-if="!hosts.hosts.length" class="empty-hero">
        <span class="empty-icon"><ArrowLeftRight :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
        <p class="empty-title">Add a host first</p>
        <p class="empty-sub">Forwards tunnel through an SSH connection, so they need a saved host to go through.</p>
        <button class="btn btn-primary" @click="dialogs.openHostDialog(null)">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New host
        </button>
      </div>

      <div v-else-if="!forwards.forwards.length" class="empty-hero">
        <span class="empty-icon"><ArrowLeftRight :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
        <p class="empty-title">No forwards yet</p>
        <p class="empty-sub">
          For example, open a database that only listens on the server at
          <code class="mono">localhost:5432</code> on this PC.
        </p>
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New forward
        </button>
      </div>

      <p v-else-if="!visible.length" class="no-results">No forwards match “{{ query.trim() }}”</p>

      <ul v-else class="inset-list">
        <li
          v-for="f in visible"
          :key="f.id"
          class="list-row fwd-row"
          :class="{ on: isActive(f.id) }"
          @contextmenu.prevent="openMenu(f, $event)"
        >
          <span class="row-tile fwd-tile" aria-hidden="true">
            <ArrowLeftRight :size="15" :stroke-width="1.75" />
          </span>

          <button class="row-main fwd-main" :aria-label="`Edit ${f.name}`" @click="openEdit(f)">
            <span class="row-title">
              <span class="text-ellipsis">{{ f.name }}</span>
              <span v-if="f.auto_start" class="chip" title="Starts whenever the host connects">Auto</span>
            </span>
            <span class="fwd-route mono">
              <span class="fwd-local">localhost:{{ f.local_port }}</span>
              <ArrowRight :size="11" :stroke-width="1.75" class="fwd-arrow" aria-hidden="true" />
              <span class="text-ellipsis">{{ f.remote_host }}:{{ f.remote_port }}</span>
            </span>
            <span class="row-sub fwd-via">
              <span class="status-dot" :class="hostStatus(f.host_id)" aria-hidden="true"></span>
              <span class="text-ellipsis">via {{ hostLabel(f.host_id) }}</span>
              <span class="fwd-state" :class="{ live: isActive(f.id) && !pending.has(f.id) }">· {{ statusText(f) }}</span>
            </span>
          </button>

          <span class="row-actions">
            <button
              class="btn-icon"
              :aria-label="`Copy localhost:${f.local_port}`"
              title="Copy local address"
              @click="copyText(`localhost:${f.local_port}`, 'Address copied')"
            >
              <Copy :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Options for ${f.name}`" @click.stop="openMenu(f, $event)">
              <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </span>

          <button
            class="switch"
            role="switch"
            :aria-checked="isActive(f.id) || pending.get(f.id) === 'connecting' || pending.get(f.id) === 'starting'"
            :aria-busy="pending.has(f.id)"
            :aria-label="`${isActive(f.id) ? 'Stop' : 'Start'} ${f.name}`"
            :title="isActive(f.id) ? 'Stop tunnel' : hostStatus(f.host_id) === 'connected' ? 'Start tunnel' : 'Connect and start'"
            @click="toggle(f)"
          ></button>
        </li>
      </ul>
    </div>

    <ForwardDialog v-if="editorOpen" :forward="editing" @close="editorOpen = false" />
    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.toolbar-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.fwd-row {
  --row-inset: 58px;
  padding-block: 11px;
}

.fwd-tile {
  transition: background-color 200ms ease, color 200ms ease;
}

.fwd-row.on .fwd-tile {
  background: var(--success-bg);
  color: var(--success);
}

.fwd-main {
  gap: 3px;
  text-align: left;
  border-radius: var(--radius-sm);
}

.fwd-route {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  font-size: var(--text-sm);
  color: var(--text-2);
}

.fwd-local {
  color: var(--text-1);
  flex-shrink: 0;
}

.fwd-arrow {
  color: var(--text-4);
  flex-shrink: 0;
}

.fwd-via {
  display: flex;
  align-items: center;
  gap: 6px;
}

.fwd-via .status-dot {
  width: 6px;
  height: 6px;
}

.fwd-state {
  flex-shrink: 0;
  color: var(--text-4);
}

.fwd-state.live {
  color: var(--success);
}

.fwd-row .switch {
  margin-left: var(--space-1);
}

code {
  font-size: 0.95em;
  color: var(--text-2);
}
</style>
