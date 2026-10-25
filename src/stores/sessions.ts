import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { useHostsStore } from './hosts'
import { useUiStore } from './ui'
import { useDialogsStore } from './dialogs'
import type { SessionKind, SessionTab, SessionStatus, HostKeyEvent, Transfer } from '../types'

function sessionView(id: number) {
  return `session:${id}` as const
}

type OutputSink = (data: string) => void

export const useSessionsStore = defineStore('sessions', () => {
  const tabs = ref<SessionTab[]>([])
  const activeId = ref<number | null>(null)
  const transfers = ref<Transfer[]>([])

  // TerminalPane registers its xterm sink per session id
  const sinks = new Map<number, OutputSink>()

  // Backend events can arrive before the tab/terminal exists (the tab is
  // created after the invoke roundtrip). Buffer and replay so nothing is
  // lost: lost hostkey prompt = infinite "Connecting…", lost stdout = blank
  // terminal.
  const statusBuffer = new Map<number, { status: SessionStatus; error?: string }>()
  const hostKeyBuffer = new Map<number, HostKeyEvent>()
  const stdoutBuffer = new Map<number, string[]>()
  const MAX_STDOUT_CHUNKS = 200

  function dropBuffers(sessionId: number) {
    statusBuffer.delete(sessionId)
    hostKeyBuffer.delete(sessionId)
    stdoutBuffer.delete(sessionId)
  }

  // Replay buffered state onto a freshly created tab
  function replayBuffered(tab: SessionTab) {
    const key = hostKeyBuffer.get(tab.id)
    if (key) {
      tab.status = 'hostkey'
      tab.hostKeyFingerprint = key.fingerprint
      hostKeyBuffer.delete(tab.id)
    }
    const status = statusBuffer.get(tab.id)
    if (status) {
      tab.status = status.status
      if (status.error !== undefined) tab.error = status.error
      statusBuffer.delete(tab.id)
    }
  }

  function attachSink(sessionId: number, sink: OutputSink) {
    sinks.set(sessionId, sink)
    // Deliver anything that arrived before the terminal mounted
    const buffered = stdoutBuffer.get(sessionId)
    if (buffered) {
      for (const chunk of buffered) sink(chunk)
      stdoutBuffer.delete(sessionId)
    }
  }
  function detachSink(sessionId: number) {
    sinks.delete(sessionId)
  }

  async function connect(hostId: number, pendingPassword?: string, opts?: { silent?: boolean }) {
    const hosts = useHostsStore()
    const host = hosts.hostById(hostId)
    if (!host) return
    hosts.touchHost(hostId)

    // Reconnect: reuse tab for same host if its session ended
    const existing = tabs.value.find(
      (t) => t.hostId === hostId && (t.status === 'closed' || t.status === 'error'),
    )
    if (existing) {
      await disconnect(existing.id)
      tabs.value = tabs.value.filter((t) => t.id !== existing.id)
    }

    const dup = tabs.value.find((t) => t.hostId === hostId)
    if (dup) {
      activeId.value = dup.id
      return
    }

    try {
      const sessionId = await api.connect(hostId, pendingPassword ?? undefined)
      const tab: SessionTab = {
        id: sessionId,
        kind: 'ssh',
        hostId,
        label: host.label,
        address: `${host.username}@${host.address}`,
        status: 'connecting',
        sftpOpen: false,
      }
      tabs.value.push(tab)
      replayBuffered(tab)
      activeId.value = sessionId
      // Silent connections (e.g. started from SFTP) don't hijack the view
      if (!opts?.silent) useUiStore().openView(sessionView(sessionId))
    } catch (e) {
      const msg = errorText(e)
      // Backend asks for credentials at connect time
      if (msg.startsWith('password_required:')) {
        useDialogsStore().openPasswordPrompt(hostId, msg.split(':')[1] ?? host.label, 'password')
      } else if (msg.startsWith('passphrase_required:')) {
        useDialogsStore().openPasswordPrompt(hostId, msg.split(':')[1] ?? host.label, 'passphrase')
      } else {
        useUiStore().toast('error', `Failed to start connection: ${msg}`)
      }
    }
  }

  // Local terminal: no auth, no host key — connected as soon as the process
  // spawns. `command` runs in place of the default shell (e.g. the herdr TUI).
  // Multiple local terminals are allowed.
  async function spawnLocalTab(
    meta: { kind: SessionKind; label: string; address: string },
    command?: string,
  ) {
    try {
      const sessionId = await api.spawnLocal(command)
      const tab: SessionTab = {
        id: sessionId,
        kind: meta.kind,
        hostId: null,
        label: meta.label,
        address: meta.address,
        status: 'connecting',
        sftpOpen: false,
      }
      tabs.value.push(tab)
      replayBuffered(tab)
      activeId.value = sessionId
      useUiStore().openView(sessionView(sessionId))
    } catch (e) {
      useUiStore().toast(
        'error',
        `Could not open ${meta.label === 'Local' ? 'local terminal' : meta.label}: ${errorText(e)}`,
      )
    }
  }

  function connectLocal() {
    return spawnLocalTab({ kind: 'local', label: 'Local', address: 'this PC' })
  }

  // herdr multiplexer: a local PTY whose child is the herdr TUI itself
  function connectHerdr() {
    return spawnLocalTab(
      { kind: 'herdr', label: 'herdr', address: 'agent multiplexer' },
      'herdr.exe',
    )
  }

  function setStatus(sessionId: number, status: SessionStatus, error?: string) {
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (!tab) return
    tab.status = status
    if (error !== undefined) tab.error = error
    else if (status === 'connected') tab.error = undefined
  }

  async function confirmHostKey(sessionId: number, accept: boolean, remember: boolean) {
    await api.confirmHostKey(sessionId, accept, remember)
  }

  async function disconnect(sessionId: number) {
    try {
      await api.disconnect(sessionId)
    } catch {
      // session may already be gone — safe to ignore
    }
    sinks.delete(sessionId)
    dropBuffers(sessionId)
    transfers.value = transfers.value.filter((t) => t.sessionId !== sessionId)
    tabs.value = tabs.value.filter((t) => t.id !== sessionId)
    if (activeId.value === sessionId) {
      activeId.value = tabs.value.length ? tabs.value[tabs.value.length - 1].id : null
    }
    // If we were looking at this session's page, fall back to the last open
    // session or the hosts dashboard
    const ui = useUiStore()
    if (ui.activeView === sessionView(sessionId)) {
      ui.openView(
        activeId.value !== null ? sessionView(activeId.value) : 'hosts',
      )
    }
  }

  function write(sessionId: number, data: string) {
    void api.write(sessionId, data).catch(() => {
      // session died; status event will update the tab
    })
  }

  function resize(sessionId: number, cols: number, rows: number) {
    void api.resize(sessionId, cols, rows).catch(() => {})
  }

  // ---- transfers ----
  function trackTransfer(t: Transfer) {
    transfers.value.push(t)
  }

  function updateTransfer(transferId: number, patch: Partial<Transfer>) {
    const t = transfers.value.find((x) => x.id === transferId)
    if (t) Object.assign(t, patch)
  }

  function removeTransfer(transferId: number) {
    transfers.value = transfers.value.filter((t) => t.id !== transferId)
  }

  async function cancelTransfer(transferId: number) {
    try {
      await api.sftpCancelTransfer(transferId)
    } catch {
      // already finished
    }
  }

  // ---- backend event wiring (called once from main.ts) ----
  async function initEvents() {
    await listen<{ sessionId: number; data: string }>('ssh:stdout', (e) => {
      const { sessionId, data } = e.payload
      const sink = sinks.get(sessionId)
      if (sink) {
        sink(data)
        return
      }
      // Terminal not mounted yet — buffer for replay in attachSink
      let buf = stdoutBuffer.get(sessionId)
      if (!buf) {
        buf = []
        stdoutBuffer.set(sessionId, buf)
      }
      buf.push(data)
      if (buf.length > MAX_STDOUT_CHUNKS) buf.shift()
    })

    await listen<{
      sessionId: number
      status: SessionStatus
      error?: string
    }>('session:status', (e) => {
      const { sessionId, status, error } = e.payload
      const tab = tabs.value.find((t) => t.id === sessionId)
      if (!tab) {
        statusBuffer.set(sessionId, { status, error })
        return
      }
      setStatus(sessionId, status, error)
      if (status === 'error' && error) {
        useUiStore().toast('error', `${tab.label}: ${error}`)
      }
    })

    await listen<HostKeyEvent>('hostkey:pending', (e) => {
      const tab = tabs.value.find((t) => t.id === e.payload.sessionId)
      if (!tab) {
        hostKeyBuffer.set(e.payload.sessionId, e.payload)
        return
      }
      tab.status = 'hostkey'
      tab.hostKeyFingerprint = e.payload.fingerprint
    })

    await listen<{
      transferId: number
      transferred: number
      size: number
      done: boolean
      error?: string
    }>('transfer:progress', (e) => {
      updateTransfer(e.payload.transferId, {
        transferred: e.payload.transferred,
        size: e.payload.size,
        done: e.payload.done,
        error: e.payload.error,
      })
    })

    await listen<{ sessionId: number }>('sudo:filled', (e) => {
      const tab = tabs.value.find((t) => t.id === e.payload.sessionId)
      useUiStore().toast(
        'info',
        `Sudo password filled automatically${tab ? ` for ${tab.label}` : ''}`,
        { duration: 2500 },
      )
    })
  }

  return {
    tabs,
    activeId,
    transfers,
    connect,
    connectLocal,
    connectHerdr,
    confirmHostKey,
    disconnect,
    write,
    resize,
    attachSink,
    detachSink,
    trackTransfer,
    updateTransfer,
    removeTransfer,
    cancelTransfer,
    initEvents,
  }
})

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e && typeof e === 'object' && 'message' in e) return String((e as { message: unknown }).message)
  return String(e)
}
