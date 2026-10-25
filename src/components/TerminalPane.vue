<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebglAddon } from '@xterm/addon-webgl'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { xtermThemeFor } from '../services/terminalThemes'
import { actionFor, captureActive } from '../services/shortcuts'
import type { SessionTab } from '../types'

const props = defineProps<{ session: SessionTab }>()

const sessions = useSessionsStore()
const ui = useUiStore()

const container = ref<HTMLElement | null>(null)

let term: Terminal | null = null
let fit: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null
let fitFrame = 0

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

onMounted(() => {
  if (!container.value) return

  term = new Terminal({
    // Nerd Font variants come after the base family so icon glyphs
    // (oh-my-posh, agent prompts) fall back per-glyph instead of tofu
    // Same face as this machine's Windows Terminal defaults (MesloLGM Nerd Font Mono).
    fontFamily:
      '"MesloLGM Nerd Font Mono", "Cascadia Code", "Cascadia Code NF", "Cascadia Mono NF", "JetBrainsMono Nerd Font", "FiraCode Nerd Font", "Hack Nerd Font", ui-monospace, Consolas, monospace',
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
  fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  term.open(container.value)
  applyRenderer()

  // App shortcuts are handled by App.vue's capture-phase listener; returning
  // false here keeps xterm from forwarding bound combos into the shell as
  // control bytes (^K, ESC-prefixed Alt sequences, …)
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== 'keydown') return true
    // While rebinding a shortcut in Settings, no key may reach the shell
    if (captureActive.value) return false
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
        queueInput(props.session.kind === 'ssh' ? '\n' : '\x1b[13;2u')
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
    term?.write(bytes)
  })

  term.onData(queueInput)
  term.onResize(({ cols, rows }) => {
    if (props.session.status === 'connected') {
      sessions.resize(props.session.id, cols, rows)
    }
  })

  resizeObserver = new ResizeObserver(() => {
    if (props.session.status === 'connected') scheduleFit()
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
  if (fitFrame) cancelAnimationFrame(fitFrame)
  sessions.detachSink(props.session.id)
  resizeObserver?.disconnect()
  webgl = null
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
