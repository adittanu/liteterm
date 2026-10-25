<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { xtermThemeFor } from '../services/terminalThemes'
import { actionFor } from '../services/shortcuts'
import type { SessionTab } from '../types'

const props = defineProps<{ session: SessionTab }>()

const sessions = useSessionsStore()
const ui = useUiStore()

const container = ref<HTMLElement | null>(null)

let term: Terminal | null = null
let fit: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null

function terminalTheme() {
  return xtermThemeFor(ui.settings.theme, ui.systemDark)
}

// Coalesce keystrokes into ~16ms batches so fast typing costs one invoke per frame
let inputBuf = ''
let flushTimer: number | null = null

function flushInput() {
  flushTimer = null
  if (!inputBuf || !term) return
  const data = inputBuf
  inputBuf = ''
  sessions.write(props.session.id, data)
}

function queueInput(data: string) {
  inputBuf += data
  if (flushTimer === null) {
    flushTimer = window.setTimeout(flushInput, 16)
  }
}

// Kitty keyboard protocol state, tracked from the app's output stream:
// `CSI > flags u` push / `CSI < u` pop / `CSI = flags u` set. While active,
// xterm encodes modifier keys natively. The tail buffer bridges sequences
// split across stdout chunks.
let kittyActive = false
let vtTail = ''
const KITTY_MODE = /\x1b\[(>|<|=)([^u]*)u/g

function scanKitty(data: string) {
  vtTail = (vtTail + data).slice(-32)
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

onMounted(() => {
  if (!container.value) return

  term = new Terminal({
    // Nerd Font variants come after the base family so icon glyphs
    // (oh-my-posh, agent prompts) fall back per-glyph instead of tofu
    fontFamily:
      '"Cascadia Code", "Cascadia Code NF", "Cascadia Mono NF", "JetBrainsMono Nerd Font", "FiraCode Nerd Font", "Hack Nerd Font", ui-monospace, Consolas, monospace',
    fontSize: ui.settings.terminalFontSize,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'bar',
    scrollback: 10000,
    allowTransparency: false,
    theme: terminalTheme(),
    // Lets kitty-aware apps (opencode, Claude Code, …) turn on enhanced
    // keyboard reporting so Shift+Enter & friends arrive disambiguated.
    // win32-input-mode is deliberately left off: modern agents read raw VT
    // input (not console INPUT_RECORDs), and the mode eats kitty
    // negotiation responses.
    vtExtensions: { kittyKeyboard: true },
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  term.open(container.value)

  // App shortcuts are handled by App.vue's capture-phase listener; returning
  // false here keeps xterm from forwarding bound combos into the shell as
  // control bytes (^K, ESC-prefixed Alt sequences, …)
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown') return true
    // Legacy input sends plain \r for Shift+Enter, so apps see it as Enter.
    // - Kitty-negotiated sessions get the native CSI 13;2u encoding.
    // - Other local (ConPTY) sessions: send CSI 13;2u ourselves — agent CLIs
    //   (Claude Code, opencode, pi) parse this sequence even without kitty
    //   negotiation. Same bytes as Windows Terminal's community
    //   sendInput("\u001b[13;2u") keybinding.
    // - SSH hosts without kitty get "\n" (the Ctrl+J newline convention),
    //   which remote shells treat harmlessly.
    if (e.key === 'Enter' && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey) {
      if (!kittyActive) {
        queueInput(props.session.kind === 'local' ? '\x1b[13;2u' : '\n')
        return false
      }
    }
    return actionFor(e, ui.settings) === null
  })

  sessions.attachSink(props.session.id, (data) => {
    // xterm gets raw bytes (it buffers partial UTF-8 itself); the kitty
    // scan runs on the binary string — its patterns are ASCII-only, so
    // multibyte split boundaries can't produce false matches
    scanKitty(atob(data))
    term?.write(b64ToBytes(data))
  })

  term.onData(queueInput)
  term.onResize(({ cols, rows }) => {
    if (props.session.status === 'connected') {
      sessions.resize(props.session.id, cols, rows)
    }
  })

  resizeObserver = new ResizeObserver(() => {
    if (props.session.status === 'connected') {
      try {
        fit?.fit()
      } catch {
        // container hidden — fit() can throw when dimensions are 0
      }
    }
  })
  resizeObserver.observe(container.value)

  if (props.session.status === 'connected') {
    term.focus()
  }

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

// View switch lands here: repaint + focus
watch(
  () => ui.activeView,
  (view) => {
    if (view !== `session:${props.session.id}`) return
    scheduleRecoverPaint()
    term?.focus()
  },
)

watch(
  () => props.session.status,
  (status) => {
    if (status === 'connected') {
      recoverPaint()
      term?.focus()
    }
  },
)

watch(() => ui.settings.terminalFontSize, applyFont)
watch(
  [() => ui.settings.theme, () => ui.systemDark],
  () => {
    if (term) term.options.theme = terminalTheme()
  },
)

onUnmounted(() => {
  document.removeEventListener('visibilitychange', onVisibilityChange)
  sessions.detachSink(props.session.id)
  resizeObserver?.disconnect()
  if (flushTimer !== null) window.clearTimeout(flushTimer)
  term?.dispose()
  term = null
})
</script>

<template>
  <div
    ref="container"
    class="terminal-container"
    :aria-label="`Terminal for ${session.label}`"
  ></div>
</template>

<style scoped>
.terminal-container {
  position: absolute;
  inset: 0;
  padding: 8px 0 0 10px;
}

.terminal-container :deep(.xterm) {
  height: 100%;
}

.terminal-container :deep(.xterm-viewport) {
  background-color: transparent !important;
}
</style>
