<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import TopBar from './components/TopBar.vue'
import NavRail from './components/NavRail.vue'
import SessionPage from './components/SessionPage.vue'
import HostsView from './views/HostsView.vue'
import KeychainView from './views/KeychainView.vue'
import ForwardsView from './views/ForwardsView.vue'
import SnippetsView from './views/SnippetsView.vue'
import KnownHostsView from './views/KnownHostsView.vue'
import LogsView from './views/LogsView.vue'
import SftpView from './views/SftpView.vue'
import HostDialog from './components/HostDialog.vue'
import HostKeyDialog from './components/HostKeyDialog.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import PasswordPromptDialog from './components/PasswordPromptDialog.vue'
import ImportDialog from './components/ImportDialog.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import CommandPalette from './components/CommandPalette.vue'
import Toasts from './components/Toasts.vue'
import { useSessionsStore } from './stores/sessions'
import { useDialogsStore } from './stores/dialogs'
import { useUiStore } from './stores/ui'

const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

function onKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey)) return
  const key = e.key.toLowerCase()
  if (key === 'k') {
    e.preventDefault()
    dialogs.showPalette = !dialogs.showPalette
  } else if (key === 't') {
    e.preventDefault()
    void sessions.connectLocal()
  } else if (key === 'w') {
    e.preventDefault()
    const current = ui.activeView
    if (current.startsWith('session:')) {
      void sessions.disconnect(Number(current.split(':')[1]))
    }
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
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

        <HostsView v-if="ui.activeView === 'hosts'" />
        <KeychainView v-else-if="ui.activeView === 'keychain'" />
        <ForwardsView v-else-if="ui.activeView === 'forwards'" />
        <SnippetsView v-else-if="ui.activeView === 'snippets'" />
        <KnownHostsView v-else-if="ui.activeView === 'known'" />
        <LogsView v-else-if="ui.activeView === 'logs'" />
        <SftpView v-else-if="ui.activeView === 'sftp'" />
      </main>
    </div>

    <HostDialog />
    <HostKeyDialog />
    <ConfirmDialog />
    <PasswordPromptDialog />
    <ImportDialog />
    <SettingsDialog />
    <CommandPalette />
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
