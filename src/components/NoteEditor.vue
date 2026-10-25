<script setup lang="ts">
import { computed, provide, ref, watch } from 'vue'
import { EditorContent, VueNodeViewRenderer, useEditor } from '@tiptap/vue-3'
import type { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import CodeBlock from '@tiptap/extension-code-block'
import Placeholder from '@tiptap/extension-placeholder'
import TaskList from '@tiptap/extension-task-list'
import TaskItem from '@tiptap/extension-task-item'
import Table from '@tiptap/extension-table'
import TableRow from '@tiptap/extension-table-row'
import TableHeader from '@tiptap/extension-table-header'
import TableCell from '@tiptap/extension-table-cell'
import { Markdown } from 'tiptap-markdown'
import {
  ArrowLeft,
  Bold,
  Code,
  Heading1,
  Heading2,
  Italic,
  List,
  ListOrdered,
  ListTodo,
  Minus,
  Pin,
  PinOff,
  Plus,
  Server,
  Table as TableIcon,
  Trash2,
} from 'lucide-vue-next'
import { useNotesStore } from '../stores/notes'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { Hashtag } from './hashtagExtension'
import { TabKeys } from './tabKeysExtension'
import CodeBlockView from './CodeBlockView.vue'
import SearchSelect, { type ComboOption } from './SearchSelect.vue'
import { NOTE_HOST_ID } from './noteContext'
import type { Note } from '../types'

const props = defineProps<{ note: Note; back?: boolean }>()
const emit = defineEmits<{ remove: []; back: [] }>()

const notes = useNotesStore()
const hosts = useHostsStore()
const ui = useUiStore()

// ---- host link: where this note's code blocks run ----
provide(NOTE_HOST_ID, computed(() => props.note.host_id))

const hostOptions = computed<ComboOption[]>(() => [
  { value: null, label: 'No host', muted: true },
  ...[...hosts.hosts]
    .sort((a, b) => a.label.localeCompare(b.label))
    .map((h) => ({ value: h.id, label: h.label, hint: h.address })),
])

async function linkHost(value: string | number | null) {
  const hostId = typeof value === 'number' ? value : null
  if (hostId === props.note.host_id) return
  try {
    await notes.setHost(props.note.id, hostId)
  } catch (e) {
    ui.toast('error', `Could not link note: ${errorText(e)}`)
  }
}

// Code blocks get Copy/Run actions through a Vue node view; same node
// name, so markdown storage and the other extensions see no difference
const RunnableCodeBlock = CodeBlock.extend({
  addNodeView() {
    return VueNodeViewRenderer(CodeBlockView)
  },
})

const title = ref(props.note.title)
const titleEl = ref<HTMLInputElement | null>(null)

function syncEdit() {
  const e = editor.value
  notes.edit(props.note.id, title.value, e ? markdownOf(e) : props.note.body)
}

// tiptap-markdown keeps markdown as the storage format, so existing
// notes and Quick Note entries open losslessly.
function markdownOf(e: Editor): string {
  const md = (e.storage as unknown as { markdown?: { getMarkdown(): string } }).markdown
  return md ? md.getMarkdown() : e.getText()
}

const editor = useEditor({
  content: props.note.body,
  extensions: [
    StarterKit.configure({ heading: { levels: [1, 2, 3] }, codeBlock: false }),
    RunnableCodeBlock,
    Placeholder.configure({ placeholder: 'Start typing… #tag to organize' }),
    TaskList,
    TaskItem.configure({ nested: true }),
    Table.configure({ resizable: false }),
    TableRow,
    TableHeader,
    TableCell,
    Markdown.configure({ html: false, breaks: true, transformPastedText: true }),
    Hashtag,
    // Last on purpose: its Tab handling must run after the list/task
    // indent shortcuts so it only sees keys they didn't consume
    TabKeys,
  ],
  onUpdate: ({ editor }) => {
    notes.edit(props.note.id, title.value, markdownOf(editor))
  },
})

// Toolbar buttons must light up as the caret moves — transactions
// bump a tick the computed below depends on.
const tick = ref(0)
watch(
  editor,
  (e, _prev, onCleanup) => {
    if (!e) return
    const bump = () => {
      tick.value++
    }
    e.on('transaction', bump)
    onCleanup(() => e.off('transaction', bump))
  },
  { immediate: true },
)

const NO_ACTIVE: Record<string, boolean> = {}

const active = computed<Record<string, boolean>>(() => {
  void tick.value
  const e = editor.value
  if (!e) return NO_ACTIVE
  return {
    h1: e.isActive('heading', { level: 1 }),
    h2: e.isActive('heading', { level: 2 }),
    bullet: e.isActive('bulletList'),
    number: e.isActive('orderedList'),
    todo: e.isActive('taskList'),
    bold: e.isActive('bold'),
    italic: e.isActive('italic'),
    code: e.isActive('code'),
    codeBlock: e.isActive('codeBlock'),
    table: e.isActive('table'),
  }
})

function run(fn: (chain: ReturnType<Editor['chain']> | undefined) => void) {
  const chain = editor.value?.chain().focus()
  if (chain) fn(chain)
}

function insertTable() {
  run((c) => c?.insertTable({ rows: 2, cols: 2, withHeaderRow: true }).run())
}

// Swap content when a different note opens — the editor instance
// itself lives as long as the pane does.
watch(
  () => props.note.id,
  (id) => {
    const note = notes.notes.find((n) => n.id === id)
    title.value = note?.title ?? ''
    if (editor.value) editor.value.commands.setContent(note?.body ?? '', false)
  },
)

function onTitleEnter() {
  editor.value?.commands.focus('end')
}

function onScrollClick(e: MouseEvent) {
  const t = e.target as HTMLElement
  if (t === e.currentTarget || t.classList.contains('editor-inner')) {
    editor.value?.commands.focus('end')
  }
}

async function togglePin() {
  try {
    await notes.setPinned(props.note.id, !props.note.pinned)
  } catch (e) {
    ui.toast('error', `Could not update note: ${errorText(e)}`)
  }
}

// ---- dates (SQLite 'YYYY-MM-DD HH:MM:SS', local time) ----

function parseDate(s: string): Date | null {
  const d = new Date(s.replace(' ', 'T'))
  return Number.isNaN(d.getTime()) ? null : d
}

function listDate(s: string): string {
  const d = parseDate(s)
  if (!d) return ''
  const now = new Date()
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }
  const yesterday = new Date(now)
  yesterday.setDate(now.getDate() - 1)
  if (d.toDateString() === yesterday.toDateString()) return 'Yesterday'
  const opts: Intl.DateTimeFormatOptions =
    d.getFullYear() === now.getFullYear()
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' }
  return d.toLocaleDateString([], opts)
}

function editedLabel(s: string): string {
  const d = parseDate(s)
  if (!d) return ''
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  if (d.toDateString() === new Date().toDateString()) return `Edited today at ${time}`
  return `Edited ${listDate(s)} at ${time}`
}

defineExpose({
  focusTitle: () => titleEl.value?.focus(),
})
</script>

<template>
  <div class="note-editor">
    <header class="editor-toolbar">
      <div class="toolbar-left">
        <button v-if="back" class="btn-icon" aria-label="All notes" title="All notes" @click="emit('back')">
          <ArrowLeft :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <span v-if="back" class="fmt-sep" aria-hidden="true"></span>
        <div class="format-group" role="toolbar" aria-label="Formatting">
          <button
            class="btn-icon"
            :class="{ on: active.h1 }"
            title="Heading 1"
            aria-label="Heading 1"
            @click="run((c) => c?.toggleHeading({ level: 1 }).run())"
          >
            <Heading1 :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button
            class="btn-icon"
            :class="{ on: active.h2 }"
            title="Heading 2"
            aria-label="Heading 2"
            @click="run((c) => c?.toggleHeading({ level: 2 }).run())"
          >
            <Heading2 :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <span class="fmt-sep" aria-hidden="true"></span>
          <button
            class="btn-icon"
            :class="{ on: active.bullet }"
            title="Bulleted list"
            aria-label="Bulleted list"
            @click="run((c) => c?.toggleBulletList().run())"
          >
            <List :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button
            class="btn-icon"
            :class="{ on: active.number }"
            title="Numbered list"
            aria-label="Numbered list"
            @click="run((c) => c?.toggleOrderedList().run())"
          >
            <ListOrdered :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button
            class="btn-icon"
            :class="{ on: active.todo }"
            title="Checklist"
            aria-label="Checklist"
            @click="run((c) => c?.toggleTaskList().run())"
          >
            <ListTodo :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <button class="btn-icon" title="Insert table" aria-label="Insert table" @click="insertTable">
            <TableIcon :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
          <span class="fmt-sep" aria-hidden="true"></span>
          <button
            class="btn-icon"
            :class="{ on: active.bold }"
            title="Bold (Ctrl+B)"
            aria-label="Bold"
            @click="run((c) => c?.toggleBold().run())"
          >
            <Bold :size="13" :stroke-width="1.75" aria-hidden="true" />
          </button>
          <button
            class="btn-icon"
            :class="{ on: active.italic }"
            title="Italic (Ctrl+I)"
            aria-label="Italic"
            @click="run((c) => c?.toggleItalic().run())"
          >
            <Italic :size="13" :stroke-width="1.75" aria-hidden="true" />
          </button>
          <button
            class="btn-icon"
            :class="{ on: active.code || active.codeBlock }"
            title="Code (Ctrl+E)"
            aria-label="Code"
            @click="run((c) => c?.toggleCode().run())"
          >
            <Code :size="13" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
        <template v-if="active.table">
          <span class="fmt-sep" aria-hidden="true"></span>
          <div class="format-group" role="group" aria-label="Table rows">
            <button
              class="btn-icon"
              title="Add row"
              aria-label="Add row"
              @click="run((c) => c?.addRowAfter().run())"
            >
              <Plus :size="13" :stroke-width="1.75" aria-hidden="true" />
            </button>
            <button
              class="btn-icon"
              title="Delete row"
              aria-label="Delete row"
              @click="run((c) => c?.deleteRow().run())"
            >
              <Minus :size="13" :stroke-width="1.75" aria-hidden="true" />
            </button>
            <button
              class="btn-icon note-delete"
              title="Delete table"
              aria-label="Delete table"
              @click="run((c) => c?.deleteTable().run())"
            >
              <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
        </template>
      </div>
      <div class="editor-actions">
        <div class="host-link" title="Linked host — code blocks run there, and the note shows in its details">
          <Server :size="13" :stroke-width="1.5" class="host-link-icon" aria-hidden="true" />
          <SearchSelect
            compact
            :model-value="note.host_id"
            :options="hostOptions"
            aria-label="Linked host"
            @update:model-value="linkHost"
          />
        </div>
        <button
          class="btn-icon"
          :class="{ pinned: note.pinned }"
          :aria-label="note.pinned ? 'Unpin note' : 'Pin note'"
          :title="note.pinned ? 'Unpin' : 'Pin'"
          @click="togglePin"
        >
          <Pin v-if="!note.pinned" :size="14" :stroke-width="1.5" aria-hidden="true" />
          <PinOff v-else :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
        <button class="btn-icon note-delete" aria-label="Delete note" title="Delete" @click="emit('remove')">
          <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>
    </header>

    <div class="editor-scroll" @click="onScrollClick">
      <div class="editor-inner">
        <p class="edited-at">{{ editedLabel(note.updated_at) }}</p>
        <input
          ref="titleEl"
          v-model="title"
          class="note-title"
          type="text"
          placeholder="Title"
          aria-label="Note title"
          spellcheck="false"
          @input="syncEdit"
          @keydown.enter.prevent="onTitleEnter"
        />

        <EditorContent :editor="editor" class="note-prose" aria-label="Note body" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.note-editor {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.editor-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-2);
  padding: 6px var(--space-4);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.toolbar-left {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 1px;
  min-width: 0;
}

.format-group {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 1px;
}

.fmt-sep {
  width: 1px;
  height: 16px;
  background: var(--border);
  margin: 0 5px;
  flex-shrink: 0;
}

.btn-icon.on {
  color: var(--accent);
  background: var(--accent-bg);
}

.editor-actions {
  display: flex;
  align-items: center;
  gap: 2px;
}

.host-link {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-right: 4px;
  max-width: 200px;
  min-width: 0;
}

.host-link-icon {
  color: var(--text-3);
  flex-shrink: 0;
}

.host-link :deep(.combo-trigger.compact) {
  max-width: 180px;
  font-family: var(--font-ui);
  border-color: transparent;
  background: transparent;
}

.host-link :deep(.combo-trigger.compact:hover) {
  background: var(--bg-hover);
}

.editor-actions .btn-icon.pinned {
  color: var(--accent);
}

.note-delete:hover {
  color: var(--danger);
}

.editor-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-5);
  min-height: 0;
}

.editor-inner {
  max-width: 720px;
  margin: 0 auto;
}

.note-title {
  display: block;
  width: 100%;
  background: transparent;
  border: none;
  outline: none;
  font-size: var(--text-xl);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  line-height: var(--leading-tight);
  color: var(--text-1);
  padding: 0;
  margin-bottom: var(--space-2);
}

.note-title::placeholder {
  color: var(--text-4);
}

/* Apple Notes puts the timestamp centered above the note, out of the way */
.edited-at {
  font-size: var(--text-xs);
  color: var(--text-3);
  text-align: center;
  font-variant-numeric: tabular-nums;
  margin: 0 0 var(--space-4);
}

/* ---- inline #tags (decorated by the Hashtag extension) ---- */

.note-prose :deep(.hashtag) {
  color: var(--tag-color, var(--accent));
  background: color-mix(in oklch, var(--tag-color, var(--accent)) 10%, transparent);
  border-radius: 4px;
  padding: 0 3px;
  font-weight: 500;
}

/* ---- prose (Tiptap) ---- */

.note-prose {
  display: block;
}

.note-prose :deep(.tiptap) {
  outline: none;
  min-height: 45vh;
  font-size: var(--text-md);
  line-height: 1.65;
  color: var(--text-1);
  overflow-wrap: break-word;
  caret-color: var(--text-1);
}

.note-prose :deep(.tiptap > * + *) {
  margin-top: 0.55em;
}

.note-prose :deep(.tiptap h1) {
  font-size: 1.4em;
  font-weight: 700;
  letter-spacing: var(--tracking-tight);
  line-height: var(--leading-tight);
  margin-top: 1.1em;
}

.note-prose :deep(.tiptap h2) {
  font-size: 1.18em;
  font-weight: 650;
  letter-spacing: var(--tracking-tight);
  line-height: var(--leading-tight);
  margin-top: 1em;
}

.note-prose :deep(.tiptap h3) {
  font-size: 1.05em;
  font-weight: 600;
  margin-top: 0.9em;
}

/* base.css resets all lists app-wide — the prose gets real markers back */
.note-prose :deep(.tiptap ul) {
  list-style: disc;
}

.note-prose :deep(.tiptap ol) {
  list-style: decimal;
}

.note-prose :deep(.tiptap ul ul) {
  list-style: circle;
}

.note-prose :deep(.tiptap ol ol) {
  list-style: lower-alpha;
}

.note-prose :deep(.tiptap ul),
.note-prose :deep(.tiptap ol) {
  padding-left: 1.4em;
}

.note-prose :deep(.tiptap li) {
  margin-top: 0.2em;
}

.note-prose :deep(.tiptap li::marker) {
  color: var(--text-3);
}

.note-prose :deep(.tiptap ul ul),
.note-prose :deep(.tiptap ol ol),
.note-prose :deep(.tiptap ul ol),
.note-prose :deep(.tiptap ol ul) {
  margin-top: 0.2em;
}

.note-prose :deep(.tiptap blockquote) {
  border-left: 3px solid color-mix(in oklch, var(--accent) 45%, var(--border));
  padding-left: 12px;
  color: var(--text-2);
}

.note-prose :deep(.tiptap code) {
  font-family: var(--font-mono);
  font-size: 0.875em;
  background: color-mix(in oklch, var(--accent) 9%, transparent);
  color: color-mix(in oklch, var(--accent) 55%, var(--text-1));
  border-radius: 4px;
  padding: 1px 5px;
}

.note-prose :deep(.tiptap pre) {
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 10px 12px;
  overflow-x: auto;
}

.note-prose :deep(.tiptap pre code) {
  background: transparent;
  color: var(--text-1);
  padding: 0;
  font-size: 0.85em;
  line-height: 1.6;
}

.note-prose :deep(.tiptap hr) {
  border: none;
  border-top: 1px solid var(--border);
  margin: 1.2em 0;
}

.note-prose :deep(.tiptap hr.ProseMirror-selectednode) {
  border-top-color: var(--accent);
}

/* Checklists */
.note-prose :deep(.tiptap ul[data-type='taskList']) {
  list-style: none;
  padding-left: 2px;
}

.note-prose :deep(.tiptap ul[data-type='taskList'] li) {
  display: flex;
  align-items: flex-start;
  gap: 8px;
}

.note-prose :deep(.tiptap ul[data-type='taskList'] li > label) {
  margin-top: 3.5px;
  flex-shrink: 0;
  user-select: none;
}

/* Round checkbox, Apple Notes style. Press feedback on pointer-down;
   the check is always rendered and transitions in (interruptible —
   rapid toggling retargets instead of restarting). */
.note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']) {
  appearance: none;
  width: 16px;
  height: 16px;
  border: 1.5px solid var(--border-strong);
  border-radius: 999px;
  background: transparent;
  cursor: pointer;
  display: block;
  position: relative;
  transition:
    transform var(--dur-press) ease-out,
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease;
}

.note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']::after) {
  content: '';
  position: absolute;
  inset: 0;
  background-color: var(--accent-text);
  clip-path: polygon(22% 48%, 15% 55%, 41% 79%, 85% 30%, 78% 23%, 41% 66%);
  opacity: 0;
  transform: scale(0.6);
  transition:
    opacity 120ms var(--ease-out),
    transform 200ms var(--ease-out);
}

.note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']:active) {
  transform: scale(0.9);
}

@media (hover: hover) and (pointer: fine) {
  .note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']:hover) {
    border-color: var(--accent);
  }
}

.note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']:checked) {
  background: var(--accent);
  border-color: var(--accent);
}

.note-prose :deep(.tiptap ul[data-type='taskList'] input[type='checkbox']:checked::after) {
  opacity: 1;
  transform: scale(1);
}

.note-prose :deep(.tiptap ul[data-type='taskList'] li > div) {
  transition: color var(--dur-hover) ease;
}

.note-prose :deep(.tiptap ul[data-type='taskList'] li[data-checked='true'] > div) {
  color: var(--text-3);
  text-decoration: line-through;
  text-decoration-color: var(--text-4);
}

/* Tables */
.note-prose :deep(.tiptap table) {
  border-collapse: collapse;
  table-layout: fixed;
  width: 100%;
  margin: 0.8em 0;
  overflow: hidden;
}

.note-prose :deep(.tiptap td),
.note-prose :deep(.tiptap th) {
  border: 1px solid var(--border);
  padding: 5px 10px;
  vertical-align: top;
  position: relative;
}

.note-prose :deep(.tiptap th) {
  background: var(--bg-surface-2);
  font-weight: 600;
  text-align: left;
}

.note-prose :deep(.tiptap .selectedCell::after) {
  content: '';
  position: absolute;
  inset: 0;
  background: var(--accent-bg);
  pointer-events: none;
}

/* Placeholder — only the very first empty paragraph */
.note-prose :deep(.tiptap p.is-editor-empty:first-child::before) {
  content: attr(data-placeholder);
  color: var(--text-4);
  float: left;
  height: 0;
  pointer-events: none;
}

.note-prose :deep(.tiptap) ::selection {
  background: var(--accent-bg-hover);
}
</style>
