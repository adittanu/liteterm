<script setup lang="ts">
// Tiptap node view for code blocks in notes: the block stays plain,
// editable text; a small action strip adds Copy and — for shell-like
// blocks — Run in a connected terminal.
import { computed, inject, ref } from 'vue'
import { NodeViewContent, NodeViewWrapper, nodeViewProps } from '@tiptap/vue-3'
import { Check, Copy, Play } from 'lucide-vue-next'
import { useSessionsStore } from '../stores/sessions'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { pickRunTarget, stripPrompts } from '../utils/runTarget'
import { waitForConnection } from '../utils/sessionWait'
import { NOTE_HOST_ID } from './noteContext'

const props = defineProps(nodeViewProps)

const sessions = useSessionsStore()
const hosts = useHostsStore()
const ui = useUiStore()
const noteHostId = inject(NOTE_HOST_ID, ref(null))

// Untagged blocks count as shell — that's what notes here mostly hold
const SHELL = new Set(['', 'sh', 'bash', 'zsh', 'fish', 'shell', 'console', 'terminal', 'powershell', 'pwsh', 'ps1', 'cmd', 'bat'])
const runnable = computed(() => SHELL.has(String(props.node.attrs.language ?? '').toLowerCase()))
const code = computed(() => props.node.textContent)
const empty = computed(() => code.value.trim() === '')

const target = computed(() => pickRunTarget(noteHostId.value))
const linkedHost = computed(() => hosts.hostById(noteHostId.value))
// A linked host that isn't the live target gets connected first
const willConnect = computed(
  () => linkedHost.value !== undefined && target.value?.hostId !== linkedHost.value.id,
)

const runTitle = computed(() => {
  if (empty.value) return 'Nothing to run'
  if (willConnect.value) return `Connect to ${linkedHost.value!.label} and run`
  if (target.value) return `Run in ${target.value.label}`
  return 'Connect to a host first — or link this note to one'
})

const busy = ref(false)

async function run() {
  if (busy.value || empty.value) return
  const command = stripPrompts(code.value)
  let tab = willConnect.value ? null : target.value
  if (!tab && linkedHost.value) {
    busy.value = true
    const hostId = linkedHost.value.id
    try {
      await sessions.connect(hostId, undefined, { silent: true })
      const id = await waitForConnection(hostId)
      tab = id === null ? null : (sessions.tabs.find((t) => t.id === id) ?? null)
    } finally {
      busy.value = false
    }
    if (!tab) return // the connect path already reported why
  }
  if (!tab) {
    ui.toast('info', 'Connect to a host first, or link this note to one')
    return
  }
  // Lines end in CR, exactly what pressing Enter sends — a remote tty maps
  // it to LF, and local PowerShell/cmd only accept a CR as Enter
  sessions.write(tab.id, command.replace(/\r?\n/g, '\r') + '\r')
  const sent = tab
  ui.toast('success', `Ran in ${sent.label}`, {
    actionLabel: 'Show',
    action: () => {
      sessions.activeId = sent.id
      ui.openView(`session:${sent.id}`)
    },
  })
}

const copied = ref(false)
let copiedTimer: ReturnType<typeof setTimeout> | undefined

async function copy() {
  try {
    await navigator.clipboard.writeText(stripPrompts(code.value))
    copied.value = true
    clearTimeout(copiedTimer)
    copiedTimer = setTimeout(() => (copied.value = false), 1400)
  } catch {
    ui.toast('error', 'Could not copy to the clipboard')
  }
}
</script>

<template>
  <NodeViewWrapper class="code-block">
    <pre><NodeViewContent as="code" /></pre>
    <!-- mousedown.prevent keeps the caret where it is -->
    <div class="code-actions" contenteditable="false">
      <button
        type="button"
        class="code-btn"
        :title="copied ? 'Copied' : 'Copy'"
        :aria-label="copied ? 'Copied' : 'Copy code'"
        @mousedown.prevent
        @click="copy"
      >
        <Check v-if="copied" :size="12" :stroke-width="2" aria-hidden="true" />
        <Copy v-else :size="12" :stroke-width="1.75" aria-hidden="true" />
      </button>
      <button
        v-if="runnable"
        type="button"
        class="code-btn run"
        :title="runTitle"
        :aria-label="runTitle"
        :disabled="empty || busy || (!target && !linkedHost)"
        @mousedown.prevent
        @click="run"
      >
        <span v-if="busy" class="spinner small" aria-hidden="true"></span>
        <Play v-else :size="11" :stroke-width="2" aria-hidden="true" />
        Run
      </button>
    </div>
  </NodeViewWrapper>
</template>

<style scoped>
.code-block {
  position: relative;
}

.code-actions {
  position: absolute;
  top: 6px;
  right: 6px;
  display: flex;
  gap: 4px;
  user-select: none;
  /* quiet until the block is in play; always shown on touch */
  opacity: 0.55;
  transition: opacity var(--dur-hover) ease;
}

.code-block:focus-within .code-actions {
  opacity: 1;
}

@media (hover: hover) and (pointer: fine) {
  .code-actions {
    opacity: 0;
  }

  .code-block:hover .code-actions,
  .code-block:focus-within .code-actions,
  .code-actions:focus-within {
    opacity: 1;
  }
}

.code-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  height: 22px;
  min-width: 22px;
  justify-content: center;
  padding: 0 6px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: var(--bg-surface);
  color: var(--text-2);
  font-family: var(--font-ui);
  font-size: var(--text-xs);
  font-weight: 600;
  transition:
    transform var(--dur-press) ease-out,
    background-color var(--dur-hover) ease,
    color var(--dur-hover) ease;
}

.code-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.code-btn:active {
  transform: scale(0.96);
}

.code-btn.run {
  color: var(--accent);
}

.code-btn:disabled {
  opacity: 0.45;
  pointer-events: none;
}
</style>
