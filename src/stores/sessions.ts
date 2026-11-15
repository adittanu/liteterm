import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { thisComputerLower } from '../utils/platform'
import { hostTarget, protocolOf } from '../utils/hostProtocol'
import { useHostsStore } from './hosts'
import { useUiStore } from './ui'
import { useDialogsStore } from './dialogs'
import { usePrivacyStore } from './privacy'
import type { SessionKind, SessionTab, SessionStatus, HostKeyEvent, Transfer, SplitDir } from '../types'

function sessionView(id: number) {
  return `session:${id}` as const
}

type OutputSink = (data: string) => void

export const useSessionsStore = defineStore('sessions', () => {
  const tabs = ref<SessionTab[]>([])
  const activeId = ref<number | null>(null)
  const transfers = ref<Transfer[]>([])

  // Bumped when a connect attempt fails before any session exists (key file
  // missing, host gone, …). No tab appears then, so whoever waits for a
  // connection (an SFTP pane, a port-forward switch) watches this instead of
  // sitting out a 90 s timeout.
  const connectFailures = ref<Record<number, number>>({})

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
      tab.hostKeyAddress = key.address
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
    inputObservers.delete(sessionId)
  }

  // Everything sent to a session — typed keys, pastes, snippets, a note's
  // Run — passes write(); TerminalPane watches it for Enter at a prompt
  const inputObservers = new Map<number, (data: string) => void>()
  function observeInput(sessionId: number, fn: (data: string) => void) {
    inputObservers.set(sessionId, fn)
  }

  // ---- title, busy, background notices ----

  function setTitle(sessionId: number, raw: string) {
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (!tab) return
    let title = raw.trim()
    // A shell's own path ("C:\…\pwsh.exe") says nothing the tab doesn't
    if (/\.exe$/i.test(title)) title = ''
    // Debian/Ubuntu bash sets "user@host: ~/dir"; the tab already names the host
    title = title.replace(/^[^@\s]+@[^:\s]+:\s*/, '')
    tab.title = title.slice(0, 120) || undefined
  }

  function setBusy(sessionId: number, busy: boolean) {
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (tab && !!tab.busy !== busy) tab.busy = busy
  }

  const lastBell = new Map<number, number>()
  const BELL_EVERY_MS = 30_000

  function took(ms: number): string {
    const s = Math.round(ms / 1000)
    if (s < 60) return `${s}s`
    const m = Math.floor(s / 60)
    if (m < 60) return `${m}m ${s % 60}s`
    return `${Math.floor(m / 60)}h ${m % 60}m`
  }

  /**
   * A long command finished, or a program rang the bell (agent CLIs do when
   * they're done or need an answer), in a tab you aren't looking at: a toast
   * that jumps to it, plus a desktop notice when LiteTerm isn't in front.
   */
  function notifyActivity(sessionId: number, kind: 'done' | 'bell', ms = 0) {
    const ui = useUiStore()
    if (!ui.settings.notifyCommandDone) return
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (!tab) return
    const focused = document.hasFocus()
    if (focused && paneVisible(sessionId)) return
    if (kind === 'bell') {
      const now = Date.now()
      if (now - (lastBell.get(sessionId) ?? 0) < BELL_EVERY_MS) return
      lastBell.set(sessionId, now)
    }
    const what = kind === 'done' ? `finished a command after ${took(ms)}` : 'needs your attention'
    ui.toast('info', `${tab.label} ${what}`, {
      actionLabel: 'Show',
      action: () => {
        activeId.value = sessionId
        ui.openView(sessionView(rootOf(sessionId)))
      },
      duration: 8000,
    })
    if (!focused) {
      // The desktop notice sits outside the window: honour privacy mode there too
      const name = usePrivacyStore().t(tab.label)
      void api.notify(name, kind === 'done' ? `Command finished after ${took(ms)}` : 'Needs your attention').catch(() => {})
    }
  }

  // ---- split panes: a tab may show a second session beside or below it ----

  /** The tab a session is shown in: itself, or the tab it is the second pane of */
  function rootOf(sessionId: number): number {
    return tabs.value.find((t) => t.id === sessionId)?.parentId ?? sessionId
  }

  /** The session's pane is on screen (its tab is the open view) */
  function paneVisible(sessionId: number): boolean {
    return useUiStore().activeView === sessionView(rootOf(sessionId))
  }

  /** The pane that has the keyboard in `rootId`'s tab */
  function focusedPane(rootId: number): number {
    const root = tabs.value.find((t) => t.id === rootId)
    return activeId.value !== null && activeId.value === root?.split?.childId ? activeId.value : rootId
  }

  // A split whose connect stopped for a password: the retry from the prompt
  // must land in the same place, not in a tab of its own
  const pendingSplit = new Map<number, { target: SplitTarget; at: number }>()
  const PENDING_SPLIT_MS = 5 * 60_000

  interface SplitTarget {
    parentId: number
    dir: SplitDir
    ratio?: number
  }

  function attachTab(tab: SessionTab, split: SplitTarget | undefined, silent: boolean) {
    const parent = split ? tabs.value.find((t) => t.id === split.parentId && !t.split && !t.parentId) : undefined
    if (parent) {
      tab.parentId = parent.id
      parent.split = { childId: tab.id, dir: split!.dir, ratio: split!.ratio ?? 0.5 }
    }
    tabs.value.push(tab)
    replayBuffered(tab)
    activeId.value = tab.id
    // Silent connections (e.g. started from SFTP) don't hijack the view
    if (!silent) useUiStore().openView(sessionView(parent?.id ?? tab.id))
  }

  /**
   * Open a second pane in this session's tab, right of it or below it, with
   * a fresh session to the same host (or a new local terminal). A tab holds
   * two panes at most.
   */
  function splitPane(sessionId: number, dir: SplitDir) {
    const rootId = rootOf(sessionId)
    const root = tabs.value.find((t) => t.id === rootId)
    if (!root) return
    if (root.split) {
      useUiStore().toast('info', 'This tab is already split in two')
      return
    }
    const target = { parentId: rootId, dir }
    if (root.kind === 'serial') {
      // The port is held by this pane; a second one could not open it
      useUiStore().toast('info', 'A serial port takes one connection at a time')
      return
    }
    if (root.hostId !== null && (root.kind === 'ssh' || root.kind === 'telnet')) {
      void connect(root.hostId, undefined, { split: target })
    } else {
      void spawnLocalTab(
        { kind: root.kind, label: root.label, address: root.address },
        root.kind === 'herdr' ? 'herdr.exe' : undefined,
        target,
      )
    }
  }

  /** Close a whole tab: both panes when it is split */
  async function closeTab(sessionId: number) {
    const rootId = rootOf(sessionId)
    const childId = tabs.value.find((t) => t.id === rootId)?.split?.childId
    if (childId !== undefined) await disconnect(childId)
    await disconnect(rootId)
  }

  /**
   * A fresh session for a pane whose session ended (Reconnect, a reset host
   * key). In a split it comes back as the tab's second pane; on its own it
   * is a plain connect.
   */
  async function reconnectPane(sessionId: number) {
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (!tab || tab.hostId === null) return
    const hostId = tab.hostId
    const root = tab.parentId !== undefined ? tabs.value.find((t) => t.id === tab.parentId) : tab
    const split = root?.split
    if (!root || !split) return connect(hostId)
    // The pane that stays is the new tab's first pane
    const keep = tab.id === root.id ? split.childId : root.id
    const { dir, ratio } = split
    await disconnect(sessionId)
    await connect(hostId, undefined, { split: { parentId: keep, dir, ratio } })
  }

  async function connect(
    hostId: number,
    pendingPassword?: string,
    opts?: { silent?: boolean; split?: SplitTarget },
  ) {
    const hosts = useHostsStore()
    const host = hosts.hostById(hostId)
    if (!host) return
    hosts.touchHost(hostId)

    const pending = pendingPassword !== undefined ? pendingSplit.get(hostId) : undefined
    const split = opts?.split ?? (pending && Date.now() - pending.at < PENDING_SPLIT_MS ? pending.target : undefined)
    pendingSplit.delete(hostId)

    if (!split) {
      // Reconnect: reuse tab for same host if its session ended
      const existing = tabs.value.find(
        (t) => t.hostId === hostId && !t.parentId && !t.split && (t.status === 'closed' || t.status === 'error'),
      )
      if (existing) {
        await disconnect(existing.id)
        tabs.value = tabs.value.filter((t) => t.id !== existing.id)
      }

      const dup = tabs.value.find((t) => t.hostId === hostId)
      if (dup) {
        activeId.value = dup.id
        // Connect / double-click / Enter on a host that already has a tab used to
        // do nothing visible; take the user to that tab (silent callers — SFTP,
        // Forwards — stay where they are)
        if (!opts?.silent) useUiStore().openView(sessionView(rootOf(dup.id)))
        return
      }
    }

    try {
      const sessionId = await api.connect(hostId, pendingPassword ?? undefined)
      const tab: SessionTab = {
        id: sessionId,
        kind: protocolOf(host),
        hostId,
        label: host.label,
        address: hostTarget(host),
        status: 'connecting',
        sftpOpen: false,
      }
      attachTab(tab, split, !!opts?.silent)
    } catch (e) {
      const msg = errorText(e)
      const silent = !!opts?.silent
      // Everything after the first colon is the name: a label may hold colons too
      const promptName = () => msg.slice(msg.indexOf(':') + 1) || host.label
      // Backend asks for credentials at connect time
      if (msg.startsWith('password_required:')) {
        if (split) pendingSplit.set(hostId, { target: split, at: Date.now() })
        useDialogsStore().openPasswordPrompt(hostId, promptName(), 'password', silent)
      } else if (msg.startsWith('passphrase_required:')) {
        if (split) pendingSplit.set(hostId, { target: split, at: Date.now() })
        useDialogsStore().openPasswordPrompt(hostId, promptName(), 'passphrase', silent)
      } else {
        connectFailures.value[hostId] = (connectFailures.value[hostId] ?? 0) + 1
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
    split?: SplitTarget,
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
      attachTab(tab, split, false)
    } catch (e) {
      useUiStore().toast(
        'error',
        `Could not open ${meta.label === 'Local' ? 'local terminal' : meta.label}: ${errorText(e)}`,
      )
    }
  }

  function connectLocal() {
    return spawnLocalTab({ kind: 'local', label: 'Local', address: thisComputerLower })
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

  // ---- auto-reconnect controls (the backend owns the retry loop) ----

  function reconnectNow(sessionId: number) {
    void api.reconnectNow(sessionId).catch(() => {})
  }

  // Stop retrying. The tab stays open (closed state) with a manual
  // Reconnect button, which starts a fresh session
  async function stopReconnecting(sessionId: number) {
    try {
      await api.disconnect(sessionId)
    } catch {
      // already gone
    }
    const tab = tabs.value.find((t) => t.id === sessionId)
    if (tab) {
      tab.status = 'closed'
      tab.reconnect = undefined
      tab.error = 'Connection lost'
    }
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
    const gone = tabs.value.find((t) => t.id === sessionId)
    const ui = useUiStore()
    // Unhook a split: the other pane stays, as a tab of its own if need be
    let survivor: number | null = null
    if (gone?.parentId !== undefined) {
      const parent = tabs.value.find((t) => t.id === gone.parentId)
      if (parent) parent.split = undefined
      survivor = gone.parentId
    } else if (gone?.split) {
      const child = tabs.value.find((t) => t.id === gone.split!.childId)
      if (child) child.parentId = undefined
      survivor = gone.split.childId
    }
    const wasOnScreen = ui.activeView === sessionView(rootOf(sessionId))
    tabs.value = tabs.value.filter((t) => t.id !== sessionId)
    if (survivor !== null && tabs.value.some((t) => t.id === survivor)) {
      if (activeId.value === sessionId) activeId.value = survivor
      if (wasOnScreen) {
        ui.openView(sessionView(survivor))
        // The pane that stays takes the keyboard back
        if (activeId.value === survivor) ui.requestTerminalFocus(survivor)
      }
      return
    }
    if (activeId.value === sessionId) {
      const last = tabs.value.filter((t) => !t.parentId).pop()
      activeId.value = last ? last.id : null
    }
    // If we were looking at this session's page, fall back to the last open
    // session or the hosts dashboard
    if (wasOnScreen) {
      ui.openView(
        activeId.value !== null ? sessionView(rootOf(activeId.value)) : 'hosts',
      )
    }
  }

  function write(sessionId: number, data: string) {
    inputObservers.get(sessionId)?.(data)
    void api.write(sessionId, data).catch(() => {
      // session died; status event will update the tab
    })
  }

  function resize(
    sessionId: number,
    cols: number,
    rows: number,
    pixWidth = 0,
    pixHeight = 0,
  ) {
    void api.resize(sessionId, cols, rows, pixWidth, pixHeight).catch(() => {})
  }

  // ---- transfers ----

  // Progress can arrive before the start command has handed back the id a row
  // is tracked under — a transfer that fails at once (a folder dropped from
  // Explorer, a locked file) reports within microseconds. Keep such events so
  // the row does not sit on "Starting…" forever.
  const earlyTransferEvents = new Map<number, Partial<Transfer>>()
  const MAX_EARLY_TRANSFER_EVENTS = 50

  function trackTransfer(t: Transfer) {
    const early = earlyTransferEvents.get(t.id)
    if (early) {
      earlyTransferEvents.delete(t.id)
      Object.assign(t, early)
    }
    transfers.value.push(t)
  }

  function updateTransfer(transferId: number, patch: Partial<Transfer>) {
    const t = transfers.value.find((x) => x.id === transferId)
    if (t) {
      Object.assign(t, patch)
      return
    }
    earlyTransferEvents.set(transferId, { ...earlyTransferEvents.get(transferId), ...patch })
    if (earlyTransferEvents.size > MAX_EARLY_TRANSFER_EVENTS) {
      const oldest = earlyTransferEvents.keys().next().value
      if (oldest !== undefined) earlyTransferEvents.delete(oldest)
    }
  }

  function removeTransfer(transferId: number) {
    transfers.value = transfers.value.filter((t) => t.id !== transferId)
  }

  async function cancelTransfer(transferId: number) {
    const t = transfers.value.find((x) => x.id === transferId)
    if (!t) return
    try {
      await api.sftpCancelTransfer(t.sessionId, transferId)
    } catch {
      // already finished
    }
  }

  // ---- backend event wiring (called once from main.ts) ----
  async function initEvents() {
    // Keep the computer awake while any live tab runs a command
    watch(
      () => useUiStore().settings.keepAwake && tabs.value.some((t) => t.busy && t.status === 'connected'),
      (on) => void api.setKeepAwake(on).catch(() => {}),
      { immediate: true },
    )

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
      attempt?: number
      maxAttempts?: number
      retryInMs?: number
      reason?: 'exit' | 'lost'
    }>('session:status', (e) => {
      const { sessionId, status, error } = e.payload
      const tab = tabs.value.find((t) => t.id === sessionId)
      if (!tab) {
        statusBuffer.set(sessionId, { status, error })
        return
      }
      const prev = tab.status
      setStatus(sessionId, status, error)

      if (status === 'reconnecting') {
        const { attempt = 1, maxAttempts = 1, retryInMs = 0 } = e.payload
        tab.reconnect = {
          attempt,
          maxAttempts,
          retryAt: retryInMs > 0 ? Date.now() + retryInMs : 0,
        }
        // The banner says it on the visible tab; elsewhere a toast does
        if (prev === 'connected' && !paneVisible(sessionId)) {
          useUiStore().toast('info', `${tab.label}: connection lost — reconnecting…`, { duration: 3000 })
        }
        return
      }
      tab.reconnect = undefined
      if (status === 'connected' && prev === 'reconnecting') {
        useUiStore().toast('success', `Reconnected to ${tab.label}`, { duration: 2500 })
      }
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
      tab.hostKeyAddress = e.payload.address
    })

    await listen<{
      transferId: number
      transferred: number
      size: number
      done: boolean
      error?: string
      filesDone?: number
      filesTotal?: number
      skipped?: number
    }>('transfer:progress', (e) => {
      updateTransfer(e.payload.transferId, {
        transferred: e.payload.transferred,
        size: e.payload.size,
        done: e.payload.done,
        error: e.payload.error,
        filesDone: e.payload.filesDone,
        filesTotal: e.payload.filesTotal,
        skipped: e.payload.skipped,
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
    connectFailures,
    connect,
    connectLocal,
    connectHerdr,
    confirmHostKey,
    reconnectNow,
    stopReconnecting,
    disconnect,
    write,
    resize,
    attachSink,
    detachSink,
    observeInput,
    rootOf,
    paneVisible,
    focusedPane,
    splitPane,
    closeTab,
    reconnectPane,
    setTitle,
    setBusy,
    notifyActivity,
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
