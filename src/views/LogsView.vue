<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Clock, Trash2 } from 'lucide-vue-next'
import { api } from '../services/api'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { SessionLog } from '../types'

const dialogs = useDialogsStore()
const ui = useUiStore()

const logs = ref<SessionLog[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)

async function load() {
  loading.value = true
  loadError.value = null
  try {
    logs.value = await api.listSessionLogs()
  } catch (e) {
    loadError.value = errorText(e)
  } finally {
    loading.value = false
  }
}

async function clearAll() {
  const ok = await dialogs.confirm({
    title: 'Clear all session history?',
    message: 'The log of past connections will be deleted from this machine.',
    confirmLabel: 'Clear',
    danger: true,
  })
  if (!ok) return
  await api.clearSessionLogs()
  await load()
  ui.toast('info', 'Session history cleared')
}

function formatDate(iso: string): string {
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  return d.toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function duration(log: SessionLog): string {
  if (!log.ended_at) return '—'
  const start = new Date(log.started_at).getTime()
  const end = new Date(log.ended_at).getTime()
  if (Number.isNaN(start) || Number.isNaN(end)) return '—'
  const secs = Math.max(0, Math.round((end - start) / 1000))
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  if (h > 0) return `${h}h ${m}m`
  if (m > 0) return `${m}m ${s}s`
  return `${s}s`
}

const rows = computed(() => logs.value)

onMounted(() => {
  void load()
})
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h2 class="page-title">Logs</h2>
        <p class="page-sub">SSH session history stored on this machine.</p>
      </div>
      <button v-if="logs.length" class="btn btn-secondary" @click="clearAll">
        <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
        Clear history
      </button>
    </header>

    <div class="page-scroll">
      <div v-if="loading" class="empty-state page-empty">
        <span class="spinner" aria-hidden="true"></span>
      </div>

      <div v-else-if="loadError" class="empty-state page-empty">
        <p class="mono error-text">{{ loadError }}</p>
        <button class="btn btn-secondary" @click="load">Retry</button>
      </div>

      <div v-else-if="!rows.length" class="empty-state page-empty">
        <Clock :size="22" :stroke-width="1.5" aria-hidden="true" />
        <p>No sessions yet. Connect to a host and it will show up here.</p>
      </div>

      <table v-else class="log-table">
        <thead>
          <tr>
            <th>Date</th>
            <th>Host</th>
            <th>User</th>
            <th class="num">Duration</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="log in rows" :key="log.id">
            <td class="log-date">{{ formatDate(log.started_at) }}</td>
            <td>
              <span class="log-host">{{ log.label }}</span>
              <span class="log-addr mono">{{ log.address }}</span>
            </td>
            <td class="mono log-user">{{ log.username }}</td>
            <td class="num log-duration" :class="{ open: !log.ended_at }">
              {{ log.ended_at ? duration(log) : 'Connected' }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.error-text {
  color: var(--danger);
  font-size: var(--text-xs);
  word-break: break-all;
  user-select: text;
}

.log-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-base);
}

.log-table th {
  text-align: left;
  font-size: var(--text-xs);
  font-weight: 600;
  letter-spacing: var(--tracking-wide);
  text-transform: uppercase;
  color: var(--text-4);
  padding: 8px 12px;
  border-bottom: 1px solid var(--border);
}

.log-table td {
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  color: var(--text-1);
  vertical-align: top;
}

.log-table tbody tr {
  transition: background-color var(--dur-hover) ease;
}

.log-table tbody tr:hover {
  background: var(--bg-hover);
}

.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.log-date {
  white-space: nowrap;
  color: var(--text-2);
}

.log-host {
  display: block;
  font-weight: 500;
}

.log-addr {
  display: block;
  font-size: var(--text-xs);
  color: var(--text-4);
}

.log-user {
  color: var(--text-2);
}

.log-duration.open {
  color: var(--success);
  font-weight: 500;
}
</style>
