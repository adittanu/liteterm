<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import {
  Search,
  X,
  SquarePen,
  Pin,
  PinOff,
  Trash2,
  StickyNote,
  Heading1,
  Heading2,
  List,
  ListOrdered,
  ListTodo,
  Table,
  Bold,
  Italic,
  Code,
} from 'lucide-vue-next'
import { useNotesStore } from '../stores/notes'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { Note } from '../types'

const notes = useNotesStore()
const ui = useUiStore()

const query = ref('')
const selectedId = ref<number | null>(null)
const titleDraft = ref('')
const bodyDraft = ref('')
const titleEl = ref<HTMLInputElement | null>(null)
const bodyEl = ref<HTMLTextAreaElement | null>(null)

// Pinned first, then most recently edited (SQLite timestamps sort
// lexicographically: 'YYYY-MM-DD HH:MM:SS')
const sorted = computed(() =>
  [...notes.notes].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
    if (a.updated_at !== b.updated_at) return a.updated_at < b.updated_at ? 1 : -1
    return b.id - a.id
  }),
)

const searching = computed(() => query.value.trim().length > 0)

const matches = computed(() => {
  if (!searching.value) return sorted.value
  const q = query.value.trim().toLowerCase()
  return sorted.value.filter(
    (n) => n.title.toLowerCase().includes(q) || n.body.toLowerCase().includes(q),
  )
})

// Search shows one flat list, like Apple Notes
const pinnedNotes = computed(() => (searching.value ? [] : matches.value.filter((n) => n.pinned)))
const otherNotes = computed(() =>
  searching.value ? matches.value : matches.value.filter((n) => !n.pinned),
)

const selectedNote = computed(() => notes.notes.find((n) => n.id === selectedId.value) ?? null)

onMounted(async () => {
  if (!notes.loaded) await notes.load()
  const first = sorted.value[0]
  if (first) select(first.id)
})

function select(id: number) {
  selectedId.value = id
  const note = notes.notes.find((n) => n.id === id)
  titleDraft.value = note?.title ?? ''
  bodyDraft.value = note?.body ?? ''
  nextTick(autogrow)
}

function sync() {
  if (selectedId.value === null) return
  notes.edit(selectedId.value, titleDraft.value, bodyDraft.value)
  nextTick(autogrow)
}

// The body textarea grows with its content so the pane scrolls as one
// continuous page, like Apple Notes.
function autogrow() {
  const el = bodyEl.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${el.scrollHeight}px`
}

// ---- formatting ----

type BlockKind = 'h1' | 'h2' | 'bullet' | 'number' | 'checkbox'

const BLOCK_PREFIX: Record<BlockKind, string> = {
  h1: '# ',
  h2: '## ',
  bullet: '- ',
  number: '', // generated per line
  checkbox: '- [ ] ',
}

const BLOCK_TEST: Record<BlockKind, RegExp> = {
  h1: /^# /,
  h2: /^## /,
  bullet: /^[-*] /,
  number: /^\d+\. /,
  checkbox: /^[-*] \[[xX ]\] /,
}

// Checkbox before bullet — the alternation order matters
const BLOCK_STRIP = /^(#{1,6} |[-*] \[[xX ]\] |[-*] |\d+\. )/

function selectedLines() {
  const el = bodyEl.value
  if (!el) return null
  const value = bodyDraft.value
  const s = el.selectionStart
  const e = el.selectionEnd
  const lineStart = s === 0 ? 0 : value.lastIndexOf('\n', s - 1) + 1
  let lineEnd = value.indexOf('\n', e)
  if (lineEnd === -1) lineEnd = value.length
  return { el, value, s, e, lineStart, lineEnd, lines: value.slice(lineStart, lineEnd).split('\n') }
}

function applyBlock(kind: BlockKind) {
  const ctx = selectedLines()
  if (!ctx) return
  const test = BLOCK_TEST[kind]
  // Toggle: every touched line already carries the prefix → strip it
  const remove = ctx.lines.every((l) => test.test(l))
  let n = 0
  const next = ctx.lines.map((l) => {
    const bare = l.replace(BLOCK_STRIP, '')
    if (remove) return bare
    if (kind === 'number') return `${++n}. ${bare}`
    return BLOCK_PREFIX[kind] + bare
  })
  const joined = next.join('\n')
  bodyDraft.value = ctx.value.slice(0, ctx.lineStart) + joined + ctx.value.slice(ctx.lineEnd)
  sync()
  const selStart = ctx.lineStart
  const selEnd = ctx.lineStart + joined.length
  nextTick(() => {
    ctx.el.focus()
    ctx.el.setSelectionRange(selStart, selEnd)
  })
}

function wrapInline(marker: string) {
  const el = bodyEl.value
  if (!el) return
  const value = bodyDraft.value
  const s = el.selectionStart
  const e = el.selectionEnd
  const sel = value.slice(s, e)
  // Selection already wrapped → unwrap
  if (sel.length >= marker.length * 2 && sel.startsWith(marker) && sel.endsWith(marker)) {
    const inner = sel.slice(marker.length, sel.length - marker.length)
    setBody(value.slice(0, s) + inner + value.slice(e), s, s + inner.length)
    return
  }
  // Markers right around the selection → unwrap
  if (value.slice(s - marker.length, s) === marker && value.slice(e, e + marker.length) === marker) {
    setBody(
      value.slice(0, s - marker.length) + sel + value.slice(e + marker.length),
      s - marker.length,
      e - marker.length,
    )
    return
  }
  const start = s + marker.length
  const end = start + sel.length
  setBody(value.slice(0, s) + marker + sel + marker + value.slice(e), start, end)
}

function insertTable() {
  const el = bodyEl.value
  if (!el) return
  const value = bodyDraft.value
  const s = el.selectionStart
  const before = value.slice(0, s)
  const gap = !before || before.endsWith('\n\n') ? '' : before.endsWith('\n') ? '\n' : '\n\n'
  const table = '| Column 1 | Column 2 |\n| -------- | -------- |\n|          |          |\n'
  const caret = s + gap.length + table.length
  setBody(before + gap + table + value.slice(s), caret, caret)
}

function setBody(text: string, selStart: number, selEnd: number) {
  bodyDraft.value = text
  sync()
  nextTick(() => {
    const el = bodyEl.value
    if (!el) return
    el.focus()
    el.setSelectionRange(selStart, selEnd)
  })
}

function onBodyKeydown(e: KeyboardEvent) {
  if (!e.ctrlKey || e.altKey || e.metaKey) return
  const k = e.key.toLowerCase()
  if (k === 'b') {
    e.preventDefault()
    wrapInline('**')
  } else if (k === 'i') {
    e.preventDefault()
    wrapInline('*')
  }
}

// ---- syntax highlight (decorative layer under the textarea) ----
// The overlay must stay character-for-character identical to the
// textarea text, so only non-layout-affecting styles are used
// (color, opacity, line-through) — never size or weight.

function escapeHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function highlightLine(line: string): string {
  const esc = escapeHtml(line)
  let m = /^(#{1,6} )/.exec(line)
  if (m) {
    const len = m[1].length
    const cls = m[1].trim() === '#' ? 'hl-h1' : 'hl-h2'
    return `<span class="${cls}">${esc.slice(0, len)}</span>${esc.slice(len)}`
  }
  m = /^([-*] \[[xX]\] )/.exec(line)
  if (m) {
    return `<span class="hl-done">${esc}</span>`
  }
  m = /^([-*] \[ \] )/.exec(line)
  if (m) {
    return `<span class="hl-marker">${esc.slice(0, m[1].length)}</span>${esc.slice(m[1].length)}`
  }
  m = /^([-*] )/.exec(line)
  if (m) {
    return `<span class="hl-marker">${esc.slice(0, m[1].length)}</span>${esc.slice(m[1].length)}`
  }
  m = /^(\d+\. )/.exec(line)
  if (m) {
    return `<span class="hl-marker">${esc.slice(0, m[1].length)}</span>${esc.slice(m[1].length)}`
  }
  return esc
}

const highlightHtml = computed(() =>
  bodyDraft.value.split('\n').map(highlightLine).join('\n'),
)

// ---- note actions ----

async function createNote() {
  try {
    const note = await notes.create()
    select(note.id)
    nextTick(() => titleEl.value?.focus())
  } catch (e) {
    ui.toast('error', `Could not create note: ${errorText(e)}`)
  }
}

async function togglePin() {
  const note = selectedNote.value
  if (!note) return
  try {
    await notes.setPinned(note.id, !note.pinned)
  } catch (e) {
    ui.toast('error', `Could not update note: ${errorText(e)}`)
  }
}

async function removeSelected() {
  const id = selectedId.value
  if (id === null) return
  const list = matches.value
  const idx = list.findIndex((n) => n.id === id)
  try {
    const undo = await notes.remove(id)
    ui.toast('info', 'Note deleted', { actionLabel: 'Undo', action: undo, duration: 6000 })
  } catch (e) {
    ui.toast('error', `Could not delete note: ${errorText(e)}`)
    return
  }
  const next = list[idx + 1] ?? list[idx - 1] ?? null
  if (next) select(next.id)
  else {
    selectedId.value = null
    titleDraft.value = ''
    bodyDraft.value = ''
  }
}

function onEditorClick(e: MouseEvent) {
  const t = e.target as HTMLElement
  if (t === e.currentTarget || t.classList.contains('editor-inner')) focusBodyEnd()
}

function focusBodyEnd() {
  const el = bodyEl.value
  if (!el) return
  el.focus()
  el.selectionStart = el.selectionEnd = el.value.length
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

const PREVIEW_STRIP = /^(#{1,6} |[-*] \[[xX ]\] |[-*] |\d+\. )/gm

function previewOf(n: Note): string {
  return n.body.replace(PREVIEW_STRIP, '').replace(/\s+/g, ' ').trim()
}
</script>

<template>
  <div class="notes">
    <aside class="notes-list">
      <div class="list-toolbar">
        <div class="search-box">
          <Search :size="13" :stroke-width="1.5" aria-hidden="true" />
          <input
            v-model="query"
            type="text"
            placeholder="Search"
            aria-label="Search notes"
            spellcheck="false"
          />
          <button
            v-if="query"
            class="search-clear"
            aria-label="Clear search"
            @click="query = ''"
          >
            <X :size="11" :stroke-width="2" aria-hidden="true" />
          </button>
        </div>
        <button class="btn-icon compose" aria-label="New note" title="New note" @click="createNote">
          <SquarePen :size="15" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>

      <div class="list-scroll">
        <template v-if="pinnedNotes.length || otherNotes.length">
          <p v-if="pinnedNotes.length" class="section-label list-label">Pinned</p>
          <button
            v-for="n in pinnedNotes"
            :key="n.id"
            class="note-row"
            :class="{ selected: n.id === selectedId }"
            @click="select(n.id)"
          >
            <span class="row-title text-ellipsis">{{ n.title || 'New Note' }}</span>
            <span class="row-sub">
              <span class="row-date">{{ listDate(n.updated_at) }}</span>
              {{ previewOf(n) || 'No additional text' }}
            </span>
          </button>

          <p v-if="pinnedNotes.length && otherNotes.length" class="section-label list-label">
            Notes
          </p>
          <button
            v-for="n in otherNotes"
            :key="n.id"
            class="note-row"
            :class="{ selected: n.id === selectedId }"
            @click="select(n.id)"
          >
            <span class="row-title text-ellipsis">{{ n.title || 'New Note' }}</span>
            <span class="row-sub">
              <span class="row-date">{{ listDate(n.updated_at) }}</span>
              {{ previewOf(n) || 'No additional text' }}
            </span>
          </button>
        </template>

        <div v-else-if="searching" class="list-empty">
          <p>No notes match “{{ query.trim() }}”</p>
        </div>
        <div v-else class="list-empty">
          <p>No notes yet</p>
        </div>
      </div>
    </aside>

    <section class="notes-editor">
      <template v-if="selectedNote">
        <header class="editor-toolbar">
          <div class="format-group" role="toolbar" aria-label="Formatting">
            <button class="btn-icon" title="Heading 1" aria-label="Heading 1" @click="applyBlock('h1')">
              <Heading1 :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Heading 2" aria-label="Heading 2" @click="applyBlock('h2')">
              <Heading2 :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <span class="fmt-sep" aria-hidden="true"></span>
            <button class="btn-icon" title="Bulleted list" aria-label="Bulleted list" @click="applyBlock('bullet')">
              <List :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Numbered list" aria-label="Numbered list" @click="applyBlock('number')">
              <ListOrdered :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Checklist" aria-label="Checklist" @click="applyBlock('checkbox')">
              <ListTodo :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Table" aria-label="Insert table" @click="insertTable">
              <Table :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <span class="fmt-sep" aria-hidden="true"></span>
            <button class="btn-icon" title="Bold (Ctrl+B)" aria-label="Bold" @click="wrapInline('**')">
              <Bold :size="13" :stroke-width="1.75" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Italic (Ctrl+I)" aria-label="Italic" @click="wrapInline('*')">
              <Italic :size="13" :stroke-width="1.75" aria-hidden="true" />
            </button>
            <button class="btn-icon" title="Code" aria-label="Inline code" @click="wrapInline('`')">
              <Code :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
          <div class="editor-actions">
            <button
              class="btn-icon"
              :class="{ pinned: selectedNote.pinned }"
              :aria-label="selectedNote.pinned ? 'Unpin note' : 'Pin note'"
              :title="selectedNote.pinned ? 'Unpin' : 'Pin'"
              @click="togglePin"
            >
              <Pin v-if="!selectedNote.pinned" :size="14" :stroke-width="1.5" aria-hidden="true" />
              <PinOff v-else :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button
              class="btn-icon note-delete"
              aria-label="Delete note"
              title="Delete"
              @click="removeSelected"
            >
              <Trash2 :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
        </header>
        <div class="editor-scroll" @click="onEditorClick">
          <div class="editor-inner">
            <input
              ref="titleEl"
              v-model="titleDraft"
              class="note-title"
              type="text"
              placeholder="Title"
              aria-label="Note title"
              spellcheck="false"
              @input="sync"
              @keydown.enter.prevent="bodyEl?.focus()"
            />
            <p class="edited-at">{{ editedLabel(selectedNote.updated_at) }}</p>
            <div class="editor-stack">
              <pre class="editor-highlight" aria-hidden="true" v-html="highlightHtml"></pre>
              <textarea
                ref="bodyEl"
                v-model="bodyDraft"
                class="note-body"
                placeholder="Start typing…"
                aria-label="Note body"
                @input="sync"
                @keydown="onBodyKeydown"
              ></textarea>
            </div>
          </div>
        </div>
      </template>

      <div v-else-if="notes.notes.length" class="editor-empty empty-state">
        <StickyNote :size="22" :stroke-width="1.5" aria-hidden="true" />
        <p>Select a note</p>
      </div>

      <div v-else class="editor-empty empty-state">
        <StickyNote :size="26" :stroke-width="1.5" aria-hidden="true" />
        <p class="empty-title">No Notes</p>
        <p class="empty-sub">Keep commands, server details and anything worth remembering.</p>
        <button class="btn btn-primary" @click="createNote">
          <SquarePen :size="13" :stroke-width="2" aria-hidden="true" />
          New note
        </button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.notes {
  display: flex;
  height: 100%;
  min-height: 0;
}

/* ---- List column ---- */

.notes-list {
  display: flex;
  flex-direction: column;
  width: 264px;
  flex-shrink: 0;
  min-height: 0;
  border-right: 1px solid var(--border);
}

.list-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-3) var(--space-2) var(--space-2);
  flex-shrink: 0;
}

.search-box {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 5px 9px;
  color: var(--text-3);
  transition: border-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.search-box:hover {
  border-color: var(--border-strong);
}

.search-box:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.search-box input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.search-box input::placeholder {
  color: var(--text-4);
}

.search-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 999px;
  color: var(--text-4);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.search-clear:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.compose {
  flex-shrink: 0;
  color: var(--text-2);
}

.list-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 0 var(--space-2) var(--space-3);
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
}

.list-label {
  margin: var(--space-2) var(--space-1) var(--space-1);
}

.list-scroll > .list-label:first-child {
  margin-top: 0;
}

.note-row {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
  width: 100%;
  padding: 9px 11px;
  border-radius: var(--radius-md);
  text-align: left;
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease;
}

.note-row:hover {
  background: var(--bg-hover);
}

.note-row.selected {
  background: var(--accent-bg);
}

.row-title {
  display: block;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.row-sub {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  font-size: var(--text-sm);
  line-height: 1.45;
  color: var(--text-3);
}

.row-date {
  margin-right: 2px;
}

.list-empty {
  padding: var(--space-6) var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
}

/* ---- Editor column ---- */

.notes-editor {
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

.editor-actions {
  display: flex;
  gap: 2px;
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
}

.note-title::placeholder {
  color: var(--text-4);
}

.edited-at {
  font-size: var(--text-xs);
  color: var(--text-3);
  margin: var(--space-1) 0 var(--space-4);
}

/* Highlight layer + textarea share one cell and one text metric, so the
   styled backdrop aligns exactly under the transparent caret text */
.editor-stack {
  display: grid;
}

.editor-stack > * {
  grid-area: 1 / 1;
  min-width: 0;
  margin: 0;
  padding: 0;
  border: none;
  font-family: var(--font-ui);
  font-size: var(--text-md);
  font-weight: 400;
  line-height: 1.65;
  letter-spacing: normal;
  white-space: pre-wrap;
  overflow-wrap: break-word;
  word-break: normal;
  tab-size: 4;
}

.editor-highlight {
  color: var(--text-1);
  user-select: none;
  pointer-events: none;
}

.hl-marker {
  color: var(--accent);
}

.hl-h1 {
  color: var(--accent);
}

.hl-h2 {
  color: color-mix(in oklch, var(--accent) 62%, var(--text-1));
}

.hl-done {
  color: var(--text-3);
  text-decoration: line-through;
  text-decoration-color: var(--text-4);
}

.note-body {
  color: transparent;
  caret-color: var(--text-1);
  background: transparent;
  outline: none;
  resize: none;
  overflow: hidden;
  min-height: 3em;
}

.note-body::placeholder {
  color: var(--text-4);
}

.note-body::selection {
  background: var(--accent-bg-hover);
}

/* ---- Empty states ---- */

.editor-empty {
  flex: 1;
}

.empty-title {
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  color: var(--text-2);
}

.empty-sub {
  max-width: 36ch;
  font-size: var(--text-sm);
}
</style>
