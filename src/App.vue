<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import AppTitlebar from './components/AppTitlebar.vue'
import Sidebar from './components/Sidebar.vue'
import TerminalTabs from './components/TerminalTabs.vue'
import WelcomePane from './components/WelcomePane.vue'
import SftpPanel from './components/SftpPanel.vue'
import HostDialog from './components/HostDialog.vue'
import HostKeyDialog from './components/HostKeyDialog.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import PasswordPromptDialog from './components/PasswordPromptDialog.vue'
import ImportDialog from './components/ImportDialog.vue'
import SnippetsDialog from './components/SnippetsDialog.vue'
import ForwardsDialog from './components/ForwardsDialog.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import CommandPalette from './components/CommandPalette.vue'
import Toasts from './components/Toasts.vue'
import { useSessionsStore } from './stores/sessions'
import { useDialogsStore } from './stores/dialogs'

const sessions = useSessionsStore()
const dialogs = useDialogsStore()

const activeTab = computed(
  () => sessions.tabs.find((t) => t.id === sessions.activeId) ?? null,
)

function onKeydown(e: KeyboardEvent) {
  if (!(e.ctrlKey || e.metaKey)) return
  const key = e.key.toLowerCase()
  if (key === 'k') {
    e.preventDefault()
    dialogs.showPalette = !dialogs.showPalette
  } else if (key === 't') {
    e.preventDefault()
    dialogs.showPalette = true
  } else if (key === 'w') {
    e.preventDefault()
    if (sessions.activeId !== null) void sessions.disconnect(sessions.activeId)
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div class="app">
    <AppTitlebar />
    <div class="app-body">
      <Sidebar />
      <main class="app-main">
        <div class="app-terminal-area">
          <TerminalTabs v-if="sessions.tabs.length" />
          <WelcomePane v-else />
        </div>
        <SftpPanel v-if="activeTab?.sftpOpen" :session-id="activeTab.id" />
      </main>
    </div>

    <HostDialog />
    <HostKeyDialog />
    <ConfirmDialog />
    <PasswordPromptDialog />
    <ImportDialog />
    <SnippetsDialog />
    <ForwardsDialog />
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
  display: flex;
  flex: 1;
  min-width: 0;
}

.app-terminal-area {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}
</style>
