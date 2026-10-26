import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'

import './styles/tokens.css'
import './styles/base.css'
import './styles/motion.css'
import './styles/themes.css'
import './styles/prose.css'
import '@xterm/xterm/css/xterm.css'

const app = createApp(App)
app.use(createPinia())
app.mount('#app')

// Suppress the WebView's browser context menu (Back / Refresh / Print /
// Inspect) — it exposes page-level actions that make no sense in a desktop
// app. Custom menus still work: they handle the event on their own element
// first. Text fields keep the native menu for cut/copy/paste, except
// xterm's hidden helper textarea, which sits under the cursor on right-click.
document.addEventListener('contextmenu', (e) => {
  const target = e.target as HTMLElement | null
  const editable = target?.closest('input, textarea, [contenteditable]:not([contenteditable="false"])')
  if (editable && !target?.closest('.xterm')) return
  e.preventDefault()
})

// Init stores after mount so components can react to data arriving
import { useSessionsStore } from './stores/sessions'
import { useUiStore } from './stores/ui'
import { useHostsStore } from './stores/hosts'
import { useCredentialsStore } from './stores/credentials'
import { useSnippetsStore, useForwardsStore } from './stores/extras'

const ui = useUiStore()
const sessions = useSessionsStore()

void ui.loadSettings()
void sessions.initEvents()
const hostsStore = useHostsStore()
void hostsStore.load()
void hostsStore.initEvents()
void useCredentialsStore().load()
void useSnippetsStore().load()
void useForwardsStore().load()
