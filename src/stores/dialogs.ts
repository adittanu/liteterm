import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { Host, HostGroup, Snippet } from '../types'
import { snippetVars } from '../utils/snippetVars'

interface ConfirmOptions {
  title: string
  message: string
  confirmLabel: string
  danger?: boolean
}

// Quick-connect prefill: host fields parsed from a `user@host:port` string,
// plus the IPs the domain resolved to (shown as a hint only)
export interface HostPrefill {
  address: string
  port: number
  username: string | null
  ips?: string[]
  /** From a pasted `ssh -i …` command */
  keyPath?: string | null
  /** From a pasted `ssh -J …` command that names a saved host */
  jumpHostId?: number | null
}

export const useDialogsStore = defineStore('dialogs', () => {
  // Host create/edit
  const hostDialog = ref<{
    open: boolean
    host: Host | null
    groupId: number | null
    prefill: HostPrefill | null
    connectAfter: boolean
    /** The vault a new host starts in (null: personal) */
    teamId: string | null
  }>({
    open: false,
    host: null,
    groupId: null,
    prefill: null,
    connectAfter: false,
    teamId: null,
  })

  function openHostDialog(
    host: Host | null = null,
    groupId: number | null = null,
    prefill: HostPrefill | null = null,
    connectAfter = false,
    teamId: string | null = null,
  ) {
    hostDialog.value = { open: true, host, groupId, prefill, connectAfter, teamId }
  }
  function closeHostDialog() {
    hostDialog.value = { open: false, host: null, groupId: null, prefill: null, connectAfter: false, teamId: null }
  }

  // Group create/rename
  const groupDialog = ref<{ open: boolean; group: HostGroup | null; teamId: string | null }>({
    open: false,
    group: null,
    teamId: null,
  })

  /** `teamId` is the vault a new group starts in (null: personal) */
  function openGroupDialog(group: HostGroup | null = null, teamId: string | null = null) {
    groupDialog.value = { open: true, group, teamId }
  }
  function closeGroupDialog() {
    groupDialog.value = { open: false, group: null, teamId: null }
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
    // The connect that asked came from SFTP or Forwards, which must keep the
    // view they are on instead of being thrown to the terminal tab
    silent: boolean
  } | null>(null)

  function openPasswordPrompt(
    hostId: number,
    label: string,
    kind: 'password' | 'passphrase',
    silent = false,
  ) {
    passwordPrompt.value = { open: true, hostId, label, kind, silent }
  }
  function closePasswordPrompt() {
    passwordPrompt.value = null
  }

  const showSettings = ref(false)
  const showImport = ref(false)
  const showExport = ref(false)
  const showPalette = ref(false)
  const showQuickNote = ref(false)

  // Values for a snippet's {{placeholders}}, asked right before it runs
  const snippetFill = ref<{
    snippet: Snippet
    /** "Run" or "Insert": names the button */
    verb: string
    resolve: (command: string | null) => void
  } | null>(null)

  /**
   * The snippet's command ready to send: as it is when it has no
   * placeholders, else filled in from a dialog. null = cancelled.
   */
  function fillSnippet(snippet: Snippet, verb = 'Run'): Promise<string | null> {
    if (!snippetVars(snippet.command).length) return Promise.resolve(snippet.command)
    snippetFill.value?.resolve(null)
    return new Promise((resolve) => {
      snippetFill.value = { snippet, verb, resolve }
    })
  }
  function answerSnippetFill(command: string | null) {
    snippetFill.value?.resolve(command)
    snippetFill.value = null
  }

  return {
    hostDialog,
    openHostDialog,
    closeHostDialog,
    groupDialog,
    openGroupDialog,
    closeGroupDialog,
    confirmState,
    confirm,
    answerConfirm,
    passwordPrompt,
    openPasswordPrompt,
    closePasswordPrompt,
    showSettings,
    showImport,
    showExport,
    showPalette,
    showQuickNote,
    snippetFill,
    fillSnippet,
    answerSnippetFill,
  }
})
