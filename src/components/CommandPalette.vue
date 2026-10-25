<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch, type Component } from 'vue'
import {
  ArrowLeftRight,
  Clock,
  Download,
  FolderTree,
  KeyRound,
  MonitorCog,
  Play,
  Plus,
  PlugZap,
  ScrollText,
  Search,
  Settings,
  ShieldCheck,
  SquareTerminal,
  StickyNote,
} from 'lucide-vue-next'
import HerdrLogo from './HerdrLogo.vue'
import PlanetAvatar from './PlanetAvatar.vue'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useSnippetsStore } from '../stores/extras'
import { useUiStore, type ViewId } from '../stores/ui'
import { useDialogsStore } from '../stores/dialogs'
import { useSmartConnect } from '../utils/smartConnect'
import { targetDisplay } from '../utils/parseTarget'
import { fuzzy, highlight } from '../utils/fuzzy'
import { comboFor, keyLabel, viewShortcutId } from '../services/shortcuts'
import type { Host } from '../types'

// Opened 100+ times a day (Ctrl+K, the + tab button): no open/close
// animation, input focused on the first frame, everything keyboard-first.

const hosts = useHostsStore()
const sessions = useSessionsStore()
const snippets = useSnippetsStore()
const ui = useUiStore()
const dialogs = useDialogsStore()

const query = ref('')
const activeIndex = ref(0)
const listEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)
let returnFocus: HTMLElement | null = null

const smart = useSmartConnect(query)

watch(
  () => dialogs.showPalette,
  (open) => {
    if (open) {
      returnFocus = document.activeElement as HTMLElement | null
      query.value = ''
      activeIndex.value = 0
      void nextTick(() => inputEl.value?.focus())
    } else {
      // Hand focus back (e.g. to the terminal) unless an action moved it
      const back = returnFocus
      returnFocus = null
      void nextTick(() => {
        if (back?.isConnected && document.activeElement === document.body) back.focus()
      })
    }
  },
)

// ---- items ----

type Section = 'Quick connect' | 'Open sessions' | 'Hosts' | 'Snippets' | 'Actions' | 'Go to'

interface Item {
  key: string
  section: Section
  label: string
  hits: number[]
  sub?: string
  score: number
  icon?: Component
  host?: Host
  status?: 'connected' | 'connecting' | 'error' | 'idle'
  herdr?: boolean
  keys?: string
  /** Footer hint for Enter */
  verb: string
  /** Shift+Enter alternative, if any */
  alt?: { verb: string; run: () => void }
  run: () => void
}

function tabStatus(hostId: number): Item['status'] {
  const t = sessions.tabs.find((x) => x.hostId === hostId)
  if (!t) return 'idle'
  if (t.status === 'connected') return 'connected'
  if (t.status === 'connecting' || t.status === 'hostkey' || t.status === 'reconnecting') return 'connecting'
  return t.status === 'error' ? 'error' : 'idle'
}

function openTab(id: number) {
  sessions.activeId = id
  ui.openView(`session:${id}`)
}

const runTarget = computed(() => {
  const live = sessions.tabs.filter((t) => t.status === 'connected')
  return live.find((t) => t.id === sessions.activeId) ?? live[live.length - 1] ?? null
})

interface ActionDef {
  key: string
  label: string
  sub?: string
  icon?: Component
  herdr?: boolean
  shortcut?: string
  always: boolean
  run: () => void
}

const actions = computed<ActionDef[]>(() => [
  { key: 'local', label: 'New local terminal', sub: 'PowerShell on this PC', icon: SquareTerminal, shortcut: 'newLocal', always: true, run: () => void sessions.connectLocal() },
  { key: 'sftp', label: 'Browse files (SFTP)', sub: 'Dual-pane file transfer', icon: FolderTree, shortcut: viewShortcutId('sftp'), always: true, run: () => ui.openView('sftp') },
  { key: 'newhost', label: 'New host…', sub: 'Save a server', icon: Plus, always: true, run: () => dialogs.openHostDialog(null) },
  { key: 'herdr', label: 'Open herdr', sub: 'Agent multiplexer', herdr: true, always: true, run: () => void sessions.connectHerdr() },
  { key: 'import', label: 'Import hosts…', sub: '~/.ssh/config or Termius', icon: Download, always: false, run: () => (dialogs.showImport = true) },
  { key: 'note', label: 'Quick note', icon: StickyNote, shortcut: 'quickNote', always: false, run: () => (dialogs.showQuickNote = true) },
  { key: 'settings', label: 'Settings', icon: Settings, shortcut: 'openSettings', always: false, run: () => (dialogs.showSettings = true) },
])

const views: { id: ViewId; label: string; icon: Component }[] = [
  { id: 'hosts', label: 'Hosts', icon: MonitorCog },
  { id: 'keychain', label: 'Keychain', icon: KeyRound },
  { id: 'forwards', label: 'Port Forwarding', icon: ArrowLeftRight },
  { id: 'snippets', label: 'Snippets', icon: ScrollText },
  { id: 'notes', label: 'Notes', icon: StickyNote },
  { id: 'known', label: 'Known Hosts', icon: ShieldCheck },
  { id: 'logs', label: 'Logs', icon: Clock },
]

function hostItem(h: Host, section: Section, score: number, hits: number[], sub?: string): Item {
  const status = tabStatus(h.id)
  const tab = sessions.tabs.find((t) => t.hostId === h.id && t.status !== 'closed' && t.status !== 'error')
  return {
    key: `${section}:${h.id}`,
    section,
    label: h.label,
    hits,
    sub: sub ?? `${h.username}@${h.address}${h.port !== 22 ? `:${h.port}` : ''}`,
    score,
    host: h,
    status,
    verb: tab ? 'Switch to' : 'Connect',
    run: () => (tab ? openTab(tab.id) : void sessions.connect(h.id)),
    alt: { verb: 'Browse files', run: () => ui.openSftpFor(h.id) },
  }
}

const items = computed<Item[]>(() => {
  const q = query.value.trim()
  const out: Item[] = []

  // 1. Quick connect target parsed from the query
  const t = smart.target.value
  const smartIds = new Set<number>()
  if (t) {
    for (const m of smart.matches.value) {
      smartIds.add(m.host.id)
      out.push(
        hostItem(
          m.host,
          'Quick connect',
          1e6,
          [],
          m.reason === 'ip' ? `${m.host.username}@${m.host.address} · via ${m.ip}` : undefined,
        ),
      )
    }
    if (!smartIds.size) {
      out.push({
        key: 'smart-new',
        section: 'Quick connect',
        label: `Connect to ${targetDisplay(t)}`,
        hits: [],
        sub: smart.resolving.value ? 'Resolving…' : smart.resolveFailed.value ? 'Address didn’t resolve — try anyway' : 'Not saved yet · adds it to Hosts',
        score: 1e6,
        icon: PlugZap,
        verb: 'Add & connect',
        run: () =>
          dialogs.openHostDialog(
            null,
            null,
            { address: t.host, port: t.port ?? 22, username: t.username, ips: t.kind === 'domain' ? smart.ips.value : [] },
            true,
          ),
      })
    }
  }

  // 2. Open sessions (switch to) — local terminals included
  const openTabs = sessions.tabs.filter((x) => x.status !== 'closed')
  for (const tab of openTabs) {
    const f = q ? fuzzy(q, tab.label) ?? fuzzy(q, tab.address) : { score: 0, hits: [] }
    if (!f) continue
    const host = tab.hostId !== null ? hosts.hostById(tab.hostId) : undefined
    if (host && smartIds.has(host.id)) continue
    out.push({
      key: `tab:${tab.id}`,
      section: 'Open sessions',
      label: tab.label,
      hits: q && fuzzy(q, tab.label) ? f.hits : [],
      sub: tab.address,
      score: f.score + 50,
      host,
      icon: host ? undefined : SquareTerminal,
      herdr: tab.kind === 'herdr',
      status: tab.status === 'connected' ? 'connected' : tab.status === 'error' ? 'error' : 'connecting',
      verb: 'Switch to',
      run: () => openTab(tab.id),
    })
  }
  const openHostIds = new Set(openTabs.map((x) => x.hostId))

  // 3. Saved hosts — fuzzy ranked, else most recent first
  const hostRows: Item[] = []
  for (const h of hosts.hosts) {
    if (smartIds.has(h.id) || openHostIds.has(h.id)) continue
    if (!q) {
      hostRows.push(hostItem(h, 'Hosts', h.last_used_at ?? 0, []))
      continue
    }
    const l = fuzzy(q, h.label)
    const a = fuzzy(q, `${h.username}@${h.address}`)
    if (!l && !a) continue
    const useLabel = !!l && l.score >= (a?.score ?? -Infinity) * 0.85
    hostRows.push(hostItem(h, 'Hosts', useLabel ? l!.score : a!.score * 0.85, useLabel ? l!.hits : []))
  }
  hostRows.sort((a, b) => b.score - a.score || a.label.localeCompare(b.label))
  out.push(...hostRows)

  // 4. Snippets — only while searching, run in the current terminal
  if (q && runTarget.value) {
    const target = runTarget.value
    const snips = snippets.snippets
      .map((s) => ({ s, f: fuzzy(q, s.name) ?? fuzzy(q, s.command) }))
      .filter((x) => x.f && x.f.score > 0)
      .sort((a, b) => b.f!.score - a.f!.score)
      .slice(0, 5)
    for (const { s, f } of snips) {
      out.push({
        key: `snip:${s.id}`,
        section: 'Snippets',
        label: s.name,
        hits: fuzzy(q, s.name)?.hits ?? [],
        sub: s.command.split('\n')[0],
        score: f!.score,
        icon: Play,
        verb: `Run in ${target.label}`,
        run: () => {
          sessions.write(target.id, s.command + '\n')
          openTab(target.id)
        },
        alt: { verb: 'Insert without running', run: () => (sessions.write(target.id, s.command), openTab(target.id)) },
      })
    }
  }

  // 5. Actions, then 6. navigation (navigation only when asked for)
  for (const a of actions.value) {
    const f = q ? fuzzy(q, a.label) : a.always ? { score: 0, hits: [] } : null
    if (!f) continue
    out.push({
      key: `act:${a.key}`,
      section: 'Actions',
      label: a.label,
      hits: f.hits,
      sub: a.sub,
      score: f.score,
      icon: a.icon,
      herdr: a.herdr,
      keys: a.shortcut ? comboFor(a.shortcut, ui.settings) : undefined,
      verb: 'Run',
      run: a.run,
    })
  }
  if (q) {
    for (const v of views) {
      const f = fuzzy(q, v.label) ?? fuzzy(q, `go ${v.label}`)
      if (!f) continue
      out.push({
        key: `view:${v.id}`,
        section: 'Go to',
        label: v.label,
        hits: fuzzy(q, v.label)?.hits ?? [],
        score: f.score,
        icon: v.icon,
        keys: comboFor(viewShortcutId(v.id), ui.settings),
        verb: 'Open',
        run: () => ui.openView(v.id),
      })
    }
  }

  // Sections stay in a fixed order; rank inside a section only
  const order: Section[] = ['Quick connect', 'Open sessions', 'Actions', 'Hosts', 'Snippets', 'Go to']
  const bySection = order.map((s) => out.filter((i) => i.section === s))
  // Hosts keep their pre-sorted order (recent first when not searching)
  if (q) bySection.forEach((list, i) => order[i] !== 'Hosts' && list.sort((a, b) => b.score - a.score))
  return bySection.flat()
})

watch(query, () => (activeIndex.value = 0))
watch(items, (list) => {
  if (activeIndex.value >= list.length) activeIndex.value = Math.max(0, list.length - 1)
})

const active = computed(() => items.value[activeIndex.value] ?? null)

function firstOfSection(i: number): boolean {
  return i === 0 || items.value[i - 1].section !== items.value[i].section
}

// ---- running ----

function close() {
  dialogs.showPalette = false
}

function run(item: Item | null, alt = false) {
  if (!item) return
  const fn = alt && item.alt ? item.alt.run : item.run
  returnFocus = null // the action decides where focus goes next
  close()
  fn()
}

function move(delta: number) {
  const n = items.value.length
  if (!n) return
  activeIndex.value = (activeIndex.value + delta + n) % n
  void nextTick(() => listEl.value?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' }))
}

function onKey(e: KeyboardEvent) {
  if (!dialogs.showPalette) return
  const ctrl = e.ctrlKey && !e.shiftKey && !e.altKey
  if (e.key === 'ArrowDown' || (ctrl && e.key.toLowerCase() === 'n')) {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp' || (ctrl && e.key.toLowerCase() === 'p')) {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'PageDown') {
    e.preventDefault()
    move(Math.min(8, items.value.length - 1 - activeIndex.value) || 0)
  } else if (e.key === 'PageUp') {
    e.preventDefault()
    move(-Math.min(8, activeIndex.value))
  } else if (e.key === 'Enter') {
    e.preventDefault()
    run(active.value, e.shiftKey)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    if (query.value) query.value = ''
    else close()
  }
}

function kbdParts(combo?: string): string[] {
  return combo ? combo.split('+').filter(Boolean).map(keyLabel) : []
}

onMounted(() => window.addEventListener('keydown', onKey, true))
onUnmounted(() => window.removeEventListener('keydown', onKey, true))
</script>

<template>
  <Teleport to="body">
    <div v-if="dialogs.showPalette" class="pal-scrim" @pointerdown.self="close">
      <div class="pal" role="dialog" aria-modal="true" aria-label="Quick connect and commands">
        <div class="pal-search">
          <Search :size="17" :stroke-width="1.6" class="pal-search-icon" aria-hidden="true" />
          <input
            ref="inputEl"
            v-model="query"
            class="pal-input"
            type="text"
            placeholder="Connect to a host, run a command, or type user@host"
            spellcheck="false"
            autocomplete="off"
            role="combobox"
            aria-expanded="true"
            aria-controls="pal-list"
            :aria-activedescendant="active ? `pal-${activeIndex}` : undefined"
            aria-label="Search hosts and commands"
          />
          <kbd class="pal-esc" aria-hidden="true">Esc</kbd>
        </div>

        <div id="pal-list" ref="listEl" class="pal-list" role="listbox" aria-label="Results">
          <div v-if="!items.length" class="pal-empty">
            <p class="pal-empty-title">No results for “{{ query.trim() }}”</p>
            <p class="pal-empty-sub">Type an address like <span class="mono">root@10.0.0.5</span> to connect directly.</p>
          </div>

          <template v-for="(item, i) in items" :key="item.key">
            <div v-if="firstOfSection(i)" class="pal-section" role="presentation">{{ item.section }}</div>
            <div
              :id="`pal-${i}`"
              class="pal-item"
              :class="{ active: i === activeIndex }"
              :data-active="i === activeIndex"
              role="option"
              :aria-selected="i === activeIndex"
              @mousemove="activeIndex = i"
              @click="run(item, $event.shiftKey)"
            >
              <span class="pal-lead">
                <template v-if="item.host">
                  <PlanetAvatar :host="item.host" :size="28" />
                  <span v-if="item.status && item.status !== 'idle'" class="pal-presence" :class="item.status" aria-hidden="true"></span>
                </template>
                <span v-else-if="item.herdr" class="pal-tile"><HerdrLogo :size="15" /></span>
                <span v-else class="pal-tile" :class="{ accent: item.section === 'Quick connect' }">
                  <component :is="item.icon" :size="15" :stroke-width="1.6" aria-hidden="true" />
                </span>
              </span>

              <span class="pal-text">
                <span class="pal-label text-ellipsis">
                  <template v-for="(seg, k) in highlight(item.label, item.hits)" :key="k"><mark v-if="seg.hit">{{ seg.text }}</mark><template v-else>{{ seg.text }}</template></template>
                </span>
                <span v-if="item.sub" class="pal-sub text-ellipsis" :class="{ mono: !!item.host || item.section === 'Snippets' || item.section === 'Open sessions' }">{{ item.sub }}</span>
              </span>

              <span class="pal-trail">
                <span v-if="item.status === 'connected' && item.section !== 'Open sessions'" class="pal-badge">Open</span>
                <span v-if="item.keys && i !== activeIndex" class="pal-keys" aria-hidden="true">
                  <kbd v-for="k in kbdParts(item.keys)" :key="k">{{ k }}</kbd>
                </span>
                <span v-if="i === activeIndex" class="pal-verb" aria-hidden="true">{{ item.verb }} <kbd>↵</kbd></span>
              </span>
            </div>
          </template>
        </div>

        <footer class="pal-foot" aria-hidden="true">
          <span><kbd>↑</kbd><kbd>↓</kbd> Navigate</span>
          <span v-if="active"><kbd>↵</kbd> {{ active.verb }}</span>
          <span v-if="active?.alt"><kbd>Shift</kbd><kbd>↵</kbd> {{ active.alt.verb }}</span>
          <span class="pal-foot-spacer"></span>
          <span><kbd>Esc</kbd> {{ query ? 'Clear' : 'Close' }}</span>
        </footer>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.pal-scrim {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 13vh;
  background: color-mix(in oklch, var(--scrim) 70%, transparent);
}

/* Material: the palette floats over whatever you were doing */
.pal {
  display: flex;
  flex-direction: column;
  width: 620px;
  max-width: calc(100vw - 2 * var(--space-5));
  max-height: min(560px, 70vh);
  overflow: hidden;
  border-radius: 14px;
  border: 1px solid var(--border-strong);
  background: color-mix(in oklch, var(--bg-surface) 88%, transparent);
  backdrop-filter: blur(24px) saturate(180%);
  -webkit-backdrop-filter: blur(24px) saturate(180%);
  box-shadow: 0 24px 64px oklch(0 0 0 / 0.4), 0 4px 16px oklch(0 0 0 / 0.22);
}

@media (prefers-reduced-transparency: reduce) {
  .pal {
    background: var(--bg-surface);
    backdrop-filter: none;
    -webkit-backdrop-filter: none;
  }
}

.pal-search {
  display: flex;
  align-items: center;
  gap: 11px;
  height: 56px;
  padding: 0 16px 0 18px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.pal-search-icon {
  color: var(--text-3);
  flex-shrink: 0;
}

.pal-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: none;
  outline: none;
  font-size: 1rem;
  letter-spacing: -0.005em;
  color: var(--text-1);
  caret-color: var(--accent);
}

.pal-input::placeholder {
  color: var(--text-4);
}

kbd {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 5px;
  border: 1px solid var(--border);
  background: var(--bg-surface-2);
  font-family: var(--font-ui);
  font-size: 10.5px;
  font-weight: 500;
  line-height: 1;
  color: var(--text-3);
}

.pal-esc {
  flex-shrink: 0;
}

.pal-list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 8px 8px;
}

.pal-section {
  padding: 10px 10px 4px;
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--text-4);
}

.pal-item {
  display: flex;
  align-items: center;
  gap: 11px;
  min-height: 44px;
  padding: 6px 10px;
  border-radius: 9px;
  cursor: default;
}

.pal-item.active {
  background: var(--accent-bg-hover);
}

.pal-lead {
  position: relative;
  display: flex;
  flex-shrink: 0;
}

.pal-lead :deep(.planet-avatar) {
  border-radius: 7px;
}

.pal-tile {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: 7px;
  background: var(--bg-surface-3);
  color: var(--text-2);
}

.pal-tile.accent {
  background: var(--accent);
  color: var(--accent-text);
}

.pal-presence {
  position: absolute;
  right: -2px;
  bottom: -2px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  border: 2px solid var(--bg-surface);
}

.pal-presence.connected { background: var(--success); }
.pal-presence.connecting { background: var(--warning); }
.pal-presence.error { background: var(--danger); }

.pal-text {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex: 1;
  min-width: 0;
  line-height: 1.3;
}

.pal-label {
  font-size: var(--text-md);
  font-weight: 500;
  color: var(--text-1);
}

.pal-label mark {
  background: none;
  color: var(--accent);
  font-weight: 650;
}

.pal-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.pal-trail {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.pal-badge {
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--success-bg);
  color: var(--success);
  font-size: var(--text-xs);
  font-weight: 500;
}

.pal-keys {
  display: flex;
  gap: 3px;
}

.pal-verb {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: var(--text-xs);
  color: var(--text-2);
  white-space: nowrap;
}

.pal-empty {
  padding: var(--space-6) var(--space-4);
  text-align: center;
}

.pal-empty-title {
  font-size: var(--text-md);
  font-weight: 600;
  color: var(--text-2);
}

.pal-empty-sub {
  margin-top: 4px;
  font-size: var(--text-sm);
  color: var(--text-4);
}

.pal-foot {
  display: flex;
  align-items: center;
  gap: 14px;
  height: 36px;
  padding: 0 14px;
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-4);
  flex-shrink: 0;
}

.pal-foot span {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}

.pal-foot-spacer {
  flex: 1;
}
</style>
