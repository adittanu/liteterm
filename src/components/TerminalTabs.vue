<script setup lang="ts">
import { computed } from 'vue'
import { X, FolderTree, RotateCw, SquareTerminal, ShieldOff } from 'lucide-vue-next'
import { api } from '../services/api'
import { useSessionsStore } from '../stores/sessions'
import type { SessionStatus, SessionTab } from '../types'
import TerminalPane from './TerminalPane.vue'

const sessions = useSessionsStore()

const orderedTabs = computed(() => sessions.tabs)
const activeTab = computed(() =>
  sessions.tabs.find((t) => t.id === sessions.activeId) ?? null,
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

function toggleSftp() {
  const tab = activeTab.value
  if (!tab || tab.status !== 'connected') return
  tab.sftpOpen = !tab.sftpOpen
}

async function resetHostKey(tab: SessionTab) {
  if (tab.hostId === null) return
  try {
    await api.forgetHostKey(tab.hostId)
    await sessions.connect(tab.hostId)
  } catch {
    // forgetHostKey failure is unlikely; reconnect attempt surfaces its own errors
  }
}
</script>

<template>
  <div class="term-wrap">
    <!-- Tab strip: switching tabs is high-frequency → no animation -->
    <div class="tabbar" role="tablist" aria-label="Open sessions">
      <div class="tabbar-scroll">
        <button
          v-for="tab in orderedTabs"
          :key="tab.id"
          class="tab"
          :class="{ active: tab.id === sessions.activeId }"
          role="tab"
          :aria-selected="tab.id === sessions.activeId"
          :title="`${tab.label} — ${statusText(tab.status)}`"
          @click="sessions.activeId = tab.id"
        >
          <span class="status-dot" :class="tab.status === 'connected' ? 'connected' : tab.status === 'error' ? 'error' : tab.status === 'closed' ? 'idle' : 'connecting'"></span>
          <span class="tab-label text-ellipsis">{{ tab.label }}</span>
          <span
            class="tab-close"
            role="button"
            :aria-label="`Close ${tab.label}`"
            @click.stop="sessions.disconnect(tab.id)"
          >
            <X :size="12" :stroke-width="2" aria-hidden="true" />
          </span>
        </button>
      </div>

      <div class="tabbar-actions">
        <button
          class="btn-icon"
          aria-label="New local terminal"
          title="New local terminal"
          @click="sessions.connectLocal()"
        >
          <SquareTerminal :size="15" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <button
          v-if="activeTab && activeTab.kind === 'ssh'"
          class="btn-icon"
          :aria-label="activeTab.sftpOpen ? 'Close file browser' : 'Open file browser'"
          :title="activeTab.sftpOpen ? 'Close file browser' : 'Open file browser'"
          :disabled="!activeTab.sftpOpen && activeTab.status !== 'connected'"
          @click="toggleSftp"
        >
          <FolderTree :size="15" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>
    </div>

    <!-- Panes: keyboard tab switches mount instantly (no transition) -->
    <div class="panes">
      <div
        v-for="tab in orderedTabs"
        v-show="tab.id === sessions.activeId"
        :key="tab.id"
        class="pane"
      >
        <TerminalPane :session="tab" />

        <!-- Overlays for non-connected states: static cues, motion-free -->
        <div v-if="tab.status === 'connecting'" class="pane-overlay">
          <span class="spinner" aria-hidden="true"></span>
          <p>Connecting to <strong>{{ tab.address }}</strong>…</p>
        </div>

        <div v-else-if="tab.status === 'hostkey'" class="pane-overlay">
          <!-- HostKeyDialog renders globally -->
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
              <RotateCw :size="14" :stroke-width="1.5" aria-hidden="true" />
              Reconnect
            </button>
            <button
              v-if="tab.hostId !== null && tab.error?.includes('HOST KEY')"
              class="btn btn-secondary"
              title="Forget the saved host key for this server, then connect again with a fresh verification prompt"
              @click="resetHostKey(tab)"
            >
              <ShieldOff :size="14" :stroke-width="1.5" aria-hidden="true" />
              Reset host key &amp; reconnect
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.term-wrap {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-height: 0;
}

.tabbar {
  display: flex;
  align-items: center;
  height: var(--tabbar-height);
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  padding: 0 var(--space-2) 0 0;
}

.tabbar-scroll {
  display: flex;
  align-items: stretch;
  overflow-x: auto;
  scrollbar-width: none;
  flex: 0 1 auto;
  min-width: 0;
}

.tabbar-scroll::-webkit-scrollbar {
  display: none;
}

.tab {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 100%;
  padding: 0 10px 0 12px;
  max-width: 200px;
  min-width: 110px;
  font-size: var(--text-base);
  line-height: 1;
  color: var(--text-2);
  border-right: 1px solid var(--border);
  position: relative;
  transition: color var(--dur-hover) ease, background-color var(--dur-hover) ease;
}

.tab:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.tab.active {
  color: var(--text-1);
  background: var(--bg-hover);
}

/* Active tab indicator sits on the tabbar's bottom border */
.tab.active::after {
  content: '';
  position: absolute;
  bottom: -1px;
  left: 0;
  right: 0;
  height: 2px;
  background: var(--accent);
}

.tab-label {
  flex: 1;
  min-width: 0;
  font-weight: 500;
}

.tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 4px;
  color: var(--text-4);
  opacity: 0;
  transition: opacity var(--dur-hover) ease, background-color var(--dur-hover) ease;
}

.tab:hover .tab-close,
.tab.active .tab-close {
  opacity: 1;
}

.tab-close:hover {
  background: var(--bg-active);
  color: var(--text-1);
}

.tabbar-actions {
  display: flex;
  align-items: center;
  gap: 2px;
  margin-left: auto;
  padding-left: var(--space-2);
}

.panes {
  position: relative;
  flex: 1;
  min-height: 0;
}

.pane {
  position: absolute;
  inset: 0;
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

.overlay-actions {
  display: flex;
  gap: var(--space-2);
}

.pane-error {
  color: var(--danger) !important;
  max-width: 48ch;
  font-family: var(--font-mono);
  font-size: var(--text-sm) !important;
  user-select: text;
}
</style>
