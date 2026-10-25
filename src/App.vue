<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
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
import Toasts from './components/Toasts.vue'
import { useSessionsStore } from './stores/sessions'
import { useDialogsStore } from './stores/dialogs'
import { useUiStore, type ViewId } from './stores/ui'
import { useUpdaterStore } from './stores/updater'
import { actionFor, captureActive, FONT_SIZE_MAX, FONT_SIZE_MIN } from './services/shortcuts'

const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()
const updater = useUpdaterStore()

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
function tabViews(): string[] {
  return ['sftp', ...sessions.tabs.map((t) => `session:${t.id}`)]
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
    case 'closeTab': {
      const view = ui.activeView
      if (view.startsWith('session:')) void sessions.disconnect(Number(view.slice(8)))
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
             xterm state survive view switches -->
        <SessionPage
          v-for="tab in sessions.tabs"
          :key="tab.id"
          v-show="ui.activeView === `session:${tab.id}`"
          :session-id="tab.id"
        />

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
</style>
