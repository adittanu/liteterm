import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { api } from '../services/api'
import type { AppSettings } from '../types'

export type ViewId =
  | 'hosts'
  | 'keychain'
  | 'forwards'
  | 'snippets'
  | 'notes'
  | 'known'
  | 'logs'
  | 'sftp'
  | `session:${number}`

export interface Toast {
  id: number
  kind: 'success' | 'error' | 'info'
  message: string
  actionLabel?: string
  action?: () => void
}

let toastSeq = 1

export const useUiStore = defineStore('ui', () => {
  // ---- view routing (Termius-style shell) ----
  const activeView = ref<ViewId>('hosts')
  const railCollapsed = ref(false)
  // Set while the rail was auto-collapsed for an immersive view, so
  // returning to a page restores it; manual collapse stays collapsed.
  const railAutoCollapsed = ref(false)

  // A host the SFTP view should open on its server side next time it looks
  // (set from Hosts / the palette, consumed by SftpView)
  const sftpHostRequest = ref<number | null>(null)

  function openSftpFor(hostId: number) {
    sftpHostRequest.value = hostId
    openView('sftp')
  }

  // A note NotesView should select next time it looks (set from a host's
  // details panel, consumed by NotesView)
  const noteRequest = ref<number | null>(null)

  function openNote(noteId: number) {
    noteRequest.value = noteId
    openView('notes')
  }

  // Find-in-terminal requests: the visible TerminalPane whose session id
  // matches opens its find bar; `n` bumps so repeats re-trigger the watch
  const terminalFindRequest = ref<{ sessionId: number; n: number } | null>(null)

  function requestTerminalFind(sessionId: number) {
    terminalFindRequest.value = { sessionId, n: (terminalFindRequest.value?.n ?? 0) + 1 }
  }

  function openView(view: ViewId) {
    const immersive = view === 'sftp' || view.startsWith('session:')
    if (immersive) {
      if (!railCollapsed.value) {
        railCollapsed.value = true
        railAutoCollapsed.value = true
      }
    } else if (railAutoCollapsed.value) {
      railCollapsed.value = false
      railAutoCollapsed.value = false
    }
    activeView.value = view
  }

  // ---- settings ----
  const settings = ref<AppSettings>({
    theme: 'system',
    terminalFontSize: 13,
    confirmDeleteFile: true,
    autoSudoPassword: true,
    autoReconnect: true,
    keybindings: {},
    hostsLayout: 'modern',
  })

  async function loadSettings() {
    settings.value = await api.getSettings()
    applyTheme(settings.value.theme)
  }

  async function updateSettings(patch: Partial<AppSettings>) {
    settings.value = { ...settings.value, ...patch }
    await api.setSettings(settings.value)
    if (patch.theme) applyTheme(patch.theme)
  }

  // ---- terminal render A/B (temporary, per-machine in localStorage) ----
  // Compares xterm's WebGL renderer against the DOM renderer, and real bold
  // weight against regular weight, to match Windows Terminal's text.
  const RENDER_KEY = 'liteterm.terminalRender'
  const terminalRender = ref<{ webgl: boolean; realBold: boolean }>({
    webgl: true,
    realBold: false,
    ...(() => {
      try {
        return JSON.parse(localStorage.getItem(RENDER_KEY) || '{}')
      } catch {
        return {}
      }
    })(),
  })
  watch(
    terminalRender,
    (v) => localStorage.setItem(RENDER_KEY, JSON.stringify(v)),
    { deep: true },
  )

  // ---- theme: snap, never smear (design contract) ----
  const systemDark = ref(
    window.matchMedia('(prefers-color-scheme: dark)').matches,
  )
  window
    .matchMedia('(prefers-color-scheme: dark)')
    .addEventListener('change', (e) => {
      systemDark.value = e.matches
    })

  // The actual data-theme value on <html> ('system' resolves to dark/light)
  const resolvedThemeId = ref<string>('dark')

  function applyTheme(theme: string) {
    const next =
      theme === 'system' ? (systemDark.value ? 'dark' : 'light') : theme
    resolvedThemeId.value = next
    const root = document.documentElement
    root.classList.add('theme-snap')
    root.dataset.theme = next
    // Force a reflow with suppression active, then release on the next frame
    void root.getBoundingClientRect()
    requestAnimationFrame(() => root.classList.remove('theme-snap'))
  }

  watch(systemDark, () => {
    if (settings.value.theme === 'system') applyTheme('system')
  })

  // ---- toasts (undo/feedback channel — not confirm dialogs) ----
  const toasts = ref<Toast[]>([])

  function toast(
    kind: Toast['kind'],
    message: string,
    opts?: { actionLabel?: string; action?: () => void; duration?: number },
  ): number {
    const id = toastSeq++
    toasts.value.push({ id, kind, message, ...opts })
    const duration = opts?.duration ?? 4000
    if (duration > 0) {
      setTimeout(() => dismissToast(id), duration)
    }
    return id
  }

  function dismissToast(id: number) {
    const i = toasts.value.findIndex((t) => t.id === id)
    if (i !== -1) toasts.value.splice(i, 1)
  }

  return {
    activeView,
    railCollapsed,
    openView,
    sftpHostRequest,
    openSftpFor,
    noteRequest,
    openNote,
    terminalFindRequest,
    requestTerminalFind,
    settings,
    resolvedThemeId,
    systemDark,
    loadSettings,
    updateSettings,
    terminalRender,
    toasts,
    toast,
    dismissToast,
  }
})
