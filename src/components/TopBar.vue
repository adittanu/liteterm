<script setup lang="ts">
import { computed } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onMounted, onUnmounted, ref } from 'vue'
import { Menu, Plus, X, FolderTree, TerminalSquare } from 'lucide-vue-next'
import { useUiStore } from '../stores/ui'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import appIcon from '../assets/app-icon.png'

const ui = useUiStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const appWindow = getCurrentWindow()

// Maximize glyph flips to "restore" while the window is maximized
const isMaximized = ref(false)
let unlistenResize: (() => void) | null = null

async function syncMaximized() {
  isMaximized.value = await appWindow.isMaximized()
}

onMounted(() => {
  void syncMaximized()
  void appWindow.onResized(() => void syncMaximized()).then((off) => {
    unlistenResize = off
  })
})

onUnmounted(() => {
  unlistenResize?.()
})

function toggleMaximize() {
  void appWindow.toggleMaximize()
}

const tabItems = computed(() => {
  const sftp = { kind: 'sftp' as const, id: 'sftp', label: 'SFTP', status: 'idle' }
  const sessionTabs = sessions.tabs.map((t) => ({
    kind: 'session' as const,
    id: `session:${t.id}`,
    sessionId: t.id,
    label: t.label,
    status: t.status,
  }))
  return [sftp, ...sessionTabs]
})

function isActive(id: string): boolean {
  return ui.activeView === id
}

function openTab(id: string) {
  ui.openView(id as never)
}

function closeSession(sessionId: number) {
  void sessions.disconnect(sessionId)
}
</script>

<template>
  <header class="topbar">
    <button
      class="topbar-btn rail-toggle"
      :aria-label="ui.railCollapsed ? 'Expand navigation' : 'Collapse navigation'"
      title="Toggle navigation"
      @click="ui.railCollapsed = !ui.railCollapsed"
    >
      <Menu :size="17" :stroke-width="1.5" aria-hidden="true" />
    </button>

    <img
      :src="appIcon"
      class="brand-icon"
      alt="LiteTerm"
      title="LiteTerm"
      draggable="false"
      data-tauri-drag-region
    />

    <div class="topbar-tabs" data-tauri-drag-region>
      <div
        v-for="tab in tabItems"
        :key="tab.id"
        class="top-tab"
        :class="{ active: isActive(tab.id) }"
        role="tab"
        :aria-selected="isActive(tab.id)"
        :data-tauri-drag-region="isActive(tab.id) ? undefined : true"
        @click="openTab(tab.id)"
      >
        <template v-if="tab.kind === 'sftp'">
          <FolderTree :size="13" :stroke-width="1.5" aria-hidden="true" />
          <span class="top-tab-label text-ellipsis">{{ tab.label }}</span>
        </template>
        <template v-else>
          <span
            class="top-tab-close"
            role="button"
            :aria-label="`Close ${tab.label}`"
            @click.stop="closeSession(tab.sessionId)"
          >
            <X :size="11" :stroke-width="2" aria-hidden="true" />
          </span>
          <span class="top-tab-label text-ellipsis">{{ tab.label }}</span>
          <TerminalSquare
            :size="13"
            :stroke-width="1.5"
            class="tab-ico"
            :class="{ live: tab.status === 'connected' }"
            aria-hidden="true"
          />
        </template>
      </div>

      <button class="topbar-btn" aria-label="New session" title="Quick connect (Ctrl+K)" @click="dialogs.showPalette = true">
        <Plus :size="15" :stroke-width="1.5" aria-hidden="true" />
      </button>
    </div>

    <div class="topbar-spacer" data-tauri-drag-region></div>

    <div class="window-controls">
      <button class="win-btn" aria-label="Minimize" @click="appWindow.minimize()">
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path d="M2 6h8" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
      </button>
      <button
        class="win-btn"
        :aria-label="isMaximized ? 'Restore' : 'Maximize'"
        @click="toggleMaximize()"
      >
        <!-- Restore: two overlapping squares; Maximize: single square -->
        <svg v-if="isMaximized" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <rect x="1.75" y="3.75" width="6.5" height="6.5" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.25" />
          <path d="M4 2.25h4a2 2 0 0 1 2 2v4.5" fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
        <svg v-else width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <rect x="2.25" y="2.25" width="7.5" height="7.5" rx="1.75" fill="none" stroke="currentColor" stroke-width="1.25" />
        </svg>
      </button>
      <button class="win-btn win-close" aria-label="Close" @click="appWindow.close()">
        <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
          <path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
        </svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
.topbar {
  display: flex;
  align-items: center;
  height: var(--topbar-height);
  flex-shrink: 0;
  /* seamless with the content below — no border, no raised material */
  background: var(--bg);
}

.topbar-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.topbar-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.rail-toggle {
  margin-left: 6px;
}

.brand-icon {
  height: 16px;
  width: 16px;
  border-radius: 4px;
  margin: 0 7px 0 3px;
  align-self: center;
  flex-shrink: 0;
  user-select: none;
  /* neutral 1px outline keeps the dark tile from floating on light themes */
  box-shadow: 0 0 0 1px var(--border);
}

.topbar-tabs {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow-x: auto;
  scrollbar-width: none;
}

.topbar-tabs::-webkit-scrollbar {
  display: none;
}

.top-tab {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 34px;
  padding: 0 10px 0 8px;
  max-width: 200px;
  min-width: 84px;
  font-size: var(--text-base);
  color: var(--text-2);
  border-radius: 10px;
  position: relative;
  cursor: pointer;
  user-select: none;
  flex-shrink: 0;
  transition: color var(--dur-hover) ease, background-color var(--dur-hover) ease;
}

.top-tab:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.top-tab.active {
  color: var(--text-1);
  background: var(--bg-surface-2);
}

.tab-ico {
  color: var(--text-4);
  flex-shrink: 0;
}

.tab-ico.live {
  color: var(--success);
}

.top-tab-label {
  min-width: 0;
  font-weight: 500;
}

.top-tab-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 5px;
  color: var(--text-4);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.top-tab:hover .top-tab-close,
.top-tab.active .top-tab-close {
  opacity: 1;
  color: var(--text-3);
}

.top-tab-close:hover {
  background: var(--bg-active);
  color: var(--text-1);
}

.topbar-spacer {
  flex: 1;
}

.window-controls {
  display: flex;
}

.win-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 32px;
  border-radius: var(--radius-sm);
  margin-right: 2px;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.win-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.win-close:hover {
  background: oklch(0.62 0.22 25);
  color: white;
}
</style>
