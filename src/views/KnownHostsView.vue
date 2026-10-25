<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { Copy, FileDown, ShieldCheck, Trash2 } from 'lucide-vue-next'
import { api } from '../services/api'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import SearchField from '../components/SearchField.vue'
import { copyText, matchesQuery, parseSqlDate, plural, relativeAge, useDelayedFlag } from '../utils/pageHelpers'
import type { KnownHostRow } from '../types'

const dialogs = useDialogsStore()
const ui = useUiStore()

const entries = ref<KnownHostRow[]>([])
const loaded = ref(false)
const loading = ref(false)
const loadError = ref<string | null>(null)
const query = ref('')
const showSpinner = useDelayedFlag(computed(() => loading.value && !loaded.value))

async function load() {
  loading.value = true
  loadError.value = null
  try {
    entries.value = await api.listKnownHosts()
    loaded.value = true
  } catch (e) {
    loadError.value = errorText(e)
  } finally {
    loading.value = false
  }
}

// One row per server; its keys (ed25519, rsa, …) nest underneath
interface HostGroup {
  host: string
  keys: KnownHostRow[]
  addedAt: Date | null
}

const groups = computed<HostGroup[]>(() => {
  const map = new Map<string, KnownHostRow[]>()
  for (const e of entries.value) {
    const list = map.get(e.host)
    if (list) list.push(e)
    else map.set(e.host, [e])
  }
  return [...map.entries()]
    .map(([host, keys]) => {
      const dates = keys.map((k) => parseSqlDate(k.added_at, true)).filter((d): d is Date => !!d)
      return {
        host,
        keys: keys.sort((a, b) => keyRank(a.key_type) - keyRank(b.key_type)),
        addedAt: dates.length ? new Date(Math.min(...dates.map((d) => d.getTime()))) : null,
      }
    })
    .sort((a, b) => a.host.localeCompare(b.host, undefined, { numeric: true }))
})

const visible = computed(() =>
  groups.value.filter((g) =>
    matchesQuery(query.value, g.host, ...g.keys.flatMap((k) => [k.key_type, k.fingerprint])),
  ),
)

function keyRank(t: string): number {
  if (t.includes('ed25519')) return 0
  if (t.includes('ecdsa')) return 1
  if (t.includes('rsa')) return 2
  return 3
}

function keyLabel(t: string): string {
  if (t.includes('ed25519')) return 'ED25519'
  if (t.includes('ecdsa')) return 'ECDSA'
  if (t === 'ssh-rsa' || t.startsWith('rsa')) return 'RSA'
  return t.replace(/^ssh-/, '').toUpperCase()
}

async function forgetKey(entry: KnownHostRow) {
  const ok = await dialogs.confirm({
    title: `Forget the ${keyLabel(entry.key_type)} key for ${entry.host}?`,
    message: 'The next connection will ask you to verify this server again.',
    confirmLabel: 'Forget key',
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteKnownHostEntry(entry.host, entry.key_type)
    entries.value = entries.value.filter((e) => !(e.host === entry.host && e.key_type === entry.key_type))
    ui.toast('info', `Forgot ${keyLabel(entry.key_type)} key for ${entry.host}`)
  } catch (e) {
    ui.toast('error', errorText(e))
    void load()
  }
}

async function forgetHost(g: HostGroup) {
  if (g.keys.length === 1) return forgetKey(g.keys[0])
  const ok = await dialogs.confirm({
    title: `Forget ${g.host}?`,
    message: `All ${g.keys.length} saved keys are removed. The next connection will ask you to verify this server again.`,
    confirmLabel: 'Forget server',
    danger: true,
  })
  if (!ok) return
  try {
    for (const k of g.keys) await api.deleteKnownHostEntry(k.host, k.key_type)
    ui.toast('info', `Forgot ${g.host}`)
  } catch (e) {
    ui.toast('error', errorText(e))
  } finally {
    void load()
  }
}

// Imports can add keys — refresh when the import dialog closes
watch(
  () => dialogs.showImport,
  (open, was) => {
    if (was && !open) void load()
  },
)

onMounted(load)
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-heading">
        <h2 class="page-title">
          Known Hosts
          <span v-if="groups.length" class="page-count">{{ groups.length }}</span>
        </h2>
        <p class="page-sub">Servers you’ve verified. You’ll be warned if a server’s key ever changes.</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-secondary" @click="dialogs.showImport = true">
          <FileDown :size="14" :stroke-width="1.5" aria-hidden="true" />
          Import
        </button>
      </div>
    </header>

    <div v-if="groups.length" class="page-toolbar">
      <SearchField v-model="query" label="Search known hosts" placeholder="Search hosts or fingerprints" />
      <span class="toolbar-spacer"></span>
      <span class="toolbar-label">{{ plural(entries.length, 'key') }}</span>
    </div>

    <div class="page-scroll">
      <div v-if="showSpinner" class="empty-hero">
        <span class="spinner" aria-label="Loading"></span>
      </div>

      <div v-else-if="loadError && !entries.length" class="empty-hero">
        <p class="empty-title">Couldn’t read known hosts</p>
        <p class="mono error-text">{{ loadError }}</p>
        <button class="btn btn-secondary" @click="load">Try again</button>
      </div>

      <template v-else-if="loaded">
        <div v-if="!entries.length" class="empty-hero">
          <span class="empty-icon"><ShieldCheck :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
          <p class="empty-title">No trusted servers yet</p>
          <p class="empty-sub">
            A server’s key is saved the first time you verify it on connect. You can also bring in
            the ones from <code class="mono">~/.ssh/known_hosts</code>.
          </p>
          <button class="btn btn-secondary" @click="dialogs.showImport = true">
            <FileDown :size="14" :stroke-width="1.5" aria-hidden="true" />
            Import from ~/.ssh
          </button>
        </div>

        <p v-else-if="!visible.length" class="no-results">No servers match “{{ query.trim() }}”</p>

        <ul v-else class="inset-list">
          <li v-for="g in visible" :key="g.host" class="list-row kh-row">
            <span class="row-tile success" aria-hidden="true">
              <ShieldCheck :size="15" :stroke-width="1.75" />
            </span>

            <div class="row-main">
              <span class="kh-head">
                <span class="row-title mono kh-host text-ellipsis" :title="g.host">{{ g.host }}</span>
                <span v-if="g.addedAt" class="kh-age" :title="g.addedAt.toLocaleString()">
                  Trusted {{ relativeAge(g.addedAt) }}
                </span>
              </span>

              <div class="kh-keys">
                <div v-for="k in g.keys" :key="k.key_type" class="kh-key">
                  <span class="chip kh-type" :title="k.key_type">{{ keyLabel(k.key_type) }}</span>
                  <code class="kh-fp mono text-ellipsis" :title="k.fingerprint">{{ k.fingerprint || 'No fingerprint' }}</code>
                  <span class="kh-key-actions">
                    <button
                      v-if="k.fingerprint"
                      class="btn-icon"
                      :aria-label="`Copy ${keyLabel(k.key_type)} fingerprint for ${g.host}`"
                      title="Copy fingerprint"
                      @click="copyText(k.fingerprint, 'Fingerprint copied')"
                    >
                      <Copy :size="13" :stroke-width="1.5" aria-hidden="true" />
                    </button>
                    <button
                      v-if="g.keys.length > 1"
                      class="btn-icon danger"
                      :aria-label="`Forget ${keyLabel(k.key_type)} key for ${g.host}`"
                      title="Forget this key"
                      @click="forgetKey(k)"
                    >
                      <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
                    </button>
                  </span>
                </div>
              </div>
            </div>

            <span class="row-actions kh-row-actions">
              <button
                class="btn btn-ghost kh-forget"
                :aria-label="`Forget ${g.host}`"
                @click="forgetHost(g)"
              >
                Forget
              </button>
            </span>
          </li>
        </ul>
      </template>
    </div>
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.kh-row {
  --row-inset: 58px;
  align-items: flex-start;
  padding-block: 12px;
}

.kh-head {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  min-width: 0;
}

.kh-host {
  display: block;
  font-size: var(--text-base);
  min-width: 0;
}

.kh-age {
  flex-shrink: 0;
  font-size: var(--text-xs);
  color: var(--text-4);
}

.kh-keys {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 4px;
}

.kh-key {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
  height: 26px;
}

.kh-type {
  flex-shrink: 0;
  min-width: 58px;
  justify-content: center;
  font-size: 10px;
  letter-spacing: var(--tracking-wide);
}

.kh-fp {
  flex: 0 1 auto;
  min-width: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
  user-select: text;
  cursor: text;
}

.kh-key-actions {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.kh-key:hover .kh-key-actions,
.kh-key:focus-within .kh-key-actions {
  opacity: 1;
}

.kh-key-actions .btn-icon {
  width: 24px;
  height: 24px;
}

.kh-row-actions {
  margin-top: 1px;
}

.kh-forget {
  padding: 4px 10px;
  font-size: var(--text-sm);
}

.kh-forget:hover {
  background: var(--danger-bg);
  color: var(--danger);
}

@media (hover: none) {
  .kh-key-actions { opacity: 1; }
}

code {
  font-size: 0.95em;
}
</style>
