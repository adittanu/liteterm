<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { ChevronDown, ChevronUp, X } from 'lucide-vue-next'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { SearchAddon, type ISearchOptions } from '@xterm/addon-search'
import { WebglAddon } from '@xterm/addon-webgl'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { usePrivacyStore } from '../stores/privacy'
import { api } from '../services/api'
import { IMAGE_EXTS, blobToBase64, imageFiles } from '../utils/imageData'
import { useSnippetsStore } from '../stores/extras'
import { nextWord, suggestionFor } from '../services/autosuggest'
import { PromptLine, isPromptText, textBeforeCursor } from '../services/promptLine'
import { osc52Text } from '../services/osc52'
import { terminalThemeFor, xtermThemeFor } from '../services/terminalThemes'
import { actionFor, captureActive, comboFor, comboText } from '../services/shortcuts'
import { isMac, isWindows, terminalClipboardKeys } from '../utils/platform'
import type { SessionTab } from '../types'

const props = defineProps<{ session: SessionTab }>()

const sessions = useSessionsStore()
const ui = useUiStore()
const pv = usePrivacyStore()

const container = ref<HTMLElement | null>(null)

let term: Terminal | null = null
let fit: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let fitFrame = 0
// Chunks that arrive in the same turn become one write. A timer here
// would add echo lag; the backend already coalesces the byte stream.
let chunks: Uint8Array[] = []
let flushQueued = false
let lastUnstick = 0

function scheduleFit() {
  if (fitFrame) return
  fitFrame = requestAnimationFrame(() => {
    fitFrame = 0
    if (!term || !fit) return
    try {
      const dims = fit.proposeDimensions()
      if (!dims || (term.cols === dims.cols && term.rows === dims.rows)) return
      fit.fit()
    } catch {
      // container hidden — fit() can throw when dimensions are 0
    }
  })
}

// OpenCode sizes its splash from the PTY's pixel winsize. A 0×0 report
// makes that art enormous and clips the prompt.
function terminalPixels(): { pixWidth: number; pixHeight: number } {
  if (!term) return { pixWidth: 0, pixHeight: 0 }
  const cell = (
    term as unknown as {
      _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } } } }
    }
  )._core?._renderService?.dimensions?.css?.cell
  if (!cell?.width || !cell?.height) return { pixWidth: 0, pixHeight: 0 }
  const dpr = window.devicePixelRatio || 1
  return {
    pixWidth: Math.round(term.cols * cell.width * dpr),
    pixHeight: Math.round(term.rows * cell.height * dpr),
  }
}

function pushSize(cols: number, rows: number) {
  const px = terminalPixels()
  sessions.resize(props.session.id, cols, rows, px.pixWidth, px.pixHeight)
}

function terminalTheme() {
  return xtermThemeFor(ui.settings.theme, ui.systemDark)
}

// One key can emit more than one sequence. Fold those into a single write
// before paint, but don't wait a frame — a 16ms timer is enough to feel
// the echo lag behind Windows Terminal.
let inputBuf = ''
let inputQueued = false

function flushInput() {
  inputQueued = false
  if (!inputBuf || !term) return
  const data = inputBuf
  inputBuf = ''
  sessions.write(props.session.id, data)
}

function queueInput(data: string) {
  inputBuf += data
  if (inputQueued) return
  inputQueued = true
  queueMicrotask(flushInput)
}

// Kitty keyboard protocol state, tracked from the app's output stream:
// `CSI > flags u` push / `CSI < u` pop / `CSI = flags u` set. While active,
// xterm encodes modifier keys natively. The tail buffer bridges sequences
// split across stdout chunks.
let kittyActive = false
let vtTail = ''
const KITTY_MODE = /\x1b\[(>|<|=)([^u]*)u/g

function scanKitty(bytes: Uint8Array) {
  const start = Math.max(0, bytes.length - 48)
  let piece = ''
  for (let i = start; i < bytes.length; i++) piece += String.fromCharCode(bytes[i])
  vtTail = (vtTail + piece).slice(-48)
  KITTY_MODE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = KITTY_MODE.exec(vtTail))) {
    const flags = m[2].split(';')[0]
    kittyActive = m[1] === '<' ? false : flags !== '' && flags !== '0'
  }
}

function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const bytes = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
  return bytes
}

function applyFont() {
  if (!term) return
  term.options.fontSize = ui.settings.terminalFontSize
  fit?.fit()
}

// ---- find in terminal (Ctrl+Shift+F) ----
// Opening is keyboard-initiated and frequent, so the bar appears instantly.

let search: SearchAddon | null = null
const findOpen = ref(false)
const findQuery = ref('')
const findCase = ref(false)
const findRegex = ref(false)
const findInput = ref<HTMLInputElement | null>(null)
const findResult = ref<{ index: number; count: number } | null>(null)
const findInvalid = ref(false)
const HIGHLIGHT_LIMIT = 1000

const findShortcut = computed(() => comboText(comboFor('terminalFind', ui.settings)))

const findCountLabel = computed(() => {
  if (findInvalid.value) return 'Invalid pattern'
  if (!findQuery.value || !findResult.value) return ''
  const { index, count } = findResult.value
  if (count === 0) return 'No results'
  const total = count >= HIGHLIGHT_LIMIT ? `${HIGHLIGHT_LIMIT}+` : String(count)
  return index >= 0 ? `${index + 1} of ${total}` : `${total} found`
})

// Decorations want #RRGGBB, so blend the palette's yellow into its
// background: quiet tint for every match, stronger for the current one
function mixHex(a: string, b: string, t: number): string {
  const pa = a.replace('#', '').slice(0, 6)
  const pb = b.replace('#', '').slice(0, 6)
  let out = '#'
  for (let i = 0; i < 6; i += 2) {
    const ca = parseInt(pa.slice(i, i + 2), 16)
    const cb = parseInt(pb.slice(i, i + 2), 16)
    out += Math.round(ca + (cb - ca) * t)
      .toString(16)
      .padStart(2, '0')
  }
  return out
}

function searchOptions(incremental: boolean): ISearchOptions {
  const p = terminalThemeFor(ui.settings.theme, ui.systemDark)
  return {
    caseSensitive: findCase.value,
    regex: findRegex.value,
    incremental,
    decorations: {
      matchBackground: mixHex(p.bg, p.yellow, 0.3),
      matchOverviewRuler: mixHex(p.bg, p.yellow, 0.6),
      activeMatchBackground: mixHex(p.bg, p.yellow, 0.65),
      activeMatchColorOverviewRuler: p.yellow,
    },
  }
}

function runFind(direction: 'next' | 'prev', incremental = false) {
  if (!search) return
  if (!findQuery.value) {
    search.clearDecorations()
    findResult.value = null
    findInvalid.value = false
    return
  }
  try {
    findInvalid.value = false
    if (direction === 'next') search.findNext(findQuery.value, searchOptions(incremental))
    else search.findPrevious(findQuery.value, searchOptions(false))
  } catch {
    // an unfinished regex like "foo(" — say so instead of throwing
    findInvalid.value = true
    search.clearDecorations()
  }
}

function openFind() {
  // A single-line selection is the most likely thing to look for
  const sel = term?.getSelection() ?? ''
  if (sel && !sel.includes('\n') && sel.length <= 200) findQuery.value = sel
  findOpen.value = true
  nextTick(() => {
    findInput.value?.focus()
    findInput.value?.select()
    if (findQuery.value) runFind('next', true)
  })
}

function closeFind() {
  findOpen.value = false
  search?.clearDecorations()
  findResult.value = null
  term?.focus()
}

function onFindKey(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    e.preventDefault()
    runFind(e.shiftKey ? 'prev' : 'next')
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    closeFind()
  }
}

watch(findQuery, () => runFind('next', true))
watch([findCase, findRegex], () => runFind('next', true))

watch(
  () => ui.terminalFindRequest,
  (req) => {
    if (req && req.sessionId === props.session.id) openFind()
  },
)

watch(
  () => ui.terminalFocusRequest,
  (req) => {
    if (req && req.sessionId === props.session.id) void nextTick(() => term?.focus())
  },
)

// ---- right-click context menu (Copy / Paste / Select All) ----
// The WebView's own menu is suppressed app-wide (main.ts), so the terminal
// carries its own. While it's open the keyboard belongs to the menu:
// captureActive stands the app shortcuts and every xterm pane down, and a
// capture-phase keydown listener swallows everything the menu doesn't use.

const ctxMenuEl = ref<HTMLElement | null>(null)
const ctxOpen = ref(false)
const ctxX = ref(0)
const ctxY = ref(0)
const ctxHasSelection = ref(false)

function openCtxMenu(e: MouseEvent) {
  const t = e.target as HTMLElement | null
  // Inputs outside the terminal keep the native menu — same rule as main.ts.
  // Inside .xterm the menu takes over even when the target is the hidden
  // helper textarea: right-click moves it under the cursor.
  if (t && !t.closest('.xterm') && t.closest('input, textarea, [contenteditable]')) return
  e.preventDefault()
  e.stopPropagation()
  if (!term || ctxOpen.value) return
  ctxHasSelection.value = term.hasSelection()
  ctxOpen.value = true
  captureActive.value = true
  window.addEventListener('keydown', onCtxKeydown, true)
  window.addEventListener('pointerdown', onCtxPointerDown, true)
  window.addEventListener('blur', closeCtxEvent)
  // Clamp inside the pane once the menu has a size, then move focus in so
  // Shift+F10 users can arrow through the items
  nextTick(() => {
    const el = ctxMenuEl.value
    const pane = el?.parentElement
    if (!el || !pane) return
    const rect = pane.getBoundingClientRect()
    ctxX.value = Math.max(0, Math.min(e.clientX - rect.left, rect.width - el.offsetWidth - 8))
    ctxY.value = Math.max(0, Math.min(e.clientY - rect.top, rect.height - el.offsetHeight - 8))
    el.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus()
  })
}

function closeCtxEvent() {
  closeCtxMenu()
}

function closeCtxMenu(refocus = true) {
  if (!ctxOpen.value) return
  ctxOpen.value = false
  captureActive.value = false
  window.removeEventListener('keydown', onCtxKeydown, true)
  window.removeEventListener('pointerdown', onCtxPointerDown, true)
  window.removeEventListener('blur', closeCtxEvent)
  if (refocus) term?.focus()
}

function onCtxPointerDown(e: PointerEvent) {
  if (!ctxMenuEl.value?.contains(e.target as Node)) closeCtxMenu(false)
}

function onCtxKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' || e.key === 'Tab') {
    e.preventDefault()
    e.stopPropagation()
    closeCtxMenu()
    return
  }
  const items = ctxMenuEl.value
    ? Array.from(ctxMenuEl.value.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
    : []
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopPropagation()
    if (!items.length) return
    const i = items.indexOf(document.activeElement as HTMLButtonElement)
    const next = e.key === 'ArrowDown' ? items[(i + 1) % items.length] : items[(i - 1 + items.length) % items.length]
    next.focus()
    return
  }
  // Let Enter/Space activate the focused item
  if ((e.key === 'Enter' || e.key === ' ') && items.includes(document.activeElement as HTMLButtonElement)) return
  e.preventDefault()
  e.stopPropagation()
}

function ctxCopy() {
  const sel = term?.getSelection() ?? ''
  closeCtxMenu()
  if (!sel) return
  navigator.clipboard.writeText(sel).catch(() => ui.toast('error', 'Could not copy to the clipboard'))
}

async function ctxPaste() {
  closeCtxMenu()
  let text = ''
  try {
    text = await navigator.clipboard.readText()
  } catch {
    ui.toast('error', `Clipboard unavailable — press ${terminalClipboardKeys.paste} to paste`)
    return
  }
  if (!term) return
  if (!text) {
    const image = await clipboardImage()
    if (image) void pasteImage(image)
    return
  }
  pasteText(text)
}

// ---- split panes (from the menu; shortcuts live in App.vue) ----
const inSplit = computed(() => !!props.session.parentId || !!props.session.split)
const splitKeys = computed(() => ({
  right: comboText(comboFor('splitRight', ui.settings)),
  down: comboText(comboFor('splitDown', ui.settings)),
  close: comboText(comboFor('closeTab', ui.settings)),
}))

function ctxSplit(dir: 'right' | 'down') {
  closeCtxMenu(false)
  sessions.splitPane(props.session.id, dir)
}

function ctxClosePane() {
  closeCtxMenu(false)
  void sessions.disconnect(props.session.id)
}

function ctxSelectAll() {
  closeCtxMenu()
  term?.selectAll()
  term?.focus()
}

// WebGL repaints a full-screen TUI on the GPU; the DOM renderer draws text
// through Chromium's own DirectWrite path (closer to Windows Terminal's
// glyph weight) but costs more CPU. Disposing the addon falls back to DOM.
let webgl: WebglAddon | null = null

function applyRenderer() {
  if (!term) return
  const want = ui.terminalRender.webgl
  if (want && !webgl) {
    try {
      const addon = new WebglAddon()
      addon.onContextLoss(() => {
        addon.dispose()
        if (webgl === addon) webgl = null
      })
      term.loadAddon(addon)
      webgl = addon
    } catch {
      // no WebGL — DOM renderer stays in place
    }
  } else if (!want && webgl) {
    webgl.dispose()
    webgl = null
  }
  term.refresh(0, term.rows - 1)
}

function unstickPaint() {
  if (!term) return
  const now = performance.now()
  if (now - lastUnstick < 100) return
  lastUnstick = now
  const renderService = (
    term as unknown as { _core?: { _renderService?: RenderServiceLike } }
  )._core?._renderService
  try {
    renderService?._renderDebouncer?.dispose?.()
  } catch {
    // private API drift
  }
  try {
    renderService?.refreshRows?.(0, term.rows - 1, true)
  } catch {
    // private API drift
  }
}

function flushScreen() {
  flushQueued = false
  if (!term || chunks.length === 0) {
    chunks = []
    return
  }
  const parts = chunks
  chunks = []
  let n = 0
  for (const part of parts) n += part.length
  const out = new Uint8Array(n)
  let offset = 0
  for (const part of parts) {
    out.set(part, offset)
    offset += part.length
  }
  term.write(out, () => {
    if (n >= 2048) unstickPaint()
    // The screen now shows what the shell sent: a new prompt, an echoed key
    prompt?.track()
    refreshGhost()
  })
}

// ---- command activity: notice when done (F), keep awake while busy (AE) ----
// No shell integration is assumed. A command counts from an Enter pressed
// while the shell sat at its prompt (output went quiet with the cursor right
// after a prompt sign) until the prompt shows again. Commands started any
// other way (a custom prompt we don't recognise) are simply not tracked.
const QUIET_MS = 1200
const LONG_COMMAND_MS = 10_000
// A command that stops printing for this long no longer counts as busy:
// shorter for full-screen programs (an idle opencode), long for a quiet build
const IDLE_MS = { normal: 30 * 60_000, alternate: 2 * 60_000 }
let commandStartedAt = 0
let lastOutputAt = 0
let quietTimer = 0
let busyTimer = 0
// The command line being typed (F, autosuggest); set up once the terminal exists
let prompt: PromptLine | null = null

function updateBusy() {
  clearTimeout(busyTimer)
  busyTimer = 0
  let busy = false
  if (commandStartedAt && term) {
    const left = IDLE_MS[term.buffer.active.type] - (Date.now() - lastOutputAt)
    busy = left > 0
    if (busy) busyTimer = window.setTimeout(updateBusy, left + 50)
  }
  sessions.setBusy(props.session.id, busy)
}

function onQuiet() {
  quietTimer = 0
  if (!term || term.buffer.active.type !== 'normal') return
  if (!isPromptText(textBeforeCursor(term))) return
  if (!commandStartedAt) return
  const ms = lastOutputAt - commandStartedAt
  commandStartedAt = 0
  updateBusy()
  if (ms >= LONG_COMMAND_MS) sessions.notifyActivity(props.session.id, 'done', ms)
}

function onOutputActivity() {
  lastOutputAt = Date.now()
  clearTimeout(quietTimer)
  quietTimer = window.setTimeout(onQuiet, QUIET_MS)
  if (commandStartedAt && !busyTimer) updateBusy()
}

function onSessionInput(data: string) {
  if (!term || term.buffer.active.type !== 'normal' || !/[\r\n]/.test(data)) return
  prompt?.track()
  if (!prompt?.active) return
  // The Enter may travel with keys the shell hasn't echoed yet (a paste,
  // an accepted suggestion): add those to what the screen shows
  const typed = prompt.current()
  const head = data.split(/[\r\n]/, 1)[0]
  if (typed && !data.includes('\x1b')) recordCommand(typed.text + head)
  prompt.clear()
  hideGhost()
  commandStartedAt = Date.now()
  lastOutputAt = commandStartedAt
  updateBusy()
}

function resetActivity() {
  prompt?.clear()
  hideGhost()
  commandStartedAt = 0
  clearTimeout(quietTimer)
  quietTimer = 0
  updateBusy()
}

// ---- suggestions while typing, from this host's history (fish-style) ----
// Drawn by LiteTerm over the terminal, never sent: → or End takes it all,
// Alt+→ (Ctrl+→) one word. Programs in the alternate screen get none.
let history: string[] = []
const hostIdForHistory = props.session.kind === 'herdr' ? undefined : props.session.hostId
const snippetsStore = useSnippetsStore()
const paneEl = ref<HTMLElement | null>(null)
const ghost = ref<{ full: string; chars: string[]; left: number; top: number; cellW: number; cellH: number } | null>(null)

function hideGhost() {
  if (ghost.value) ghost.value = null
}

async function loadHistory() {
  if (hostIdForHistory === undefined) return
  try {
    history = await api.historyList(hostIdForHistory)
  } catch {
    history = []
  }
  refreshGhost()
}

function recordCommand(raw: string) {
  if (hostIdForHistory === undefined || !ui.settings.autosuggest) return
  const cmd = raw.trimEnd()
  // A leading space keeps a command out of history, as in bash and zsh
  if (!cmd.trim() || cmd.startsWith(' ') || cmd.length > 1000) return
  history = [cmd, ...history.filter((c) => c !== cmd)].slice(0, 2000)
  void api.historyAdd(hostIdForHistory, cmd).catch(() => {})
}

function cellCss(): { width: number; height: number } | null {
  const cell = (
    term as unknown as {
      _core?: { _renderService?: { dimensions?: { css?: { cell?: { width: number; height: number } } } } }
    } | null
  )?._core?._renderService?.dimensions?.css?.cell
  return cell?.width && cell?.height ? { width: cell.width, height: cell.height } : null
}

function refreshGhost() {
  const next = computeGhost()
  if (next || ghost.value) ghost.value = next
}

function computeGhost(): typeof ghost.value {
  if (!term || !ui.settings.autosuggest || hostIdForHistory === undefined || !prompt?.active) return null
  const buf = term.buffer.active
  // Scrolled back: the cursor's row isn't on screen
  if (buf.type !== 'normal' || buf.viewportY !== buf.baseY) return null
  const input = prompt.current()
  if (!input?.free || input.room <= 0) return null
  const snippetCommands = snippetsStore.snippets.map((s) => s.command)
  const full = suggestionFor(input.text, history, snippetCommands)
  if (!full) return null
  const cell = cellCss()
  const screen = term.element?.querySelector('.xterm-screen')
  if (!cell || !screen || !paneEl.value) return null
  const s = screen.getBoundingClientRect()
  const p = paneEl.value.getBoundingClientRect()
  return {
    full,
    chars: Array.from(full).slice(0, input.room),
    left: s.left - p.left + buf.cursorX * cell.width,
    top: s.top - p.top + buf.cursorY * cell.height,
    cellW: cell.width,
    cellH: cell.height,
  }
}

function acceptGhost(oneWord: boolean): boolean {
  const g = ghost.value
  if (!g) return false
  hideGhost()
  queueInput(oneWord ? nextWord(g.full) : g.full)
  return true
}

const ghostStyle = computed(() => {
  const g = ghost.value
  if (!g) return {}
  const palette = terminalThemeFor(ui.settings.theme, ui.systemDark)
  return {
    left: `${g.left}px`,
    top: `${g.top}px`,
    height: `${g.cellH}px`,
    lineHeight: `${g.cellH}px`,
    fontFamily: term?.options.fontFamily,
    fontSize: `${ui.settings.terminalFontSize}px`,
    color: palette.brightBlack,
    '--cell-w': `${g.cellW}px`,
  }
})

watch(
  () => ui.historyEpoch,
  () => void loadHistory(),
)
watch(
  () => ui.settings.autosuggest,
  () => refreshGhost(),
)

// The server's own shell history, read once per host on the first connect
let importAsked = false
function maybeImportHistory() {
  if (importAsked || props.session.kind !== 'ssh' || hostIdForHistory == null) return
  if (!ui.settings.autosuggest || !ui.settings.autosuggestImport) return
  importAsked = true
  void api
    .historyImport(props.session.id)
    .then((n) => {
      if (n) void loadHistory()
    })
    .catch(() => {})
}

// ---- pasting an image (AA) ----
// A screenshot on the clipboard can't travel as keystrokes. It is saved
// where this tab's programs can read it (the server's /tmp for SSH) and its
// path is pasted instead, which is how opencode and friends take an image.
const MAX_PASTE_IMAGE = 25 * 1024 * 1024
let pastingImage = false

function pasteText(text: string) {
  if (!term) return
  // A real paste event, so xterm applies newline normalization and
  // bracketed-paste wrapping (vim, zsh, agent CLIs, …)
  const dt = new DataTransfer()
  dt.setData('text/plain', text)
  term.textarea?.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt }))
  term.focus()
}

function quotePath(path: string): string {
  if (!/[\s'"()&;$`]/.test(path)) return path
  if ((props.session.kind === 'local' || props.session.kind === 'herdr') && isWindows) return `"${path}"`
  return `'${path.replace(/'/g, `'\\''`)}'`
}

async function pasteImage(image: Blob) {
  const ext = IMAGE_EXTS[image.type]
  if (!ext || pastingImage) return
  if (image.size > MAX_PASTE_IMAGE) {
    ui.toast('error', 'That image is larger than 25 MB')
    return
  }
  // No file transfer over Telnet or a serial line: there is nowhere to put it
  if (props.session.kind === 'telnet' || props.session.kind === 'serial') {
    ui.toast('info', `Images can't be pasted into a ${props.session.kind === 'telnet' ? 'Telnet' : 'serial'} session`)
    return
  }
  pastingImage = true
  const remote = props.session.kind === 'ssh'
  const toastId = remote ? ui.toast('info', `Uploading the image to ${props.session.label}…`, { duration: 0 }) : 0
  try {
    const path = await api.pasteImage(props.session.id, await blobToBase64(image), ext)
    pasteText(quotePath(path))
  } catch (e) {
    ui.toast('error', `Could not paste the image: ${errorText(e)}`)
  } finally {
    if (toastId) ui.dismissToast(toastId)
    pastingImage = false
  }
}

// Capture phase on the pane, ahead of xterm's own paste handler on its textarea
function onPasteCapture(e: ClipboardEvent) {
  const dt = e.clipboardData
  // Text wins: copying from a page can carry an image alongside it
  if (!dt || dt.getData('text/plain')) return
  const [image] = imageFiles(dt.items)
  if (!image) return
  e.preventDefault()
  e.stopImmediatePropagation()
  void pasteImage(image)
}

async function clipboardImage(): Promise<Blob | null> {
  try {
    for (const item of await navigator.clipboard.read()) {
      const type = item.types.find((t) => IMAGE_EXTS[t])
      if (type) return await item.getType(type)
    }
  } catch {
    // no image, or the web view won't say
  }
  return null
}

onMounted(() => {
  if (!container.value) return

  term = new Terminal({
    // Nerd Font variants come after the base family so icon glyphs
    // (oh-my-posh, agent prompts) fall back per-glyph instead of tofu
    // Same face as this machine's Windows Terminal defaults (MesloLGM Nerd Font Mono).
    fontFamily:
      '"MesloLGM Nerd Font Mono", "Cascadia Code", "Cascadia Code NF", "Cascadia Mono NF", "JetBrainsMono Nerd Font", "FiraCode Nerd Font", "Hack Nerd Font", ui-monospace, Consolas, Menlo, "DejaVu Sans Mono", "Liberation Mono", monospace',
    fontSize: ui.settings.terminalFontSize,
    fontWeight: 'normal',
    // Windows Terminal's documented default intenseTextStyle is "bright"
    // (regular weight), but on this machine WT visibly draws SGR bold heavy.
    // Toggleable from Settings → Terminal while comparing.
    fontWeightBold: ui.terminalRender.realBold ? 'bold' : 'normal',
    // 1.0 keeps block-character art (opencode's splash, box drawing) flush.
    // A taller line height opens a gap between rows and the mark looks sliced.
    lineHeight: 1,
    cursorBlink: true,
    cursorStyle: 'bar',
    // Windows xterm's default replaces the current selection with the word
    // under the cursor on right-click; the context menu needs the real
    // selection kept alive so its Copy entry copies what the user dragged
    rightClickSelectsWord: false,
    scrollback: 10000,
    // Instant scroll. A non-zero duration animates the viewport and reads as lag.
    smoothScrollDuration: 0,
    // One Half Dark uses the same yellow for the bright slot. Promoting bold
    // to that slot is a no-op here, and a bright ramp would lighten the gold.
    drawBoldTextInBrightColors: false,
    minimumContrastRatio: 1,
    allowTransparency: false,
    theme: terminalTheme(),
    // Lets kitty-aware apps (opencode, Claude Code, …) turn on enhanced
    // keyboard reporting so Shift+Enter & friends arrive disambiguated.
    // win32-input-mode is deliberately left off: modern agents read raw VT
    // input (not console INPUT_RECORDs), and the mode eats kitty
    // negotiation responses.
    vtExtensions: { kittyKeyboard: true },
  })
  term.options.theme = terminalTheme()
  prompt = new PromptLine(term)
  fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  search = new SearchAddon({ highlightLimit: HIGHLIGHT_LIMIT })
  search.onDidChangeResults(({ resultIndex, resultCount }) => {
    findResult.value = { index: resultIndex, count: resultCount }
  })
  term.loadAddon(search)
  term.open(container.value)
  applyRenderer()

  // OSC 52 clipboard writes. A TUI that captures the mouse (opencode,
  // Claude Code, Neovim) gets the drag instead of xterm's selection, and
  // copies by sending this sequence. Without a handler the copy is dropped
  // and the app's "Copied to clipboard" notice is a lie.
  term.parser.registerOscHandler(52, (data) => {
    const text = osc52Text(data)
    if (text !== null) {
      api.clipboardWriteText(text).catch(() => ui.toast('error', 'Could not copy to the clipboard'))
    }
    return true
  })

  // App shortcuts are handled by App.vue's capture-phase listener; returning
  // false here keeps xterm from forwarding bound combos into the shell as
  // control bytes (^K, ESC-prefixed Alt sequences, …)
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown') return true
    // While rebinding a shortcut in Settings, no key may reach the shell
    if (captureActive.value) return false
    // A suggestion on screen: → / End take it, Alt+→ or Ctrl+→ one word
    // (Option+→ / Cmd+→ on a Mac). With none shown the keys work as usual.
    if (ghost.value && (e.key === 'ArrowRight' || e.key === 'End')) {
      const plain = !e.shiftKey && !e.altKey && !e.ctrlKey && !e.metaKey
      const word = e.key === 'ArrowRight' && !e.shiftKey && !e.metaKey && (e.altKey !== e.ctrlKey)
      const all = plain || (isMac && e.key === 'ArrowRight' && e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey)
      if ((word || all) && acceptGhost(word)) return false
    }
    // Legacy input sends plain \r for Shift+Enter, so apps see it as Enter.
    // - Kitty-negotiated sessions get the native CSI 13;2u encoding.
    // - Other local (ConPTY) sessions — local shell and herdr: send
    //   CSI 13;2u ourselves — agent CLIs (Claude Code, opencode, pi) parse
    //   this sequence even without kitty negotiation. Same bytes as Windows
    //   Terminal's community sendInput("\u001b[13;2u") keybinding.
    // - SSH hosts without kitty get "\n" (the Ctrl+J newline convention),
    //   which remote shells treat harmlessly.
    if (e.key === 'Enter' && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (!kittyActive) {
        queueInput(props.session.kind === 'local' || props.session.kind === 'herdr' ? '\x1b[13;2u' : '\n')
        return false
      }
    }
    // Ctrl+C / Ctrl+V: xterm maps these to ^C/^V and preventDefault()s the
    // keydown, which cancels the native clipboard events its own copy/paste
    // handlers listen for. Returning false skips that so the browser fires
    // them — paste lands through xterm's handler (newline-normalized,
    // bracketed-paste aware) and copy writes the selection. Without a
    // selection Ctrl+C stays a plain ^C so it can still interrupt a job.
    // macOS skips this: Cmd+C / Cmd+V arrive as native copy and paste events
    // through the Edit menu, and Ctrl+C is always ^C there — a selection must
    // never turn an interrupt into a copy.
    if (!isMac && e.ctrlKey && !e.shiftKey && !e.altKey && !e.metaKey) {
      if (e.key === 'c' || e.key === 'C') return !term?.hasSelection()
      if (e.key === 'v' || e.key === 'V') return false
    }
    // Linux terminals (GNOME Terminal, Konsole) copy and paste with Ctrl+Shift+C / V
    if (!isWindows && !isMac && e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey) {
      if (e.code === 'KeyC') {
        ctxCopy()
        return false
      }
      if (e.code === 'KeyV') {
        void ctxPaste()
        return false
      }
    }
    return actionFor(e, ui.settings) === null
  })

  sessions.attachSink(props.session.id, (data) => {
    // xterm gets raw bytes (it buffers partial UTF-8 itself); the kitty
    // scan only looks at the ASCII tail, so a multibyte split can't match
    const bytes = b64ToBytes(data)
    scanKitty(bytes)
    chunks.push(bytes)
    onOutputActivity()
    if (flushQueued) return
    flushQueued = true
    queueMicrotask(flushScreen)
  })

  sessions.observeInput(props.session.id, onSessionInput)
  term.onBell(() => sessions.notifyActivity(props.session.id, 'bell'))
  term.onTitleChange((title) => sessions.setTitle(props.session.id, title))
  term.onScroll(() => refreshGhost())
  container.value.addEventListener('paste', onPasteCapture, true)
  void loadHistory()
  if (props.session.status === 'connected') maybeImportHistory()

  term.onData(queueInput)
  term.onResize(({ cols, rows }) => {
    if (props.session.status === 'connected') pushSize(cols, rows)
    refreshGhost()
  })

  resizeObserver = new ResizeObserver(() => {
    if (props.session.status === 'connected') scheduleFit()
  })
  resizeObserver.observe(container.value)

  if (props.session.status === 'connected' && hasKeyboard()) {
    term.focus()
  }
  // In a split, clicking a pane (or tabbing into it) gives it the keyboard
  term.textarea?.addEventListener('focus', onTermFocus)

  // Long occlusion/minimize can strand a debounced paint; repaint once the
  // window becomes visible again
  document.addEventListener('visibilitychange', onVisibilityChange)
})

// ---- blank-pane recovery ----
// xterm pauses rendering while a pane is hidden (IntersectionObserver) and
// debounces paints on rAF. If the WebView freezes rAF for a long time
// (occluded or minimized window), a pending frame can be dropped while the
// debouncer still holds its id — every later refresh() early-returns and the
// pane stops painting, so rows emptied by an alt-screen switch or a resize
// stay blank. On (re)show: cancel any stale frame, refit, then repaint the
// whole viewport synchronously from the buffer.
type RenderServiceLike = {
  _renderDebouncer?: { dispose?: () => void }
  refreshRows?: (start: number, end: number, sync?: boolean) => void
}

function recoverPaint() {
  if (!term) return
  try {
    fit?.fit()
  } catch {
    // container hidden — fit() can throw when dimensions are 0
  }
  const renderService = (
    term as unknown as { _core?: { _renderService?: RenderServiceLike } }
  )._core?._renderService
  try {
    renderService?._renderDebouncer?.dispose?.()
  } catch {
    // private API drift — the sync repaint below still helps
  }
  try {
    renderService?.refreshRows?.(0, term.rows - 1, true)
  } catch {
    // private API drift — the IntersectionObserver refresh still applies
  }
}

// The render service unpauses via IntersectionObserver only after the first
// post-display layout pass; a second rAF lands safely past that delivery.
function scheduleRecoverPaint() {
  requestAnimationFrame(() => requestAnimationFrame(recoverPaint))
}

function onVisibilityChange() {
  if (document.visibilityState === 'visible') recoverPaint()
}

// View switch lands here: repaint + focus. In a split both panes repaint;
// only the one that had the keyboard takes it back.
watch(
  () => ui.activeView,
  () => {
    if (!sessions.paneVisible(props.session.id)) return
    closeCtxMenu(false)
    scheduleRecoverPaint()
    if (hasKeyboard()) term?.focus()
  },
)

/** This pane is on screen and is the one typing goes to */
function hasKeyboard(): boolean {
  const id = props.session.id
  return sessions.paneVisible(id) && sessions.focusedPane(sessions.rootOf(id)) === id
}

function onTermFocus() {
  if (sessions.activeId !== props.session.id) sessions.activeId = props.session.id
}

// A split pane appears or goes: both panes change size and redraw
watch(
  () => !!props.session.parentId || !!props.session.split,
  () => scheduleRecoverPaint(),
)

// Reconnect: the backend redials on the same session, so this terminal and
// its scrollback survive. The old shell is gone — undo whatever modes it
// left on (alt screen, mouse reporting, hidden cursor, kitty keys) and mark
// the seam so old output isn't mistaken for the new shell's.
const RESET_MODES =
  '\x1b[?1049l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[=0u\x1b[?25h\x1b[0m'
let lostConnection = false

watch(
  () => props.session.status,
  (status, prev) => {
    if (status === 'reconnecting' && prev === 'connected' && term) {
      lostConnection = true
      kittyActive = false
      // The shell that ran the command is gone, and so is its title
      resetActivity()
      sessions.setTitle(props.session.id, '')
      term.write(`${RESET_MODES}\r\n\x1b[2m── connection lost ──\x1b[0m\r\n`)
    }
    if (status === 'connected') {
      maybeImportHistory()
      if (lostConnection && term) {
        lostConnection = false
        term.write('\x1b[2m── reconnected ──\x1b[0m\r\n')
      }
      recoverPaint()
      // A fresh PTY starts at 80×24; tell it the real size even when the
      // fit didn't change (reconnects keep this pane's dimensions)
      if (term) pushSize(term.cols, term.rows)
      if (!findOpen.value && hasKeyboard()) term?.focus()
    }
  },
)

watch(() => ui.settings.terminalFontSize, applyFont)
watch(() => ui.terminalRender.webgl, applyRenderer)
watch(
  () => ui.terminalRender.realBold,
  (bold) => {
    if (term) term.options.fontWeightBold = bold ? 'bold' : 'normal'
  },
)
watch(
  [() => ui.settings.theme, () => ui.systemDark],
  () => {
    if (term) term.options.theme = terminalTheme()
  },
  { immediate: true },
)

onUnmounted(() => {
  document.removeEventListener('visibilitychange', onVisibilityChange)
  term?.textarea?.removeEventListener('focus', onTermFocus)
  container.value?.removeEventListener('paste', onPasteCapture, true)
  clearTimeout(quietTimer)
  clearTimeout(busyTimer)
  prompt?.clear()
  closeCtxMenu(false)
  if (fitFrame) cancelAnimationFrame(fitFrame)
  chunks = []
  sessions.detachSink(props.session.id)
  resizeObserver?.disconnect()
  webgl = null
  search = null
  term?.dispose()
  term = null
})
</script>

<template>
  <div ref="paneEl" class="terminal-pane" @contextmenu="openCtxMenu">
    <div
      ref="container"
      class="terminal-container"
      :aria-label="pv.t(`Terminal for ${session.label}`)"
    ></div>

    <!-- Suggestion from history, drawn over the cell grid after the cursor;
         one span per cell so it lines up with xterm's own glyphs -->
    <div v-if="ghost" class="ghost" :style="ghostStyle" aria-hidden="true"><span v-for="(ch, i) in ghost.chars" :key="i">{{ ch }}</span></div>

    <!-- Find bar: floats over the top-right of the output, like Windows Terminal -->
    <div v-if="findOpen" class="find-bar" role="search" aria-label="Find in terminal">
      <input
        ref="findInput"
        v-model="findQuery"
        class="find-input mono"
        :class="{ invalid: findInvalid }"
        type="text"
        placeholder="Find"
        aria-label="Find in terminal"
        spellcheck="false"
        autocomplete="off"
        @keydown="onFindKey"
      />
      <span class="find-count" aria-live="polite">{{ findCountLabel }}</span>
      <button
        class="find-opt"
        :class="{ on: findCase }"
        :aria-pressed="findCase"
        title="Match case"
        aria-label="Match case"
        @click="findCase = !findCase"
      >
        Aa
      </button>
      <button
        class="find-opt mono"
        :class="{ on: findRegex }"
        :aria-pressed="findRegex"
        title="Use regular expression"
        aria-label="Use regular expression"
        @click="findRegex = !findRegex"
      >
        .*
      </button>
      <span class="find-sep" aria-hidden="true"></span>
      <button
        class="btn-icon find-btn"
        title="Previous match (Shift+Enter)"
        aria-label="Previous match"
        :disabled="!findQuery"
        @click="runFind('prev')"
      >
        <ChevronUp :size="14" :stroke-width="1.75" aria-hidden="true" />
      </button>
      <button
        class="btn-icon find-btn"
        title="Next match (Enter)"
        aria-label="Next match"
        :disabled="!findQuery"
        @click="runFind('next')"
      >
        <ChevronDown :size="14" :stroke-width="1.75" aria-hidden="true" />
      </button>
      <button
        class="btn-icon find-btn"
        :title="`Close (Esc) — reopen with ${findShortcut}`"
        aria-label="Close find"
        @click="closeFind"
      >
        <X :size="13" :stroke-width="1.75" aria-hidden="true" />
      </button>
    </div>

    <!-- Right-click menu: Copy / Paste / Select All -->
    <div
      v-if="ctxOpen"
      ref="ctxMenuEl"
      class="ctx-menu"
      role="menu"
      aria-label="Terminal clipboard"
      :style="{ left: `${ctxX}px`, top: `${ctxY}px` }"
      @contextmenu.prevent.stop
    >
      <button class="ctx-item" role="menuitem" :disabled="!ctxHasSelection" @click="ctxCopy">
        <span>Copy</span><kbd>{{ terminalClipboardKeys.copy }}</kbd>
      </button>
      <button class="ctx-item" role="menuitem" @click="ctxPaste">
        <span>Paste</span><kbd>{{ terminalClipboardKeys.paste }}</kbd>
      </button>
      <div class="ctx-sep" role="separator" aria-hidden="true"></div>
      <button class="ctx-item" role="menuitem" @click="ctxSelectAll">
        <span>Select All</span>
      </button>
      <div class="ctx-sep" role="separator" aria-hidden="true"></div>
      <template v-if="!inSplit">
        <button class="ctx-item" role="menuitem" @click="ctxSplit('right')">
          <span>Split right</span><kbd v-if="splitKeys.right">{{ splitKeys.right }}</kbd>
        </button>
        <button class="ctx-item" role="menuitem" @click="ctxSplit('down')">
          <span>Split down</span><kbd v-if="splitKeys.down">{{ splitKeys.down }}</kbd>
        </button>
      </template>
      <button v-else class="ctx-item" role="menuitem" @click="ctxClosePane">
        <span>Close pane</span><kbd v-if="splitKeys.close">{{ splitKeys.close }}</kbd>
      </button>
    </div>
  </div>
</template>

<style scoped>
.terminal-pane {
  position: absolute;
  inset: 0;
}

.ghost {
  position: absolute;
  z-index: 3;
  display: flex;
  white-space: pre;
  pointer-events: none;
  user-select: none;
  font-weight: normal;
}

.ghost span {
  display: inline-block;
  width: var(--cell-w);
  overflow: visible;
}

.find-bar {
  position: absolute;
  top: 8px;
  right: 18px;
  z-index: 3;
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 4px 4px 4px 6px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-strong);
  background: var(--bg-surface);
  box-shadow: var(--shadow-pop);
}

.find-input {
  width: 200px;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  padding: 4px 8px;
  font-size: var(--text-sm);
  color: var(--text-1);
  outline: none;
}

.find-input:focus {
  border-color: var(--accent);
}

.find-input.invalid {
  border-color: var(--danger);
}

.find-count {
  min-width: 72px;
  padding: 0 6px;
  font-size: var(--text-xs);
  color: var(--text-3);
  text-align: right;
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}

.find-opt {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 24px;
  min-width: 26px;
  padding: 0 5px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  font-weight: 600;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.find-opt:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.find-opt.on {
  background: var(--accent-bg);
  color: var(--accent);
}

.find-sep {
  width: 1px;
  height: 16px;
  margin: 0 3px;
  background: var(--border);
}

.find-btn {
  width: 24px;
  height: 24px;
}

.find-btn:disabled {
  opacity: 0.4;
  pointer-events: none;
}

.ctx-menu {
  position: absolute;
  z-index: 30;
  min-width: 176px;
  padding: 4px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-strong);
  background: var(--bg-surface);
  box-shadow: var(--shadow-pop);
  user-select: none;
  transform-origin: top left;
  animation: ctx-in 100ms cubic-bezier(0.2, 0.9, 0.3, 1);
}

@keyframes ctx-in {
  from {
    opacity: 0;
    transform: scale(0.96);
  }
}

.ctx-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  width: 100%;
  padding: 6px 10px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  font: inherit;
  font-size: var(--text-sm);
  color: var(--text-1);
  text-align: left;
  cursor: pointer;
  transition: background-color var(--dur-hover) ease;
}

.ctx-item:hover:not(:disabled),
.ctx-item:focus-visible {
  background: var(--bg-hover);
  outline: none;
}

.ctx-item:disabled {
  color: var(--text-3);
  opacity: 0.55;
  cursor: default;
}

.ctx-item kbd {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--text-3);
}

.ctx-sep {
  height: 1px;
  margin: 4px 6px;
  background: var(--border);
}

.terminal-container {
  position: absolute;
  inset: 0;
  padding: 8px 0 0 10px;
  overflow: hidden;
}

.terminal-container :deep(.xterm) {
  height: 100%;
}

.terminal-container :deep(.xterm-viewport) {
  background-color: transparent !important;
  /* The app-wide scrollbar uses a transparent border and background-clip.
     On this overlay that disables fast scrolling in WebView2 and each
     wheel notch waits on the main thread. A plain thumb does not. */
  overscroll-behavior: contain;
  overflow-anchor: none;
  scrollbar-width: thin;
  scrollbar-color: var(--border-strong) transparent;
}

.terminal-container :deep(.xterm-viewport)::-webkit-scrollbar {
  width: 8px;
  background: transparent;
}

.terminal-container :deep(.xterm-viewport)::-webkit-scrollbar-track {
  background: transparent;
}

.terminal-container :deep(.xterm-viewport)::-webkit-scrollbar-thumb {
  background: var(--border-strong);
  border: none;
  border-radius: 4px;
}
</style>
