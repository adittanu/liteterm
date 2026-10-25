<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { WifiOff } from 'lucide-vue-next'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { terminalThemeFor } from '../services/terminalThemes'
import type { SessionTab } from '../types'
import TerminalPane from './TerminalPane.vue'

const props = defineProps<{ sessionId: number }>()

const sessions = useSessionsStore()
const ui = useUiStore()

// The stage behind the terminal paints the exact color the terminal
// canvas paints — the xterm palette is the single source of truth, so
// a CSS theme value can never drift from the terminal background.
const terminalBg = computed(() => terminalThemeFor(ui.settings.theme, ui.systemDark).bg)

const tab = computed(
  () => sessions.tabs.find((t) => t.id === props.sessionId) as SessionTab | undefined,
)

// ---- auto-reconnect countdown ----
// The clock only ticks while a reconnect is pending
const now = ref(Date.now())
let ticker: ReturnType<typeof setInterval> | undefined
watch(
  () => tab.value?.status === 'reconnecting',
  (on) => {
    clearInterval(ticker)
    ticker = undefined
    if (on) {
      now.value = Date.now()
      ticker = setInterval(() => (now.value = Date.now()), 250)
    }
  },
  { immediate: true },
)
onBeforeUnmount(() => clearInterval(ticker))

const reconnectLabel = computed(() => {
  const r = tab.value?.reconnect
  if (!r) return 'Reconnecting…'
  const secs = r.retryAt ? Math.ceil((r.retryAt - now.value) / 1000) : 0
  return secs > 0 ? `Reconnecting in ${secs}s` : 'Reconnecting…'
})

async function resetHostKey(t: SessionTab) {
  if (t.hostId === null) return
  const { api } = await import('../services/api')
  try {
    await api.forgetHostKey(t.hostId)
    await sessions.connect(t.hostId)
  } catch {
    // reconnect attempt surfaces its own errors
  }
}
</script>

<template>
  <div v-if="tab" class="session-page">
    <div class="session-stage" :style="{ '--terminal-bg': terminalBg }">
      <TerminalPane :session="tab" />

      <!-- Dropped connection: the terminal stays readable underneath while
           the backend redials on the same session -->
      <Transition name="banner">
        <div v-if="tab.status === 'reconnecting'" class="reconnect-banner" role="status">
          <WifiOff :size="14" :stroke-width="1.75" aria-hidden="true" />
          <span class="reconnect-text">
            <strong>Connection lost.</strong> {{ reconnectLabel }}
            <span v-if="tab.reconnect" class="reconnect-attempt">
              · attempt {{ tab.reconnect.attempt }} of {{ tab.reconnect.maxAttempts }}
            </span>
          </span>
          <span v-if="tab.reconnect && !tab.reconnect.retryAt" class="spinner small" aria-hidden="true"></span>
          <button
            v-else
            class="btn btn-secondary banner-btn"
            @click="sessions.reconnectNow(tab.id)"
          >
            Reconnect now
          </button>
          <button class="btn btn-ghost banner-btn" @click="sessions.stopReconnecting(tab.id)">
            Stop
          </button>
        </div>
      </Transition>

      <div v-if="tab.status === 'connecting'" class="pane-overlay">
        <span class="spinner" aria-hidden="true"></span>
        <p>Connecting to <strong>{{ tab.address }}</strong>…</p>
      </div>

      <div v-else-if="tab.status === 'error' || tab.status === 'closed'" class="pane-overlay">
        <p class="pane-error">
          {{ tab.status === 'error' ? (tab.error || 'Connection failed') : (tab.error || 'Session closed') }}
        </p>
        <div class="overlay-actions">
          <button
            v-if="tab.hostId !== null"
            class="btn btn-secondary"
            @click="sessions.connect(tab.hostId!)"
          >
            Reconnect
          </button>
          <button
            v-if="tab.hostId !== null && tab.error?.includes('HOST KEY')"
            class="btn btn-secondary"
            title="Forget the saved host key, then connect again with a fresh verification prompt"
            @click="resetHostKey(tab)"
          >
            Reset host key &amp; reconnect
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.session-page {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 0;
}

.session-stage {
  position: relative;
  flex: 1;
  min-height: 0;
  background: var(--terminal-bg);
}

.pane-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  background: var(--terminal-bg);
  text-align: center;
  padding: var(--space-5);
}

.pane-overlay p {
  color: var(--text-3);
  font-size: var(--text-md);
}

.pane-overlay strong {
  color: var(--text-1);
  font-weight: 500;
}

.pane-error {
  color: var(--danger) !important;
  max-width: 48ch;
  font-family: var(--font-mono);
  font-size: var(--text-sm) !important;
  user-select: text;
}

.overlay-actions {
  display: flex;
  gap: var(--space-2);
}

.reconnect-banner {
  position: absolute;
  top: 10px;
  left: 50%;
  z-index: 4;
  display: flex;
  align-items: center;
  gap: var(--space-2);
  max-width: calc(100% - 32px);
  padding: 6px 6px 6px 12px;
  border-radius: var(--radius-lg);
  border: 1px solid color-mix(in oklch, var(--warning) 35%, var(--border));
  background: var(--bg-surface);
  box-shadow: var(--shadow-pop);
  color: var(--warning);
  font-size: var(--text-sm);
  transform: translateX(-50%);
}

.reconnect-text {
  color: var(--text-2);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  font-variant-numeric: tabular-nums;
}

.reconnect-text strong {
  color: var(--text-1);
  font-weight: 600;
}

.reconnect-attempt {
  color: var(--text-4);
}

.banner-btn {
  padding: 4px 10px;
  font-size: var(--text-sm);
}

/* Rare event — a short drop-in from where it sits, faster on the way out */
.banner-enter-active {
  transition: opacity 200ms var(--ease-out), transform 200ms var(--ease-out);
}

.banner-leave-active {
  transition: opacity 140ms var(--ease-out), transform 140ms var(--ease-out);
}

.banner-enter-from,
.banner-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(-6px);
}

@media (prefers-reduced-motion: reduce) {
  .banner-enter-from,
  .banner-leave-to {
    transform: translateX(-50%);
  }
}
</style>
