<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { Search, Server, SquareTerminal, FolderTree, CornerDownLeft, PlugZap } from 'lucide-vue-next'
import HerdrLogo from './HerdrLogo.vue'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { useDialogsStore } from '../stores/dialogs'
import { useSmartConnect } from '../utils/smartConnect'
import { targetDisplay, type SmartTarget } from '../utils/parseTarget'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const ui = useUiStore()
const dialogs = useDialogsStore()

const query = ref('')
const activeIndex = ref(0)
const listEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)

// Quick connect: parse the query as user@host:port / domain / IP and match
// it against saved hosts (by address or resolved IP)
const smart = useSmartConnect(query)

// Focus the input every time the palette opens (autofocus doesn't fire
// on dynamically mounted elements)
watch(
  () => dialogs.showPalette,
  (open) => {
    if (open) {
      query.value = ''
      activeIndex.value = 0
      void nextTick(() => inputEl.value?.focus())
    }
  },
)

interface Item {
  key: string
  label: string
  sub: string
  icon: 'local' | 'sftp' | 'host' | 'plug' | 'herdr'
  hostId?: number
  newTarget?: SmartTarget
}

// Rows injected by quick connect (matches first, else a single add-new row);
// the first regular row after them gets a hairline separator
const smartCount = computed(() => {
  if (!smart.target.value) return 0
  return smart.matches.value.length || 1
})

const items = computed<Item[]>(() => {
  const q = query.value.trim().toLowerCase()
  const t = smart.target.value

  const smartItems: Item[] = []
  const matchedIds = new Set<number>()
  if (t) {
    for (const m of smart.matches.value) {
      matchedIds.add(m.host.id)
      smartItems.push({
        key: `smart:${m.host.id}`,
        label: m.host.label,
        sub:
          m.reason === 'ip'
            ? `${m.host.username}@${m.host.address} · via ${m.ip}`
            : `${m.host.username}@${m.host.address}`,
        icon: 'host',
        hostId: m.host.id,
      })
    }
    if (!matchedIds.size) {
      smartItems.push({
        key: 'smart-new',
        label: `Connect to ${targetDisplay(t)}`,
        sub: smart.resolving.value ? 'Resolving…' : 'Not saved — add & connect',
        icon: 'plug',
        newTarget: t,
      })
    }
  }

  const list: Item[] = [
    { key: 'local', label: 'Open local terminal', sub: 'this PC', icon: 'local' },
    { key: 'herdr', label: 'Open herdr', sub: 'agent multiplexer', icon: 'herdr' },
    { key: 'sftp', label: 'Open SFTP file browser', sub: 'dual-pane transfer', icon: 'sftp' },
  ]
  const hostItems: Item[] = hosts.hosts
    .filter((h) => !matchedIds.has(h.id))
    .filter(
      (h) =>
        !q ||
        h.label.toLowerCase().includes(q) ||
        h.address.toLowerCase().includes(q) ||
        h.username.toLowerCase().includes(q),
    )
    .map((h) => ({
      key: `host:${h.id}`,
      label: h.label,
      sub: `${h.username}@${h.address}`,
      icon: 'host' as const,
      hostId: h.id,
    }))
  if (!q) return [...smartItems, ...list, ...hostItems]
  const actionItems = list.filter((i) => i.label.toLowerCase().includes(q) || i.sub.includes(q))
  return [...smartItems, ...actionItems, ...hostItems]
})

watch([query, items], () => {
  activeIndex.value = 0
})

function move(delta: number) {
  if (!items.value.length) return
  activeIndex.value = (activeIndex.value + delta + items.value.length) % items.value.length
  void nextTick(() => {
    listEl.value?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' })
  })
}

function choose() {
  const item = items.value[activeIndex.value]
  if (!item) return
  close()
  if (item.newTarget) {
    const t = item.newTarget
    dialogs.openHostDialog(
      null,
      null,
      {
        address: t.host,
        port: t.port ?? 22,
        username: t.username,
        ips: t.kind === 'domain' ? smart.ips.value : [],
      },
      true,
    )
  } else if (item.icon === 'local') void sessions.connectLocal()
  else if (item.icon === 'herdr') void sessions.connectHerdr()
  else if (item.icon === 'sftp') ui.openView('sftp')
  else if (item.hostId !== undefined) void sessions.connect(item.hostId)
}

function close() {
  dialogs.showPalette = false
}

function onKey(e: KeyboardEvent) {
  if (!dialogs.showPalette) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    choose()
  } else if (e.key === 'Escape') {
    close()
  }
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div v-if="dialogs.showPalette" class="palette-scrim" @click.self="close">
      <div class="palette surface instant" role="dialog" aria-modal="true" aria-label="Quick connect">
        <div class="palette-search">
          <Search :size="15" :stroke-width="1.5" aria-hidden="true" />
          <input
            ref="inputEl"
            v-model="query"
            class="palette-input"
            type="text"
            placeholder="Search hosts, or type user@hostname to connect…"
            spellcheck="false"
            aria-label="Search hosts"
          />
        </div>

        <div ref="listEl" class="palette-list" role="listbox" aria-label="Hosts and actions">
          <div v-if="!items.length" class="palette-empty">No matches</div>
          <template v-for="(item, i) in items" v-else :key="item.key">
            <div v-if="smartCount > 0 && i === smartCount && items.length > smartCount" class="palette-sep" role="presentation"></div>
            <button
              class="palette-item"
              :class="{ active: i === activeIndex }"
              :data-active="i === activeIndex"
              role="option"
              :aria-selected="i === activeIndex"
              @mousemove="activeIndex = i"
              @click="choose"
            >
              <PlugZap v-if="item.icon === 'plug'" :size="14" :stroke-width="1.5" class="palette-plug" aria-hidden="true" />
              <SquareTerminal v-else-if="item.icon === 'local'" :size="14" :stroke-width="1.5" aria-hidden="true" />
              <HerdrLogo v-else-if="item.icon === 'herdr'" :size="14" />
              <FolderTree v-else-if="item.icon === 'sftp'" :size="14" :stroke-width="1.5" aria-hidden="true" />
              <Server v-else :size="14" :stroke-width="1.5" aria-hidden="true" />
              <span class="palette-label">{{ item.label }}</span>
              <span class="palette-addr mono">{{ item.sub }}</span>
              <CornerDownLeft v-if="i === activeIndex" :size="12" :stroke-width="1.5" class="palette-enter" aria-hidden="true" />
            </button>
          </template>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.palette-scrim {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 12vh;
  background: var(--scrim);
}

.palette {
  width: 480px;
  max-width: calc(100vw - 2 * var(--space-5));
  box-shadow: var(--shadow-3);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  max-height: 60vh;
}

.palette-search {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
  color: var(--text-3);
}

.palette-input {
  flex: 1;
  border: none;
  background: none;
  font-size: var(--text-md);
  color: var(--text-1);
}

.palette-input:focus {
  outline: none;
}

.palette-input::placeholder {
  color: var(--text-4);
}

.palette-list {
  overflow-y: auto;
  padding: 6px;
}

.palette-empty {
  padding: var(--space-5);
  text-align: center;
  color: var(--text-4);
  font-size: var(--text-sm);
}

.palette-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  color: var(--text-2);
  text-align: left;
}

.palette-item.active {
  background: var(--accent-bg);
  color: var(--text-1);
}

.palette-label {
  font-weight: 500;
  color: var(--text-1);
}

.palette-addr {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.palette-sep {
  border-top: 1px solid var(--border);
  margin: 6px 4px;
}

.palette-plug {
  color: var(--accent);
  flex-shrink: 0;
}

.palette-enter {
  color: var(--text-4);
  flex-shrink: 0;
}
</style>
