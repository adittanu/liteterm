<script setup lang="ts">
import { computed } from 'vue'
import { useSessionsStore } from '../stores/sessions'
import type { SessionTab, SessionStatus } from '../types'
import TerminalPane from './TerminalPane.vue'

const props = defineProps<{ sessionId: number }>()

const sessions = useSessionsStore()
const tab = computed(
  () => sessions.tabs.find((t) => t.id === props.sessionId) as SessionTab | undefined,
)

function statusText(status: SessionStatus): string {
  switch (status) {
    case 'connecting': return 'Connecting…'
    case 'hostkey': return 'Verify host'
    case 'connected': return 'Connected'
    case 'error': return 'Failed'
    case 'closed': return 'Closed'
  }
}

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
    <div class="session-status-bar">
      <span class="status-dot" :class="tab.status === 'connected' ? 'connected' : tab.status === 'error' ? 'error' : tab.status === 'closed' ? 'idle' : 'connecting'"></span>
      <span class="session-title text-ellipsis">{{ tab.label }}</span>
      <span class="session-sub text-ellipsis mono">{{ tab.address }}</span>
      <span class="session-state">{{ statusText(tab.status) }}</span>
    </div>

    <div class="session-stage">
      <TerminalPane :session="tab" />

      <div v-if="tab.status === 'connecting'" class="pane-overlay">
        <span class="spinner" aria-hidden="true"></span>
        <p>Connecting to <strong>{{ tab.address }}</strong>…</p>
      </div>

      <div v-else-if="tab.status === 'error' || tab.status === 'closed'" class="pane-overlay">
        <p class="pane-error">
          {{ tab.status === 'error' ? (tab.error || 'Connection failed') : 'Session closed' }}
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

.session-status-bar {
  display: flex;
  align-items: center;
  gap: 9px;
  height: 40px;
  padding: 0 var(--space-4);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  min-width: 0;
}

.session-title {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.session-sub {
  font-size: var(--text-xs);
  color: var(--text-3);
}

.session-state {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.session-stage {
  position: relative;
  flex: 1;
  min-height: 0;
  background: var(--bg-terminal);
}

.pane-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  background: var(--bg-terminal);
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
</style>
