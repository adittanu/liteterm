import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'

import './styles/tokens.css'
import './styles/base.css'
import './styles/motion.css'
import './styles/themes.css'
import '@xterm/xterm/css/xterm.css'

const app = createApp(App)
app.use(createPinia())
app.mount('#app')

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
void useHostsStore().load()
void useCredentialsStore().load()
void useSnippetsStore().load()
void useForwardsStore().load()
