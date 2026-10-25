<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Search, X, SquarePen, Pin, StickyNote, LayoutGrid, Rows3 } from 'lucide-vue-next'
import { useNotesStore } from '../stores/notes'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { tagColor } from '../utils/tags'
import type { Note } from '../types'
import NoteEditor from '../components/NoteEditor.vue'

const notes = useNotesStore()
const ui = useUiStore()

type Layout = 'split' | 'gallery'
const layout = ref<Layout>(
  localStorage.getItem('notes.layout') === 'gallery' ? 'gallery' : 'split',
)
watch(layout, (v) => localStorage.setItem('notes.layout', v))

const query = ref('')
const activeTag = ref<string | null>(null)
const selectedId = ref<number | null>(null) // split mode selection
const galleryId = ref<number | null>(null) // gallery mode open note
const editorComp = ref<InstanceType<typeof NoteEditor> | null>(null)

// Pinned first, then most recently edited (SQLite timestamps sort
// lexicographically: 'YYYY-MM-DD HH:MM:SS')
const sorted = computed(() =>
  [...notes.notes].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
    if (a.updated_at !== b.updated_at) return a.updated_at < b.updated_at ? 1 : -1
    return b.id - a.id
  }),
)

const tagList = computed(() => {
  const counts = new Map<string, number>()
  for (const n of notes.notes) for (const t of n.tags) counts.set(t, (counts.get(t) ?? 0) + 1)
  return [...counts.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
})

const searching = computed(() => query.value.trim().length > 0)

const matches = computed(() => {
  let list = sorted.value
  if (activeTag.value) list = list.filter((n) => n.tags.includes(activeTag.value as string))
  if (searching.value) {
    const q = query.value.trim().toLowerCase()
    list = list.filter(
      (n) =>
        n.title.toLowerCase().includes(q) ||
        n.body.toLowerCase().includes(q) ||
        n.tags.some((t) => t.toLowerCase().includes(q)),
    )
  }
  return list
})

// Pinned/Notes sections only in the unfiltered All Notes view — inside
// a search or a tag folder the list reads better flat, like Apple Notes
const sectioned = computed(() => !searching.value && activeTag.value === null)
const pinnedNotes = computed(() =>
  sectioned.value ? matches.value.filter((n) => n.pinned) : [],
)
const otherNotes = computed(() =>
  sectioned.value ? matches.value.filter((n) => !n.pinned) : matches.value,
)

const selectedNote = computed(() => notes.notes.find((n) => n.id === selectedId.value) ?? null)
const galleryNote = computed(() => notes.notes.find((n) => n.id === galleryId.value) ?? null)

onMounted(async () => {
  if (!notes.loaded) await notes.load()
  if (layout.value === 'split' && sorted.value[0]) selectedId.value = sorted.value[0].id
})

function toggleTag(name: string) {
  activeTag.value = activeTag.value === name ? null : name
}

async function createNote() {
  try {
    const note = await notes.create()
    if (layout.value === 'split') selectedId.value = note.id
    else galleryId.value = note.id
    nextTick(() => editorComp.value?.focusTitle())
  } catch (e) {
    ui.toast('error', `Could not create note: ${errorText(e)}`)
  }
}

async function removeNote(id: number) {
  const list = matches.value
  const idx = list.findIndex((n) => n.id === id)
  try {
    const undo = await notes.remove(id)
    ui.toast('info', 'Note deleted', { actionLabel: 'Undo', action: undo, duration: 6000 })
  } catch (e) {
    ui.toast('error', `Could not delete note: ${errorText(e)}`)
    return
  }
  if (layout.value === 'gallery') {
    galleryId.value = null
    return
  }
  const next = list[idx + 1] ?? list[idx - 1] ?? null
  selectedId.value = next ? next.id : null
}

// Escape closes the full-page editor opened from the gallery
function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && layout.value === 'gallery' && galleryId.value !== null) {
    galleryId.value = null
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

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

const MARKER = /^(#{1,6} |[-*] \[[xX ]\] |[-*] |\d+\. |>\s?)/
const SEPARATOR_LINE = /^\s*\|?[-:\s|]+\|?\s*$/

function previewOf(n: Note): string {
  return n.body
    .split('\n')
    .filter((l) => l.trim() !== '' && !SEPARATOR_LINE.test(l))
    .map((l) => l.replace(MARKER, '').replace(/(\*\*|~~|\*|`|\|)/g, ''))
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
}
</script>

<template>
  <div class="notes">
    <Transition name="swap" mode="out-in">
      <!-- ==== Split: list column + editor ==== -->
      <div v-if="layout === 'split'" key="split" class="split-root">
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
            <div class="layout-toggle" role="group" aria-label="Notes layout">
              <button
                class="on"
                aria-label="List layout"
                title="List layout"
                @click="layout = 'split'"
              >
                <Rows3 :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
              <button aria-label="Gallery layout" title="Gallery layout" @click="layout = 'gallery'">
                <LayoutGrid :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
            </div>
            <button class="btn-icon compose" aria-label="New note" title="New note" @click="createNote">
              <SquarePen :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>

          <div class="list-scroll">
            <template v-if="tagList.length">
              <button
                class="tag-row"
                :class="{ on: activeTag === null }"
                @click="activeTag = null"
              >
                <StickyNote :size="12" :stroke-width="1.75" aria-hidden="true" />
                <span class="text-ellipsis">All Notes</span>
                <span class="tag-count">{{ notes.notes.length }}</span>
              </button>
              <p class="section-label list-label">Tags</p>
              <button
                v-for="t in tagList"
                :key="t.name"
                class="tag-row"
                :class="{ on: activeTag === t.name }"
                @click="toggleTag(t.name)"
              >
                <i class="tag-dot" :style="{ background: tagColor(t.name) }" aria-hidden="true"></i>
                <span class="text-ellipsis">{{ t.name }}</span>
                <span class="tag-count">{{ t.count }}</span>
              </button>
            </template>

            <template v-if="pinnedNotes.length || otherNotes.length">
              <p v-if="pinnedNotes.length" class="section-label list-label">Pinned</p>
              <button
                v-for="n in pinnedNotes"
                :key="n.id"
                class="note-row"
                :class="{ selected: n.id === selectedId }"
                @click="selectedId = n.id"
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
                @click="selectedId = n.id"
              >
                <span class="row-title text-ellipsis">{{ n.title || 'New Note' }}</span>
                <span class="row-sub">
                  <span class="row-date">{{ listDate(n.updated_at) }}</span>
                  {{ previewOf(n) || 'No additional text' }}
                </span>
              </button>
            </template>

            <div v-else-if="searching || activeTag" class="list-empty">
              <p v-if="searching">No notes match “{{ query.trim() }}”</p>
              <p v-else>No notes with this tag</p>
            </div>
            <div v-else class="list-empty">
              <p>No notes yet</p>
            </div>
          </div>
        </aside>

        <section class="notes-editor">
          <NoteEditor
            v-if="selectedNote"
            ref="editorComp"
            :note="selectedNote"
            @remove="removeNote(selectedNote.id)"
          />
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

      <!-- ==== Gallery: card grid + full-page editor ==== -->
      <div v-else key="gallery" class="gallery-root">
        <header class="gallery-toolbar">
          <div class="gallery-row">
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
            <div class="gallery-spacer"></div>
            <div class="layout-toggle" role="group" aria-label="Notes layout">
              <button aria-label="List layout" title="List layout" @click="layout = 'split'">
                <Rows3 :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
              <button
                class="on"
                aria-label="Gallery layout"
                title="Gallery layout"
                @click="layout = 'gallery'"
              >
                <LayoutGrid :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
            </div>
            <button class="btn-icon compose" aria-label="New note" title="New note" @click="createNote">
              <SquarePen :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
          <div v-if="tagList.length" class="gallery-tags">
            <button class="gchip" :class="{ on: activeTag === null }" @click="activeTag = null">
              All
            </button>
            <button
              v-for="t in tagList"
              :key="t.name"
              class="gchip"
              :class="{ on: activeTag === t.name }"
              @click="toggleTag(t.name)"
            >
              <i class="tag-dot" :style="{ background: tagColor(t.name) }" aria-hidden="true"></i>
              {{ t.name }}
              <span class="gchip-count">{{ t.count }}</span>
            </button>
          </div>
        </header>

        <div class="gallery-scroll">
          <div v-if="matches.length" class="card-grid">
            <button
              v-for="(n, i) in matches"
              :key="n.id"
              class="card"
              :style="{ '--d': `${Math.min(i, 12) * 28}ms` }"
              @click="galleryId = n.id"
            >
              <span class="card-top">
                <span class="card-title">{{ n.title || 'New Note' }}</span>
                <Pin v-if="n.pinned" class="card-pin" :size="11" :stroke-width="1.75" aria-hidden="true" />
              </span>
              <span class="card-sub">{{ previewOf(n) || 'No additional text' }}</span>
              <span class="card-foot">
                <span class="card-date">{{ listDate(n.updated_at) }}</span>
                <span v-if="n.tags.length" class="card-dots">
                  <i
                    v-for="t in n.tags.slice(0, 4)"
                    :key="t"
                    class="tag-dot"
                    :style="{ background: tagColor(t) }"
                    :title="t"
                    aria-hidden="true"
                  ></i>
                </span>
              </span>
            </button>
          </div>

          <div v-else-if="searching || activeTag" class="list-empty gallery-empty">
            <p v-if="searching">No notes match “{{ query.trim() }}”</p>
            <p v-else>No notes with this tag</p>
          </div>
          <div v-else class="list-empty gallery-empty">
            <StickyNote :size="26" :stroke-width="1.5" aria-hidden="true" />
            <p class="empty-title">No Notes</p>
            <p class="empty-sub">Keep commands, server details and anything worth remembering.</p>
            <button class="btn btn-primary" @click="createNote">
              <SquarePen :size="13" :stroke-width="2" aria-hidden="true" />
              New note
            </button>
          </div>
        </div>

        <Transition name="page">
          <div v-if="galleryNote" class="gallery-editor">
            <NoteEditor
              ref="editorComp"
              :note="galleryNote"
              back
              @remove="removeNote(galleryNote.id)"
              @back="galleryId = null"
            />
          </div>
        </Transition>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.notes {
  position: relative;
  display: flex;
  height: 100%;
  min-height: 0;
}

.split-root,
.gallery-root {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

/* Layout swap crossfade — quick, opacity only */
.swap-enter-active,
.swap-leave-active {
  transition: opacity 130ms ease;
}
.swap-enter-from,
.swap-leave-to {
  opacity: 0;
}

/* ==== shared chrome ==== */

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

.layout-toggle {
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  overflow: hidden;
  flex-shrink: 0;
}

.layout-toggle button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 27px;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.layout-toggle button:hover {
  color: var(--text-1);
  background: var(--bg-hover);
}

.layout-toggle button.on {
  background: var(--accent-bg);
  color: var(--accent);
}

.layout-toggle button + button {
  border-left: 1px solid var(--border);
}

.tag-dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  flex-shrink: 0;
  display: inline-block;
}

.list-empty {
  padding: var(--space-6) var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
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

/* ==== Split mode ==== */

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

.tag-row {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 5px 9px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-2);
  text-align: left;
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.tag-row:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.tag-row.on {
  background: var(--accent-bg);
  color: var(--accent);
}

.tag-count {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-4);
  flex-shrink: 0;
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

.notes-editor {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.editor-empty {
  flex: 1;
}

/* ==== Gallery mode ==== */

.gallery-root {
  position: relative;
  flex-direction: column;
}

.gallery-toolbar {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4) var(--space-2);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.gallery-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.gallery-row .search-box {
  max-width: 320px;
}

.gallery-spacer {
  flex: 1;
}

.gallery-tags {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 5px;
}

.gchip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid transparent;
  font-size: var(--text-xs);
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease,
    border-color var(--dur-hover) ease;
}

.gchip:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.gchip.on {
  background: var(--accent-bg);
  border-color: color-mix(in oklch, var(--accent) 30%, transparent);
  color: var(--accent);
}

.gchip-count {
  font-size: 10px;
  opacity: 0.7;
}

.gallery-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-4);
  min-height: 0;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(215px, 1fr));
  gap: 12px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  text-align: left;
  padding: 13px 14px 11px;
  min-height: 128px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border);
  background: var(--bg-surface);
  overflow: hidden;
  animation: card-in 280ms var(--ease-out) both;
  animation-delay: var(--d, 0ms);
  transition: transform var(--dur-hover) var(--ease-out), border-color var(--dur-hover) ease,
    box-shadow var(--dur-hover) ease;
}

@media (hover: hover) and (pointer: fine) {
  .card:hover {
    transform: translateY(-2px);
    border-color: var(--border-strong);
    box-shadow: var(--shadow-2);
  }
}

.card:active {
  transform: scale(0.98);
}

@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
}

.card-top {
  display: flex;
  align-items: flex-start;
  gap: 6px;
}

.card-title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  line-height: 1.35;
  flex: 1;
  min-width: 0;
}

.card-pin {
  color: var(--accent);
  flex-shrink: 0;
  margin-top: 2px;
}

.card-sub {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-3);
  flex: 1;
}

.card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: auto;
}

.card-date {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.card-dots {
  display: flex;
  gap: 4px;
}

.gallery-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-7) var(--space-4);
}

/* Full-page editor over the grid */
.gallery-editor {
  position: absolute;
  inset: 0;
  z-index: 5;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}

.page-enter-active {
  transition: opacity 200ms var(--ease-out), transform 200ms var(--ease-out);
}

.page-leave-active {
  transition: opacity 130ms ease;
}

.page-enter-from {
  opacity: 0;
  transform: scale(0.985);
}

.page-leave-to {
  opacity: 0;
}

@media (prefers-reduced-motion: reduce) {
  .card {
    animation: none;
  }

  .page-enter-active,
  .page-leave-active,
  .swap-enter-active,
  .swap-leave-active {
    transition: opacity 130ms ease;
  }

  .page-enter-from {
    transform: none;
  }
}
</style>
