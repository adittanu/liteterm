<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import TopBar from './components/TopBar.vue'
import NavRail from './components/NavRail.vue'
import SessionPage from './components/SessionPage.vue'
import HostsView from './views/HostsView.vue'
import HostsClassicView from './views/HostsClassicView.vue'
import KeychainView from './views/KeychainView.vue'
import ForwardsView from './views/ForwardsView.vue'
import SnippetsView from './views/SnippetsView.vue'
import NotesView from './views/NotesView.vue'
import KnownHostsView from './views/KnownHostsView.vue'
import LogsView from './views/LogsView.vue'
import SftpView from './views/SftpView.vue'
import HostDialog from './components/HostDialog.vue'
import GroupDialog from './components/GroupDialog.vue'
import HostKeyDialog from './components/HostKeyDialog.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import PasswordPromptDialog from './components/PasswordPromptDialog.vue'
import ImportDialog from './components/ImportDialog.vue'
import ExportDialog from './components/ExportDialog.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import CommandPalette from './components/CommandPalette.vue'
import QuickNoteDialog from './components/QuickNoteDialog.vue'
import SnippetVarsDialog from './components/SnippetVarsDialog.vue'
import Toasts from './components/Toasts.vue'
import { useSessionsStore } from './stores/sessions'
import { useDialogsStore } from './stores/dialogs'
import { useUiStore, type ViewId } from './stores/ui'
import { useUpdaterStore } from './stores/updater'
import { usePrivacyStore } from './stores/privacy'
import { actionFor, captureActive, FONT_SIZE_MAX, FONT_SIZE_MIN } from './services/shortcuts'
import type { SessionTab } from './types'

const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()
const updater = useUpdaterStore()
const privacy = usePrivacyStore()

// Shortcut id -> view it opens ('viewNotes' -> 'notes', …)
const VIEW_TARGETS: Record<string, ViewId> = {
  viewHosts: 'hosts',
  viewKeychain: 'keychain',
  viewForwards: 'forwards',
  viewSnippets: 'snippets',
  viewNotes: 'notes',
  viewKnown: 'known',
  viewLogs: 'logs',
  viewSftp: 'sftp',
}

// Tab order mirrors the top bar: SFTP first, then sessions in creation order
// (a split's second pane belongs to its tab, not a tab of its own)
function tabViews(): string[] {
  return ['sftp', ...sessions.tabs.filter((t) => !t.parentId).map((t) => `session:${t.id}`)]
}

// ---- the open session tab and its panes ----

const activeRoot = computed<SessionTab | null>(() => {
  const view = ui.activeView
  if (!view.startsWith('session:')) return null
  const id = Number(view.slice(8))
  return sessions.tabs.find((t) => t.id === id && !t.parentId) ?? null
})

const splitShown = computed(() => {
  const s = activeRoot.value?.split
  return !!s && sessions.tabs.some((t) => t.id === s.childId)
})

// A pane's own view id (from a toast, the palette, a snippet) shows its tab,
// with that pane focused
watch(
  () => ui.activeView,
  (view) => {
    if (!view.startsWith('session:')) return
    const id = Number(view.slice(8))
    const tab = sessions.tabs.find((t) => t.id === id)
    if (tab?.parentId !== undefined) {
      sessions.activeId = id
      ui.openView(`session:${tab.parentId}`)
    }
  },
)

const DIVIDER = 1 // px, the line between two panes

function paneStyle(tab: SessionTab): Record<string, string> {
  const root = activeRoot.value
  const split = root?.split
  if (!root || !split || !splitShown.value || sessions.rootOf(tab.id) !== root.id) return { inset: '0' }
  const first = tab.id === root.id
  const r = split.ratio
  const a = `calc(${r * 100}% - ${DIVIDER / 2}px)`
  const b = `calc(${(1 - r) * 100}% - ${DIVIDER / 2}px)`
  return split.dir === 'right'
    ? { top: '0', bottom: '0', left: first ? '0' : 'auto', right: first ? 'auto' : '0', width: first ? a : b }
    : { left: '0', right: '0', top: first ? '0' : 'auto', bottom: first ? 'auto' : '0', height: first ? a : b }
}

const dividerStyle = computed((): Record<string, string> => {
  const s = activeRoot.value?.split
  if (!s) return {}
  const at = `calc(${s.ratio * 100}% - ${DIVIDER / 2}px)`
  const style: Record<string, string> =
    s.dir === 'right'
      ? { top: '0', bottom: '0', left: at, width: `${DIVIDER}px` }
      : { left: '0', right: '0', top: at, height: `${DIVIDER}px` }
  return style
})

const sessionsEl = ref<HTMLElement | null>(null)
const drag = ref<{ pointerId: number } | null>(null)

function startDrag(e: PointerEvent) {
  const split = activeRoot.value?.split
  const box = sessionsEl.value
  if (!split || !box) return
  e.preventDefault()
  const target = e.currentTarget as HTMLElement
  target.setPointerCapture(e.pointerId)
  drag.value = { pointerId: e.pointerId }
  const move = (ev: PointerEvent) => {
    const rect = box.getBoundingClientRect()
    const pos = split.dir === 'right' ? (ev.clientX - rect.left) / rect.width : (ev.clientY - rect.top) / rect.height
    // Neither pane may get too small to be a terminal
    split.ratio = Math.min(0.85, Math.max(0.15, pos))
  }
  const end = () => {
    drag.value = null
    target.removeEventListener('pointermove', move)
    target.removeEventListener('pointerup', end)
    target.removeEventListener('pointercancel', end)
  }
  target.addEventListener('pointermove', move)
  target.addEventListener('pointerup', end)
  target.addEventListener('pointercancel', end)
}

/** The pane with the keyboard in the open tab, if a session tab is open */
function focusedSession(): number | null {
  const root = activeRoot.value
  return root ? sessions.focusedPane(root.id) : null
}

function cycleTab(delta: number) {
  const views = tabViews()
  if (!views.length) return
  const i = views.indexOf(ui.activeView)
  const next =
    i === -1
      ? delta > 0
        ? 0
        : views.length - 1
      : (i + delta + views.length) % views.length
  ui.openView(views[next] as ViewId)
}

function bumpFontSize(delta: number) {
  const next = Math.min(FONT_SIZE_MAX, Math.max(FONT_SIZE_MIN, ui.settings.terminalFontSize + delta))
  if (next !== ui.settings.terminalFontSize) void ui.updateSettings({ terminalFontSize: next })
}

function runAction(id: string) {
  if (VIEW_TARGETS[id]) {
    ui.openView(VIEW_TARGETS[id])
    return
  }
  switch (id) {
    case 'palette':
      dialogs.showPalette = !dialogs.showPalette
      break
    case 'newLocal':
      void sessions.connectLocal()
      break
    case 'quickNote':
      dialogs.showQuickNote = !dialogs.showQuickNote
      break
    case 'openSettings':
      dialogs.showSettings = true
      break
    case 'fontSizeUp':
      bumpFontSize(1)
      break
    case 'fontSizeDown':
      bumpFontSize(-1)
      break
    case 'privacyMode':
      privacy.toggle()
      break
    case 'closeTab': {
      // In a split, the pane with the keyboard; else the whole tab
      const sid = focusedSession()
      if (sid !== null) void sessions.disconnect(sid)
      break
    }
    case 'terminalFind': {
      const sid = focusedSession()
      if (sid !== null) ui.requestTerminalFind(sid)
      break
    }
    case 'splitRight':
    case 'splitDown': {
      const sid = focusedSession()
      if (sid !== null) sessions.splitPane(sid, id === 'splitRight' ? 'right' : 'down')
      break
    }
    case 'focusOtherPane': {
      const root = activeRoot.value
      if (root?.split && splitShown.value) {
        const next = sessions.focusedPane(root.id) === root.id ? root.split.childId : root.id
        sessions.activeId = next
        ui.requestTerminalFocus(next)
      }
      break
    }
    case 'nextTab':
      cycleTab(1)
      break
    case 'prevTab':
      cycleTab(-1)
      break
    default: {
      if (id.startsWith('tab')) {
        const target = tabViews()[Number(id.slice(3)) - 1]
        if (target) ui.openView(target as ViewId)
      }
    }
  }
}

function onKeydown(e: KeyboardEvent) {
  // The Settings dialog owns the keyboard while it's open — rebinding
  // capture and stray presses must never fire app actions behind the modal.
  if (e.repeat || captureActive.value || dialogs.showSettings) return
  const id = actionFor(e, ui.settings)
  if (!id) return
  e.preventDefault()
  runAction(id)
}

// Capture phase so the shortcut fires before xterm's textarea handlers,
// which swallow keydowns while a terminal has focus
onMounted(() => {
  window.addEventListener('keydown', onKeydown, true)
  // Quiet update check on launch — only surfaces a dot when a newer
  // release exists; nothing downloads without the user clicking.
  void updater.init()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown, true))
</script>

<template>
  <div class="app">
    <TopBar />
    <div class="app-body">
      <NavRail />
      <main class="app-main">
        <!-- Session terminals stay mounted (v-show) so scrollback and
             xterm state survive view switches. A split tab places its two
             panes by style alone: a pane never moves in the DOM, so
             splitting, closing a pane or promoting one keeps its terminal. -->
        <div v-show="activeRoot" ref="sessionsEl" class="sessions" :class="{ dragging: !!drag, rows: activeRoot?.split?.dir === 'down' }">
          <SessionPage
            v-for="tab in sessions.tabs"
            :key="tab.id"
            v-show="activeRoot !== null && sessions.rootOf(tab.id) === activeRoot.id"
            class="pane"
            :class="{ 'pane-focused': splitShown && sessions.focusedPane(activeRoot!.id) === tab.id }"
            :style="paneStyle(tab)"
            :session-id="tab.id"
          />
          <div
            v-if="splitShown"
            class="split-divider"
            :class="activeRoot!.split!.dir"
            :style="dividerStyle"
            role="separator"
            :aria-orientation="activeRoot!.split!.dir === 'right' ? 'vertical' : 'horizontal'"
            aria-label="Resize panes"
            @pointerdown="startDrag"
            @dblclick="activeRoot!.split!.ratio = 0.5"
          ></div>
        </div>

        <template v-if="ui.activeView === 'hosts'">
          <HostsClassicView v-if="ui.settings.hostsLayout === 'classic'" />
          <HostsView v-else />
        </template>
        <KeychainView v-else-if="ui.activeView === 'keychain'" />
        <ForwardsView v-else-if="ui.activeView === 'forwards'" />
        <SnippetsView v-else-if="ui.activeView === 'snippets'" />
        <NotesView v-else-if="ui.activeView === 'notes'" />
        <KnownHostsView v-else-if="ui.activeView === 'known'" />
        <LogsView v-else-if="ui.activeView === 'logs'" />
        <SftpView v-else-if="ui.activeView === 'sftp'" />
      </main>
    </div>

    <HostDialog />
    <GroupDialog />
    <HostKeyDialog />
    <ConfirmDialog />
    <PasswordPromptDialog />
    <ImportDialog />
    <ExportDialog />
    <SettingsDialog />
    <CommandPalette />
    <QuickNoteDialog />
    <SnippetVarsDialog />
    <Toasts />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.app-body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.app-main {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

/* ---- session panes (one, or two in a split) ---- */
.sessions {
  position: relative;
  flex: 1;
  min-height: 0;
}

.sessions.dragging {
  cursor: col-resize;
  user-select: none;
}

.sessions.dragging.rows {
  cursor: row-resize;
}

.sessions.dragging .pane {
  pointer-events: none;
}

.pane {
  position: absolute;
  height: auto;
}

/* The pane with the keyboard, when there are two */
.pane-focused::after {
  content: '';
  position: absolute;
  inset: 0;
  border: 1px solid color-mix(in oklch, var(--accent) 55%, transparent);
  pointer-events: none;
  z-index: 4;
}

.split-divider {
  position: absolute;
  z-index: 5;
  background: var(--border);
  touch-action: none;
}

.split-divider::before {
  /* A wider grab area than the 1px line */
  content: '';
  position: absolute;
  inset: -4px;
}

.split-divider.right {
  cursor: col-resize;
}

.split-divider.down {
  cursor: row-resize;
}

.split-divider:hover,
.sessions.dragging .split-divider {
  background: var(--accent);
}
</style>
