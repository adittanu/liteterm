<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
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
import Toasts from './components/Toasts.vue'
import { useSessionsStore } from './stores/sessions'
import { useUiStore } from './stores/ui'

const sessions = useSessionsStore()
const ui = useUiStore()

const activeSessionId = computed(() => {
  if (!ui.activeView.startsWith('session:')) return null
  return Number(ui.activeView.split(':')[1])
})

function onKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey)) return
  const key = e.key.toLowerCase()
  if (key === 'k') {
    e.preventDefault()
    // Ctrl+K: jump to a host — open palette (reuses Hosts search semantics)
    ui.openView('hosts')
    window.dispatchEvent(new CustomEvent('liteterm:focus-host-search'))
  } else if (key === 't') {
    e.preventDefault()
    void sessions.connectLocal()
  } else if (key === 'w') {
    e.preventDefault()
    const sid = activeSessionId.value
    if (sid !== null) void sessions.disconnect(sid)
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
        <HostsView v-if="ui.activeView === 'hosts'" />
        <KeychainView v-else-if="ui.activeView === 'keychain'" />
        <ForwardsView v-else-if="ui.activeView === 'forwards'" />
        <SnippetsView v-else-if="ui.activeView === 'snippets'" />
        <KnownHostsView v-else-if="ui.activeView === 'known'" />
        <LogsView v-else-if="ui.activeView === 'logs'" />
        <SftpView v-else-if="ui.activeView === 'sftp'" />
        <SessionPage v-else-if="activeSessionId !== null" :session-id="activeSessionId" />
        <HostsView v-else />
      </main>
    </div>

    <HostDialog />
    <HostKeyDialog />
    <ConfirmDialog />
    <PasswordPromptDialog />
    <ImportDialog />
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
