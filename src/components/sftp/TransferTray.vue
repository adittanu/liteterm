<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { ArrowDownToLine, ArrowUpFromLine, ArrowLeftRight, Check, ChevronDown, CircleAlert, X } from 'lucide-vue-next'
import { useSessionsStore } from '../../stores/sessions'
import type { Transfer } from '../../types'
import { fmtBytes } from './fsUtils'

// Floating transfer tray. Finished uploads clear themselves after a few
// seconds (paused while the pointer is over the tray, like Sonner);
// failures stay until dismissed.
const props = defineProps<{ destinations: Record<string, string> }>()

const sessions = useSessionsStore()
const collapsed = ref(false)
const hovering = ref(false)

const key = (t: Transfer) => `${t.sessionId}:${t.id}`

// ---- throughput: smoothed bytes/s per transfer ----
const samples = reactive<Record<string, { t: number; bytes: number; speed: number }>>({})

watch(
  () => sessions.transfers.map((t) => `${key(t)}=${t.transferred}`).join('|'),
  () => {
    const now = performance.now()
    for (const t of sessions.transfers) {
      const k = key(t)
      const s = samples[k]
      if (!s) {
        samples[k] = { t: now, bytes: t.transferred, speed: 0 }
        continue
      }
      const dt = (now - s.t) / 1000
      if (dt < 0.25) continue
      const inst = Math.max(0, (t.transferred - s.bytes) / dt)
      s.speed = s.speed ? s.speed * 0.7 + inst * 0.3 : inst
      s.t = now
      s.bytes = t.transferred
    }
  },
)

const active = computed(() => sessions.transfers.filter((t) => !t.done && !t.error))
const failed = computed(() => sessions.transfers.filter((t) => t.error && t.error !== 'Cancelled'))
const finished = computed(() => sessions.transfers.filter((t) => t.done || t.error))

const overall = computed(() => {
  const sized = active.value.filter((t) => t.size > 0)
  const size = sized.reduce((s, t) => s + t.size, 0)
  const done = sized.reduce((s, t) => s + t.transferred, 0)
  return size ? done / size : null
})

const title = computed(() => {
  if (active.value.length) {
    const pct = overall.value !== null ? ` · ${Math.round(overall.value * 100)}%` : ''
    return `${active.value.length === 1 ? 'Transferring 1 file' : `Transferring ${active.value.length} files`}${pct}`
  }
  if (failed.value.length) return failed.value.length === 1 ? '1 transfer failed' : `${failed.value.length} transfers failed`
  return 'All transfers complete'
})

function pct(t: Transfer): number {
  return t.size ? Math.min(100, (t.transferred / t.size) * 100) : 0
}

function detail(t: Transfer): string {
  if (t.error) return t.error === 'Cancelled' ? 'Cancelled' : t.error
  if (t.done) return t.size > 1 ? `${fmtBytes(t.size)} · Done` : 'Done'
  if (!t.size) return 'Starting…'
  const s = samples[key(t)]?.speed ?? 0
  const left = s > 0 ? (t.size - t.transferred) / s : null
  const eta = left === null ? '' : left < 60 ? ` · ${Math.max(1, Math.round(left))}s left` : ` · ${Math.round(left / 60)} min left`
  return `${fmtBytes(t.transferred)} of ${fmtBytes(t.size)}${s > 0 ? ` · ${fmtBytes(s)}/s` : ''}${eta}`
}

function icon(t: Transfer) {
  if (t.fileName.includes(' → ')) return ArrowLeftRight
  return t.kind === 'upload' ? ArrowUpFromLine : ArrowDownToLine
}

function dismiss(t: Transfer) {
  delete samples[key(t)]
  sessions.removeTransfer(t.id)
}

function clearFinished() {
  for (const t of finished.value) dismiss(t)
}

// ---- auto-clear successful transfers ----
const timers = new Map<string, ReturnType<typeof setTimeout>>()

function schedule(t: Transfer) {
  const k = key(t)
  if (timers.has(k)) return
  timers.set(
    k,
    setTimeout(() => {
      timers.delete(k)
      if (hovering.value) return schedule(t) // try again once the pointer leaves
      dismiss(t)
    }, 5000),
  )
}

watch(
  () => sessions.transfers.filter((t) => t.done && !t.error).map(key).join('|'),
  () => {
    for (const t of sessions.transfers) if (t.done && !t.error) schedule(t)
  },
  { immediate: true },
)

onBeforeUnmount(() => timers.forEach(clearTimeout))
</script>

<template>
  <Transition name="tray">
    <aside
      v-if="sessions.transfers.length"
      class="tray material"
      :class="{ collapsed }"
      aria-label="Transfers"
      @pointerenter="hovering = true"
      @pointerleave="hovering = false"
    >
      <header class="tray-head">
        <button class="tray-toggle" :aria-expanded="!collapsed" @click="collapsed = !collapsed">
          <span v-if="active.length" class="spinner small" aria-hidden="true"></span>
          <CircleAlert v-else-if="failed.length" :size="14" :stroke-width="1.75" class="head-icon fail" aria-hidden="true" />
          <Check v-else :size="14" :stroke-width="2" class="head-icon ok" aria-hidden="true" />
          <span class="tray-title" aria-live="polite">{{ title }}</span>
          <ChevronDown :size="14" :stroke-width="1.75" class="tray-chev" aria-hidden="true" />
        </button>
        <button v-if="finished.length && !collapsed" class="tray-clear" @click="clearFinished">Clear</button>
      </header>

      <div v-if="collapsed && overall !== null" class="tray-overall" aria-hidden="true">
        <div class="bar-fill" :style="{ transform: `scaleX(${overall})` }"></div>
      </div>

      <ul v-if="!collapsed" class="tray-list">
        <li v-for="t in sessions.transfers" :key="key(t)" class="xfer" :class="{ done: t.done && !t.error, failed: t.error && t.error !== 'Cancelled' }">
          <span class="xfer-icon" aria-hidden="true">
            <component :is="icon(t)" :size="14" :stroke-width="1.75" />
          </span>
          <div class="xfer-main">
            <div class="xfer-top">
              <span class="xfer-name text-ellipsis" :title="t.fileName">{{ t.fileName }}</span>
              <span v-if="!t.done && !t.error && t.size" class="xfer-pct">{{ Math.round(pct(t)) }}%</span>
            </div>
            <div
              v-if="!t.error"
              class="bar"
              role="progressbar"
              :aria-label="`${t.fileName} progress`"
              aria-valuemin="0"
              aria-valuemax="100"
              :aria-valuenow="Math.round(t.done ? 100 : pct(t))"
            >
              <div
                class="bar-fill"
                :class="{ indeterminate: !t.done && !t.size, complete: t.done }"
                :style="t.size || t.done ? { transform: `scaleX(${t.done ? 1 : pct(t) / 100})` } : undefined"
              ></div>
            </div>
            <span class="xfer-meta text-ellipsis" :title="detail(t)">
              {{ detail(t) }}<template v-if="props.destinations[key(t)] && !t.error"> · {{ props.destinations[key(t)] }}</template>
            </span>
          </div>
          <button
            class="btn-icon xfer-x"
            :aria-label="t.done || t.error ? `Dismiss ${t.fileName}` : `Cancel ${t.fileName}`"
            :title="t.done || t.error ? 'Dismiss' : 'Cancel'"
            @click="t.done || t.error ? dismiss(t) : sessions.cancelTransfer(t.id)"
          >
            <X :size="13" :stroke-width="1.75" aria-hidden="true" />
          </button>
        </li>
      </ul>
    </aside>
  </Transition>
</template>

<style scoped>
.tray {
  position: absolute;
  right: var(--space-3);
  bottom: 52px;
  z-index: 20;
  width: 340px;
  max-width: calc(100% - 2 * var(--space-3));
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border);
  box-shadow: var(--shadow-3);
  overflow: hidden;
}

.tray-head {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 6px 4px 4px;
}

.tray-toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
  min-width: 0;
  height: 32px;
  padding: 0 8px;
  border-radius: var(--radius-md);
  text-align: left;
  transition: background-color var(--dur-hover) ease;
}

.tray-toggle:hover {
  background: var(--bg-hover);
}

.head-icon.ok { color: var(--success); }
.head-icon.fail { color: var(--danger); }

.tray-title {
  flex: 1;
  min-width: 0;
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-1);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.tray-chev {
  color: var(--text-3);
  transition: transform 200ms var(--ease-out);
}

.tray.collapsed .tray-chev {
  transform: rotate(180deg);
}

.tray-clear {
  padding: 4px 9px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.tray-clear:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.tray-overall {
  height: 2px;
  margin: 0 12px 8px;
  border-radius: 2px;
  background: var(--bg-surface-3);
  overflow: hidden;
}

.tray-list {
  display: flex;
  flex-direction: column;
  max-height: 240px;
  overflow-y: auto;
  padding: 0 6px 6px;
  border-top: 1px solid var(--border);
}

.xfer {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 9px 4px 8px 8px;
}

.xfer + .xfer {
  border-top: 1px solid var(--border);
}

.xfer-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  margin-top: 1px;
  border-radius: 7px;
  background: var(--accent-bg);
  color: var(--accent);
  flex-shrink: 0;
  transition: background-color 200ms ease, color 200ms ease;
}

.xfer.done .xfer-icon {
  background: var(--success-bg);
  color: var(--success);
}

.xfer.failed .xfer-icon {
  background: var(--danger-bg);
  color: var(--danger);
}

.xfer-main {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  min-width: 0;
}

.xfer-top {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.xfer-name {
  flex: 1;
  min-width: 0;
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-1);
}

.xfer-pct {
  font-size: var(--text-xs);
  color: var(--text-2);
  font-variant-numeric: tabular-nums;
}

.bar {
  height: 4px;
  border-radius: 999px;
  background: var(--bg-surface-3);
  overflow: hidden;
}

/* scaleX, not width: progress repaints on the compositor */
.bar-fill {
  height: 100%;
  width: 100%;
  border-radius: inherit;
  background: var(--accent);
  transform-origin: left center;
  transform: scaleX(0);
  transition: transform 200ms linear, background-color 200ms ease;
}

.bar-fill.complete {
  background: var(--success);
}

.bar-fill.indeterminate {
  width: 32%;
  transform: none;
  animation: indeterminate 1.1s var(--ease-move) infinite;
}

@keyframes indeterminate {
  from { translate: -100% 0; }
  to { translate: 320% 0; }
}

.xfer-meta {
  font-size: var(--text-xs);
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}

.xfer.failed .xfer-meta {
  color: var(--danger);
  white-space: normal;
  word-break: break-word;
  user-select: text;
}

.xfer-x {
  width: 24px;
  height: 24px;
  flex-shrink: 0;
}

.tray-enter-active {
  transition: transform 240ms var(--ease-out), opacity 240ms var(--ease-out);
}

.tray-leave-active {
  transition: transform 160ms var(--ease-out), opacity 160ms var(--ease-out);
}

.tray-enter-from {
  transform: translateY(10px) scale(0.98);
  opacity: 0;
}

.tray-leave-to {
  transform: translateY(6px);
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .tray-enter-from,
  .tray-leave-to {
    transform: none;
  }
}
</style>
