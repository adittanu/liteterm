<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { terminalThemeFor } from '../services/terminalThemes'
import type { SessionTab } from '../types'

const props = defineProps<{ session: SessionTab }>()

const sessions = useSessionsStore()
const ui = useUiStore()

const container = ref<HTMLElement | null>(null)

let term: Terminal | null = null
let fit: FitAddon | null = null
let resizeObserver: ResizeObserver | null = null

function terminalTheme() {
  return terminalThemeFor(ui.settings.theme, ui.systemDark)
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
    fontFamily: '"Cascadia Code", ui-monospace, Consolas, monospace',
    fontSize: ui.settings.terminalFontSize,
    lineHeight: 1.2,
    cursorBlink: true,
    cursorStyle: 'bar',
    scrollback: 10000,
    allowTransparency: false,
    theme: terminalTheme(),
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new WebLinksAddon())
  term.open(container.value)

  sessions.attachSink(props.session.id, (data) => {
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
})

// View switch lands here: focus without animation
watch(
  () => ui.activeView,
  (view) => {
    if (view === `session:${props.session.id}`) term?.focus()
  },
)

watch(
  () => props.session.status,
  (status) => {
    if (status === 'connected') {
      try {
        fit?.fit()
      } catch {
        /* ignore */
      }
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
