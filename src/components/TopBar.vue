<script setup lang="ts">
import { computed } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { onMounted, onUnmounted, ref } from 'vue'
import { Menu, Plus, X, FolderTree } from 'lucide-vue-next'
import { useUiStore } from '../stores/ui'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { comboFor } from '../services/shortcuts'

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

// The whole bar is the window drag region, minus anything interactive.
// data-tauri-drag-region only fires when the mousedown lands on the exact
// element carrying the attribute, so we drive the window API ourselves.
// Double-click is detected manually: e.detail is unreliable once
// startDragging has swallowed a mouseup, and it also counts clicks that
// landed on interactive elements outside the drag region.
const DRAG_EXCLUDE =
  'button, a, input, select, textarea, [contenteditable], [role="tab"], [role="button"], [data-no-drag]'

let lastDragDown = { x: -9999, y: -9999, t: -Infinity }

function onTopbarMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  const target = e.target as HTMLElement | null
  if (target?.closest?.(DRAG_EXCLUDE)) return
  e.preventDefault()

  // Windows defaults: 500ms double-click time, ~4px double-click distance
  const now = performance.now()
  const isDoubleClick =
    now - lastDragDown.t < 500 &&
    Math.abs(e.clientX - lastDragDown.x) < 5 &&
    Math.abs(e.clientY - lastDragDown.y) < 5
  lastDragDown = isDoubleClick
    ? { x: -9999, y: -9999, t: -Infinity }
    : { x: e.clientX, y: e.clientY, t: now }

  if (isDoubleClick) toggleMaximize()
  else void appWindow.startDragging()
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

function dotState(status: string): string {
  if (status === 'connected') return 'live'
  if (status === 'connecting' || status === 'hostkey' || status === 'reconnecting') return 'connecting'
  if (status === 'error') return 'error'
  return ''
}

function closeSession(sessionId: number) {
  void sessions.disconnect(sessionId)
}
</script>

<template>
  <header class="topbar" @mousedown="onTopbarMouseDown">
    <button
      class="topbar-btn rail-toggle"
      :aria-label="ui.railCollapsed ? 'Expand navigation' : 'Collapse navigation'"
      title="Toggle navigation"
      @click="ui.railCollapsed = !ui.railCollapsed"
    >
      <Menu :size="17" :stroke-width="1.5" aria-hidden="true" />
    </button>

    <div class="topbar-tabs">
      <div
        v-for="tab in tabItems"
        :key="tab.id"
        class="top-tab"
        :class="{ active: isActive(tab.id) }"
        role="tab"
        :aria-selected="isActive(tab.id)"
        @click="openTab(tab.id)"
      >
        <template v-if="tab.kind === 'sftp'">
          <FolderTree :size="13" :stroke-width="1.5" aria-hidden="true" />
          <span class="top-tab-label text-ellipsis">{{ tab.label }}</span>
        </template>
        <template v-else>
          <span class="top-tab-dot" :class="dotState(tab.status)" aria-hidden="true"></span>
          <span class="top-tab-label text-ellipsis">{{ tab.label }}</span>
          <span
            class="top-tab-close"
            role="button"
            :aria-label="`Close ${tab.label}`"
            @click.stop="closeSession(tab.sessionId)"
          >
            <X :size="11" :stroke-width="2" aria-hidden="true" />
          </span>
        </template>
      </div>

      <button
        class="topbar-btn"
        aria-label="New session"
        :title="`Quick connect (${comboFor('palette', ui.settings)})`"
        @click="dialogs.showPalette = true"
      >
        <Plus :size="15" :stroke-width="1.5" aria-hidden="true" />
      </button>
    </div>

    <div class="topbar-spacer"></div>

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
  /* acts as the title bar — nothing in it is selectable text */
  user-select: none;
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
  /* Center at x=24, the same column the collapsed rail icons use */
  margin-left: 8px;
}

.topbar-tabs {
  display: flex;
  align-items: center;
  gap: 2px;
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
  gap: 6px;
  height: 28px;
  padding: 0 8px;
  max-width: 180px;
  font-size: var(--text-base);
  color: var(--text-3);
  border-radius: 7px;
  position: relative;
  cursor: pointer;
  user-select: none;
  flex-shrink: 0;
  transition:
    color var(--dur-hover) ease,
    background-color var(--dur-hover) ease,
    padding var(--dur-hover) ease;
}

/* Reserve room for the hover-revealed close button only while it is shown */
.top-tab:has(.top-tab-close):hover,
.top-tab:has(.top-tab-close).active {
  padding-right: 28px;
}

.top-tab:hover {
  color: var(--text-1);
}

.top-tab.active {
  color: var(--text-1);
  background: var(--bg-surface-2);
}

.top-tab-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--text-4);
  flex-shrink: 0;
}

.top-tab-dot.live {
  background: var(--success);
  box-shadow: 0 0 5px var(--success-bg);
}

.top-tab-dot.connecting {
  background: var(--warning);
  animation: tab-dot-pulse 1.2s ease-in-out infinite alternate;
}

.top-tab-dot.error {
  background: var(--danger);
}

@keyframes tab-dot-pulse {
  from {
    opacity: 0.35;
  }
  to {
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .top-tab-dot.connecting {
    animation: none;
  }
}

.top-tab-label {
  min-width: 0;
  font-weight: 500;
}

.top-tab-close {
  position: absolute;
  top: 50%;
  right: 6px;
  transform: translateY(-50%);
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 4px;
  color: var(--text-4);
  opacity: 0;
  transition:
    opacity var(--dur-hover) ease,
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease;
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
