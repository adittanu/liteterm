<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  Copy,
  MoreHorizontal,
  Pencil,
  Play,
  Plus,
  ScrollText,
  SquareTerminal,
  TextCursorInput,
  Trash2,
} from 'lucide-vue-next'
import { useSnippetsStore } from '../stores/extras'
import { useSessionsStore, errorText } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import SnippetDialog from '../components/SnippetDialog.vue'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'
import SearchField from '../components/SearchField.vue'
import SearchSelect, { type ComboOption } from '../components/SearchSelect.vue'
import { copyText, matchesQuery } from '../utils/pageHelpers'
import type { Snippet } from '../types'

const snippets = useSnippetsStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const query = ref('')
const editorOpen = ref(false)
const editing = ref<Snippet | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)

const visible = computed(() =>
  [...snippets.snippets]
    .sort((a, b) => a.sort - b.sort || a.id - b.id)
    .filter((s) => matchesQuery(query.value, s.name, s.command)),
)

// ---- run target: explicit, never a guess the user can't see ----

const connectedTabs = computed(() => sessions.tabs.filter((t) => t.status === 'connected'))
const chosenId = ref<number | null>(null)

// The user's pick while it stays connected, else the last focused terminal,
// else the newest connected one
const targetId = computed<number | null>(() => {
  const live = connectedTabs.value
  if (chosenId.value !== null && live.some((t) => t.id === chosenId.value)) return chosenId.value
  if (sessions.activeId !== null && live.some((t) => t.id === sessions.activeId)) return sessions.activeId
  return live.length ? live[live.length - 1].id : null
})
const target = computed(() => connectedTabs.value.find((t) => t.id === targetId.value) ?? null)

const targetOptions = computed<ComboOption[]>(() =>
  connectedTabs.value.map((t) => ({ value: t.id, label: t.label, hint: t.address })),
)

function run(s: Snippet, execute: boolean) {
  const tab = target.value
  if (!tab) {
    ui.toast('info', 'Connect to a host or open a local terminal first')
    return
  }
  sessions.write(tab.id, execute ? s.command + '\n' : s.command)
  ui.toast('success', `${execute ? 'Ran' : 'Inserted'} “${s.name}” in ${tab.label}`, {
    actionLabel: 'Show',
    action: () => {
      sessions.activeId = tab.id
      ui.openView(`session:${tab.id}`)
    },
  })
}

function openNew() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(s: Snippet) {
  editing.value = s
  editorOpen.value = true
}

function openMenu(s: Snippet, e: MouseEvent) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      { label: 'Run', icon: Play, disabled: !target.value, action: () => run(s, true) },
      { label: 'Insert without running', icon: TextCursorInput, disabled: !target.value, action: () => run(s, false) },
      { label: 'Copy command', icon: Copy, action: () => void copyText(s.command, 'Command copied') },
      { label: 'Edit', icon: Pencil, action: () => openEdit(s) },
      { label: 'Delete', icon: Trash2, danger: true, action: () => void remove(s) },
    ],
  }
}

// Forgiveness over confirmation: delete now, Undo puts it back
async function remove(s: Snippet) {
  try {
    await snippets.remove(s.id)
  } catch (e) {
    ui.toast('error', `Could not delete: ${errorText(e)}`)
    return
  }
  ui.toast('info', `Deleted “${s.name}”`, {
    actionLabel: 'Undo',
    duration: 6000,
    action: () => void snippets.save(s.name, s.command).catch((e) => ui.toast('error', errorText(e))),
  })
}

function lineCount(cmd: string): number {
  return cmd.split('\n').length
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div class="page-heading">
        <h2 class="page-title">
          Snippets
          <span v-if="snippets.snippets.length" class="page-count">{{ snippets.snippets.length }}</span>
        </h2>
        <p class="page-sub">Commands you type often, one click from any connected terminal.</p>
      </div>
      <div class="page-actions">
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New snippet
        </button>
      </div>
    </header>

    <div v-if="snippets.snippets.length" class="page-toolbar">
      <SearchField v-model="query" label="Search snippets" placeholder="Search names and commands" />
      <span class="toolbar-spacer"></span>
      <span class="toolbar-label" id="run-target-label">Run in</span>
      <div v-if="connectedTabs.length > 1" class="target-select">
        <SearchSelect
          :model-value="targetId"
          :options="targetOptions"
          aria-labelledby="run-target-label"
          @update:model-value="chosenId = $event as number"
        />
      </div>
      <span v-else-if="target" class="target-fixed" :title="target.address">
        <span class="status-dot connected" aria-hidden="true"></span>
        <span class="text-ellipsis">{{ target.label }}</span>
      </span>
      <button v-else class="target-fixed none" title="Open a local terminal" @click="sessions.connectLocal()">
        <SquareTerminal :size="13" :stroke-width="1.5" aria-hidden="true" />
        No terminal — open local
      </button>
    </div>

    <div class="page-scroll">
      <div v-if="!snippets.snippets.length" class="empty-hero">
        <span class="empty-icon"><ScrollText :size="22" :stroke-width="1.5" aria-hidden="true" /></span>
        <p class="empty-title">No snippets yet</p>
        <p class="empty-sub">
          Save the commands you keep retyping — restarts, log tails, deploys — and send them to a
          terminal in one click.
        </p>
        <button class="btn btn-primary" @click="openNew">
          <Plus :size="14" :stroke-width="2" aria-hidden="true" />
          New snippet
        </button>
      </div>

      <p v-else-if="!visible.length" class="no-results">No snippets match “{{ query.trim() }}”</p>

      <ul v-else class="inset-list">
        <li
          v-for="s in visible"
          :key="s.id"
          class="list-row snip-row"
          @contextmenu.prevent="openMenu(s, $event)"
        >
          <button class="row-main snip-main" :aria-label="`Edit ${s.name}`" @click="openEdit(s)">
            <span class="row-title">
              <span class="text-ellipsis">{{ s.name }}</span>
              <span v-if="lineCount(s.command) > 1" class="chip quiet">{{ lineCount(s.command) }} lines</span>
            </span>
            <code class="snip-cmd mono">{{ s.command }}</code>
          </button>

          <span class="row-actions">
            <button
              class="btn-icon"
              :aria-label="`Insert ${s.name} without running`"
              title="Insert without running"
              :disabled="!target"
              @click="run(s, false)"
            >
              <TextCursorInput :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Copy ${s.name}`" title="Copy command" @click="copyText(s.command, 'Command copied')">
              <Copy :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Options for ${s.name}`" @click.stop="openMenu(s, $event)">
              <MoreHorizontal :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </span>

          <button
            class="btn btn-secondary run-btn"
            :disabled="!target"
            :aria-label="target ? `Run ${s.name} in ${target.label}` : `Run ${s.name}`"
            :title="target ? `Run in ${target.label}` : 'No connected terminal'"
            @click="run(s, true)"
          >
            <Play :size="12" :stroke-width="2" aria-hidden="true" />
            Run
          </button>
        </li>
      </ul>
    </div>

    <SnippetDialog v-if="editorOpen" :snippet="editing" @close="editorOpen = false" />
    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.target-select {
  width: 200px;
}

.target-select :deep(.combo-trigger) {
  padding-block: 5px;
  font-size: var(--text-base);
}

.target-fixed {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  max-width: 220px;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--radius-md);
  border: 1px solid var(--border);
  background: var(--bg-surface);
  font-size: var(--text-base);
  color: var(--text-1);
}

.target-fixed.none {
  color: var(--text-3);
  border-style: dashed;
  background: transparent;
  transition: color var(--dur-hover) ease, background-color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.target-fixed.none:hover {
  color: var(--text-1);
  background: var(--bg-hover);
}

.target-fixed.none:active {
  transform: scale(0.97);
}

.snip-row {
  --row-inset: 14px;
  align-items: flex-start;
  padding-block: 12px;
}

.snip-main {
  gap: 6px;
  text-align: left;
  border-radius: var(--radius-sm);
}

.snip-cmd {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 3;
  overflow: hidden;
  white-space: pre-wrap;
  word-break: break-all;
  padding: 6px 9px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border);
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-2);
}

.snip-row .row-actions,
.run-btn {
  margin-top: 1px;
}

.run-btn {
  flex-shrink: 0;
  gap: 5px;
  padding: 4px 9px 4px 8px;
  font-size: var(--text-sm);
}

.run-btn:not(:disabled):hover {
  background: var(--accent-bg);
  border-color: color-mix(in oklch, var(--accent) 35%, transparent);
  color: var(--accent);
}

.btn-icon:disabled {
  opacity: 0.35;
  pointer-events: none;
}
</style>
