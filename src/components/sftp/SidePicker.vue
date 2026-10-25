<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { Laptop, Search } from 'lucide-vue-next'
import PlanetAvatar from '../PlanetAvatar.vue'
import { useHostsStore } from '../../stores/hosts'
import { useSessionsStore } from '../../stores/sessions'
import { matchesQuery } from '../../utils/pageHelpers'

// Chooses what one side of the SFTP view shows: this PC or a saved host.
// Connected hosts lead the list — they open instantly.
const props = withDefaults(
  defineProps<{ currentHostId?: number | null; currentLocal?: boolean; showLocal?: boolean; autofocus?: boolean }>(),
  { currentHostId: null, currentLocal: false, showLocal: true, autofocus: true },
)
const emit = defineEmits<{ local: []; host: [id: number]; close: [] }>()

const hosts = useHostsStore()
const sessions = useSessionsStore()

const query = ref('')
const active = ref(0)
const inputEl = ref<HTMLInputElement | null>(null)
const listEl = ref<HTMLElement | null>(null)

const connected = computed(
  () => new Set(sessions.tabs.filter((t) => t.kind === 'ssh' && t.status === 'connected').map((t) => t.hostId)),
)

type Item = { kind: 'local' } | { kind: 'host'; id: number }

const hostList = computed(() =>
  [...hosts.hosts]
    .filter((h) => matchesQuery(query.value, h.label, h.address, h.username))
    .sort((a, b) => {
      const ca = connected.value.has(a.id) ? 0 : 1
      const cb = connected.value.has(b.id) ? 0 : 1
      return ca - cb || (b.last_used_at ?? 0) - (a.last_used_at ?? 0) || a.label.localeCompare(b.label)
    }),
)

const showLocalRow = computed(
  () => props.showLocal && matchesQuery(query.value, 'local this pc computer windows'),
)

const items = computed<Item[]>(() => [
  ...(showLocalRow.value ? [{ kind: 'local' as const }] : []),
  ...hostList.value.map((h) => ({ kind: 'host' as const, id: h.id })),
])

watch(query, () => (active.value = 0))

function choose(item: Item | undefined) {
  if (!item) return
  if (item.kind === 'local') emit('local')
  else emit('host', item.id)
}

function onKey(e: KeyboardEvent) {
  const n = items.value.length
  if (e.key === 'ArrowDown' && n) {
    e.preventDefault()
    active.value = (active.value + 1) % n
  } else if (e.key === 'ArrowUp' && n) {
    e.preventDefault()
    active.value = (active.value - 1 + n) % n
  } else if (e.key === 'Enter') {
    e.preventDefault()
    choose(items.value[active.value])
    return
  } else if (e.key === 'Escape') {
    e.stopPropagation()
    if (query.value) query.value = ''
    else emit('close')
    return
  } else {
    return
  }
  void nextTick(() => listEl.value?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' }))
}

onMounted(() => {
  if (props.autofocus) {
    // After the opening click settles — WebView2 can hand focus back to the trigger
    void nextTick(() => {
      inputEl.value?.focus({ preventScroll: true })
      window.setTimeout(() => inputEl.value?.focus({ preventScroll: true }), 0)
    })
  }
})
</script>

<template>
  <div class="side-picker" @keydown="onKey">
    <div class="sp-search">
      <Search :size="14" :stroke-width="1.5" aria-hidden="true" />
      <input
        ref="inputEl"
        v-model="query"
        type="text"
        placeholder="Search hosts"
        spellcheck="false"
        autocomplete="off"
        aria-label="Search hosts"
        role="combobox"
        aria-expanded="true"
      />
    </div>

    <div ref="listEl" class="sp-list" role="listbox" aria-label="Choose a location">
      <button
        v-if="showLocalRow"
        type="button"
        class="sp-row"
        role="option"
        :aria-selected="currentLocal"
        :data-active="active === 0"
        :class="{ active: active === 0, current: currentLocal }"
        @mouseenter="active = 0"
        @click="emit('local')"
      >
        <span class="sp-tile"><Laptop :size="15" :stroke-width="1.5" aria-hidden="true" /></span>
        <span class="sp-text">
          <span class="sp-name">This PC</span>
          <span class="sp-sub">Local files</span>
        </span>
      </button>

      <p v-if="hostList.length" class="sp-label">{{ connected.size ? 'Servers' : 'Saved hosts' }}</p>

      <button
        v-for="(h, i) in hostList"
        :key="h.id"
        type="button"
        class="sp-row"
        role="option"
        :aria-selected="currentHostId === h.id"
        :data-active="active === i + (showLocalRow ? 1 : 0)"
        :class="{ active: active === i + (showLocalRow ? 1 : 0), current: currentHostId === h.id }"
        @mouseenter="active = i + (showLocalRow ? 1 : 0)"
        @click="emit('host', h.id)"
      >
        <PlanetAvatar :host="h" :size="30" />
        <span class="sp-text">
          <span class="sp-name text-ellipsis">{{ h.label }}</span>
          <span class="sp-sub mono text-ellipsis">{{ h.username }}@{{ h.address }}</span>
        </span>
        <span v-if="connected.has(h.id)" class="sp-live" title="Connected — opens instantly">
          <span class="status-dot connected" aria-hidden="true"></span>
          Connected
        </span>
      </button>

      <p v-if="!items.length" class="sp-empty">
        {{ hosts.hosts.length ? `No hosts match “${query.trim()}”` : 'No saved hosts yet — add one in Hosts.' }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.side-picker {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  min-height: 0;
}

.sp-search {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 11px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  color: var(--text-4);
  transition: border-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.sp-search:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
  color: var(--text-3);
}

.sp-search input {
  flex: 1;
  min-width: 0;
  border: none;
  background: none;
  outline: none;
  font-size: var(--text-md);
  color: var(--text-1);
}

.sp-search input::placeholder {
  color: var(--text-4);
}

.sp-list {
  display: flex;
  flex-direction: column;
  gap: 1px;
  overflow-y: auto;
  min-height: 0;
}

.sp-label {
  margin: 6px 8px 2px;
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: var(--tracking-wide);
  text-transform: uppercase;
  color: var(--text-4);
}

.sp-row {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 7px 8px;
  border-radius: var(--radius-md);
  text-align: left;
  transition: background-color 80ms ease, transform var(--dur-press) ease-out;
}

.sp-row.active {
  background: var(--bg-hover);
}

.sp-row.current {
  background: var(--accent-bg);
}

.sp-row:active {
  transform: scale(0.985);
}

.sp-tile {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 30px;
  height: 30px;
  border-radius: var(--radius-md);
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
}

.sp-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.3;
}

.sp-name {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.sp-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.sp-live {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
  font-size: var(--text-xs);
  color: var(--success);
}

.sp-live .status-dot {
  width: 6px;
  height: 6px;
}

.sp-empty {
  padding: var(--space-5) var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
}
</style>
