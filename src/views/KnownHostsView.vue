<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ShieldCheck, Trash2, FileDown } from 'lucide-vue-next'
import { api } from '../services/api'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { KnownHostRow } from '../types'

const dialogs = useDialogsStore()
const ui = useUiStore()

const entries = ref<KnownHostRow[]>([])
const loading = ref(false)
const loadError = ref<string | null>(null)

async function load() {
  loading.value = true
  loadError.value = null
  try {
    entries.value = await api.listKnownHosts()
  } catch (e) {
    loadError.value = errorText(e)
  } finally {
    loading.value = false
  }
}

async function removeEntry(entry: KnownHostRow) {
  const ok = await dialogs.confirm({
    title: `Forget host key for ${entry.host}?`,
    message: `The ${entry.key_type} key will be removed. The next connection will ask for verification again.`,
    confirmLabel: 'Forget key',
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteKnownHostEntry(entry.host, entry.key_type)
    await load()
  } catch (e) {
    ui.toast('error', errorText(e))
  }
}

function shortFingerprint(fp: string): string {
  return fp || '—'
}

onMounted(load)
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h2 class="page-title">Known Hosts</h2>
        <p class="page-sub">Trusted server keys, used to verify connections (TOFU).</p>
      </div>
      <button class="btn btn-secondary" @click="dialogs.showImport = true">
        <FileDown :size="14" :stroke-width="1.5" aria-hidden="true" />
        Import
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

      <div v-else-if="!entries.length" class="empty-state page-empty">
        <ShieldCheck :size="22" :stroke-width="1.5" aria-hidden="true" />
        <p>
          No trusted host keys yet. Keys are remembered when you connect and verify a
          host, or imported from <code class="mono">~/.ssh/known_hosts</code>.
        </p>
      </div>

      <div v-else class="card-grid">
        <div v-for="entry in entries" :key="`${entry.host}|${entry.key_type}`" class="kh-card">
          <span class="kh-icon" aria-hidden="true">
            <ShieldCheck :size="16" :stroke-width="1.5" />
          </span>
          <span class="kh-text">
            <span class="kh-host mono text-ellipsis">{{ entry.host }}</span>
            <span class="kh-meta text-ellipsis" :title="entry.fingerprint">
              {{ entry.key_type }} · {{ shortFingerprint(entry.fingerprint) }}
            </span>
          </span>
          <span class="kh-actions">
            <button class="btn-icon" :aria-label="`Forget key for ${entry.host}`" title="Forget key" @click="removeEntry(entry)">
              <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </span>
        </div>
      </div>
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

.kh-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  min-width: 0;
  transition: background-color var(--dur-hover) ease;
}

.kh-card:hover {
  background: var(--bg-hover);
}

.kh-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 38px;
  height: 38px;
  border-radius: var(--radius-md);
  background: var(--success-bg);
  color: var(--success);
  flex-shrink: 0;
}

.kh-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  flex: 1;
  line-height: 1.35;
}

.kh-host {
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-1);
}

.kh-meta {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.kh-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.kh-card:hover .kh-actions,
.kh-card:focus-within .kh-actions {
  opacity: 1;
}

.kh-actions .btn-icon:hover {
  color: var(--danger);
}
</style>
