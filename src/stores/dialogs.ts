import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Host } from '../types'

interface ConfirmOptions {
  title: string
  message: string
  confirmLabel: string
  danger?: boolean
}

export const useDialogsStore = defineStore('dialogs', () => {
  // Host create/edit
  const hostDialog = ref<{ open: boolean; host: Host | null; groupId: number | null }>({
    open: false,
    host: null,
    groupId: null,
  })

  function openHostDialog(host: Host | null = null, groupId: number | null = null) {
    hostDialog.value = { open: true, host, groupId }
  }
  function closeHostDialog() {
    hostDialog.value = { open: false, host: null, groupId: null }
  }

  // Confirm (destructive only). resolve(true) only on explicit confirm.
  const confirmState = ref<
    ConfirmOptions & { open: boolean; resolve: ((v: boolean) => void) | null }
  >({ open: false, title: '', message: '', confirmLabel: 'Confirm', danger: false, resolve: null })

  function confirm(opts: ConfirmOptions): Promise<boolean> {
    return new Promise((resolve) => {
      // If a confirm is already open, resolve it as cancelled
      confirmState.value.resolve?.(false)
      confirmState.value = { ...opts, open: true, resolve }
    })
  }
  function answerConfirm(v: boolean) {
    confirmState.value.resolve?.(v)
    confirmState.value = { ...confirmState.value, open: false, resolve: null }
  }

  // Password / key-passphrase prompt at connect time
  const passwordPrompt = ref<{
    open: boolean
    hostId: number
    label: string
    kind: 'password' | 'passphrase'
  } | null>(null)

  function openPasswordPrompt(hostId: number, label: string, kind: 'password' | 'passphrase') {
    passwordPrompt.value = { open: true, hostId, label, kind }
  }
  function closePasswordPrompt() {
    passwordPrompt.value = null
  }

  const showSettings = ref(false)
  const showImport = ref(false)
  const showPalette = ref(false)
  const showQuickNote = ref(false)

  return {
    hostDialog,
    openHostDialog,
    closeHostDialog,
    confirmState,
    confirm,
    answerConfirm,
    passwordPrompt,
    openPasswordPrompt,
    closePasswordPrompt,
    showSettings,
    showImport,
    showPalette,
    showQuickNote,
  }
})
