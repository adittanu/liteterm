import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { api } from '../services/api'
import type { AppSettings } from '../types'

export interface Toast {
  id: number
  kind: 'success' | 'error' | 'info'
  message: string
  actionLabel?: string
  action?: () => void
}

let toastSeq = 1

export const useUiStore = defineStore('ui', () => {
  // ---- settings ----
  const settings = ref<AppSettings>({
    theme: 'system',
    terminalFontSize: 13,
    confirmDeleteFile: true,
    autoSudoPassword: true,
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
    settings,
    resolvedThemeId,
    systemDark,
    loadSettings,
    updateSettings,
    toasts,
    toast,
    dismissToast,
  }
})
