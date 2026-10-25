<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Clock, Plug, Server, Trash2 } from 'lucide-vue-next'
import { api } from '../services/api'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import PlanetAvatar from '../components/PlanetAvatar.vue'
import SearchField from '../components/SearchField.vue'
import { dayLabel, matchesQuery, parseSqlDate, plural, useDelayedFlag } from '../utils/pageHelpers'
import type { SessionLog } from '../types'

const LIMIT = 200

const dialogs = useDialogsStore()
const hosts = useHostsStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const logs = ref<SessionLog[]>([])
const loaded = ref(false)
const loading = ref(false)
const loadError = ref<string | null>(null)
const query = ref('')
const showSpinner = useDelayedFlag(computed(() => loading.value && !loaded.value))

async function load() {
  loading.value = true
  loadError.value = null
  try {
    logs.value = await api.listSessionLogs(LIMIT)
    loaded.value = true
  } catch (e) {
    loadError.value = errorText(e)
  } finally {
    loading.value = false
  }
}

async function clearAll() {
  const ok = await dialogs.confirm({
    title: 'Clear all session history?',
    message: `${plural(logs.value.length, 'entry', 'entries')} will be deleted from this machine. Saved hosts are not affected.`,
    confirmLabel: 'Clear history',
    danger: true,
  })
  if (!ok) return
  try {
    await api.clearSessionLogs()
    await load()
    ui.toast('info', 'Session history cleared')
  } catch (e) {
    ui.toast('error', errorText(e))
  }
}

// ---- derived rows ----

interface Row {
  log: SessionLog
  start: Date | null
  secs: number | null // null = still open or unknown
  live: boolean
}

function isLive(log: SessionLog): boolean {
  return (
    !log.ended_at &&
    log.host_id !== null &&
    sessions.tabs.some((t) => t.hostId === log.host_id && t.status === 'connected')
  )
}

const rows = computed<Row[]>(() =>
  logs.value
    .filter((l) => matchesQuery(query.value, l.label, l.address, l.username))
    .map((log) => {
      const start = parseSqlDate(log.started_at)
      const end = log.ended_at ? parseSqlDate(log.ended_at) : null
      const secs = start && end ? Math.max(0, Math.round((end.getTime() - start.getTime()) / 1000)) : null
      return { log, start, secs, live: isLive(log) }
    }),
)

// Day sections, newest first (the backend already returns newest first)
const days = computed(() => {
  const out: { key: string; label: string; rows: Row[]; secs: number }[] = []
  for (const r of rows.value) {
    const key = r.start ? r.start.toDateString() : 'unknown'
    let day = out[out.length - 1]
    if (!day || day.key !== key) {
      day = { key, label: r.start ? dayLabel(r.start) : 'Unknown date', rows: [], secs: 0 }
      out.push(day)
    }
    day.rows.push(r)
    day.secs += r.secs ?? 0
  }
  return out
})

function formatDuration(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

function formatTime(d: Date | null): string {
  return d ? d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '—'
}

function hostFor(log: SessionLog) {
  return hosts.hostById(log.host_id)
}

function reconnect(log: SessionLog) {
  if (log.host_id !== null) void sessions.connect(log.host_id)
}

// New sessions write a log row on connect and close it on disconnect
const sessionKey = computed(() => sessions.tabs.map((t) => `${t.id}:${t.status}`).join('|'))
let reloadTimer: ReturnType<typeof setTimeout> | undefined
watch(sessionKey, () => {
  clearTimeout(reloadTimer)
  reloadTimer = setTimeout(() => void load(), 500)
})

onMounted(load)
onBeforeUnmount(() => clearTimeout(reloadTimer))
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-heading">
        <h2 class="page-title">
          Logs
          <span v-if="logs.length" class="page-count">{{ logs.length }}</span>
        </h2>
        <p class="page-sub">Your SSH session history. Stored only on this machine.</p>
      </div>
      <div v-if="logs.length" class="page-actions">
        <button class="btn btn-ghost clear-btn" @click="clearAll">
          <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
          Clear history
        </button>
      </div>
    </header>

    <div v-if="logs.length" class="page-toolbar">
      <SearchField v-model="query" label="Search session history" placeholder="Search host, address or user" />
    </div>

    <div class="page-scroll">
      <div v-if="showSpinner" class="empty-hero">
        <span class="spinner" aria-label="Loading"></span>
      </div>

      <div v-else-if="loadError && !logs.length" class="empty-hero">
        <p class="empty-title">Couldn’t read session history</p>
        <p class="mono error-text">{{ loadError }}</p>
        <button class="btn btn-secondary" @click="load">Try again</button>
      </div>

      <template v-else-if="loaded">
        <div v-if="!logs.length" class="empty-hero">
          <span class="empty-icon"><Clock :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
          <p class="empty-title">No sessions yet</p>
          <p class="empty-sub">Every SSH connection you open shows up here with when it started and how long it lasted.</p>
          <button class="btn btn-secondary" @click="ui.openView('hosts')">Go to Hosts</button>
        </div>

        <p v-else-if="!rows.length" class="no-results">No sessions match “{{ query.trim() }}”</p>

        <template v-else>
          <section v-for="day in days" :key="day.key" class="log-day">
            <h3 class="list-group-label">
              {{ day.label }}
              <span class="group-meta">
                {{ plural(day.rows.length, 'session') }}<template v-if="day.secs"> · {{ formatDuration(day.secs) }}</template>
              </span>
            </h3>
            <ul class="inset-list">
              <li v-for="r in day.rows" :key="r.log.id" class="list-row log-row">
                <PlanetAvatar v-if="hostFor(r.log)" :host="hostFor(r.log)!" :size="32" />
                <span v-else class="row-tile" title="Host no longer saved" aria-hidden="true">
                  <Server :size="15" :stroke-width="1.5" />
                </span>

                <span class="row-main">
                  <span class="row-title">
                    <span class="text-ellipsis">{{ r.log.label }}</span>
                  </span>
                  <span class="row-sub mono text-ellipsis">{{ r.log.username }}@{{ r.log.address }}</span>
                </span>

                <span v-if="hostFor(r.log) && !r.live" class="row-actions">
                  <button class="btn btn-ghost log-connect" :aria-label="`Connect to ${r.log.label}`" @click="reconnect(r.log)">
                    <Plug :size="13" :stroke-width="1.5" aria-hidden="true" />
                    Connect
                  </button>
                </span>

                <span class="log-when">
                  <span class="log-time">{{ formatTime(r.start) }}</span>
                  <span v-if="r.live" class="chip success log-live">
                    <span class="status-dot connected" aria-hidden="true"></span>
                    Connected
                  </span>
                  <span v-else-if="r.secs !== null" class="log-duration">{{ formatDuration(r.secs) }}</span>
                  <span v-else class="log-duration" title="This session didn’t close cleanly">—</span>
                </span>
              </li>
            </ul>
          </section>

          <p v-if="logs.length >= LIMIT" class="list-foot">Showing the latest {{ LIMIT }} sessions</p>
        </template>
      </template>
    </div>
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.clear-btn:hover {
  background: var(--danger-bg);
  color: var(--danger);
}

.log-day + .log-day {
  margin-top: var(--space-4);
}

.log-day .list-group-label {
  margin-bottom: 0;
}

.log-row {
  --row-inset: 58px;
}

.log-connect {
  padding: 4px 9px;
  font-size: var(--text-sm);
  gap: 5px;
}

.log-when {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  min-width: 72px;
  flex-shrink: 0;
  font-variant-numeric: tabular-nums;
}

.log-time {
  font-size: var(--text-sm);
  color: var(--text-2);
}

.log-duration {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.log-live {
  padding-block: 0;
  gap: 5px;
}

.log-live .status-dot {
  width: 6px;
  height: 6px;
}
</style>
