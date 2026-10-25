<script setup lang="ts">
import { nextTick, ref } from 'vue'
import { Folder, HardDrive, Pencil } from 'lucide-vue-next'
import type { FileEntry } from '../types'

export interface Crumb {
  name: string
  path: string
}

interface Suggestion {
  name: string
  path: string
  drive?: boolean
}

const props = withDefaults(
  defineProps<{
    path: string
    crumbs: Crumb[]
    /** Lists one directory for autocomplete; null = suggestions off */
    listFn: ((dir: string) => Promise<FileEntry[]>) | null
    drives?: string[]
    sep?: '\\' | '/'
    placeholder?: string
  }>(),
  { drives: () => [], sep: '/', placeholder: 'Type a path…' },
)

const emit = defineEmits<{ navigate: [path: string] }>()

const editing = ref(false)
const draft = ref('')
const inputEl = ref<HTMLInputElement | null>(null)

const suggestions = ref<Suggestion[]>([])
const sugOpen = ref(false)
const active = ref(-1)
const highlightLen = ref(0)
const listing = ref(false)

let debounce = 0
let listToken = 0
const listCache = new Map<string, FileEntry[]>()

function beginEdit() {
  if (editing.value) return
  editing.value = true
  draft.value = props.path
  void nextTick(() => {
    inputEl.value?.focus()
    inputEl.value?.select()
  })
}

function endEdit() {
  editing.value = false
  sugOpen.value = false
  suggestions.value = []
  active.value = -1
}

defineExpose({ beginEdit })

function navigate(path: string) {
  endEdit()
  emit('navigate', path)
}

// ---- autocomplete resolution ----

function splitDraft(): { dir: string; typed: string } | null {
  const norm = props.sep === '\\' ? draft.value.replace(/\//g, '\\') : draft.value
  if (props.sep === '\\') {
    // Partial drive text ("", "C", "c:", "D:\") → offer the drive list
    if (/^[A-Za-z](:\\?)?$/.test(norm) || norm === '') return null
  } else if (norm === '') {
    return null
  }
  const i = norm.lastIndexOf(props.sep)
  if (i === -1) return { dir: props.sep, typed: norm }
  return { dir: norm.slice(0, i + 1), typed: norm.slice(i + 1) }
}

function join(dir: string, name: string): string {
  return dir.endsWith(props.sep) ? dir + name : dir + props.sep + name
}

function resolveSuggestions() {
  const split = splitDraft()

  if (!split) {
    // Drive-level (local) or empty input: suggest roots
    const q = draft.value.trim().toLowerCase()
    const drives = (props.drives ?? []).filter((d) => d.toLowerCase().startsWith(q))
    suggestions.value = drives.map((d) => ({ name: d, path: d, drive: true }))
    highlightLen.value = q.length
    active.value = -1
    sugOpen.value = suggestions.value.length > 0
    listing.value = false
    return
  }

  const { dir, typed } = split
  const q = typed.toLowerCase()
  highlightLen.value = typed.length
  active.value = -1

  if (!props.listFn) {
    suggestions.value = []
    sugOpen.value = false
    return
  }

  const cached = listCache.get(dir.toLowerCase())
  if (cached) {
    applyEntries(cached, q, dir)
    return
  }

  listing.value = true
  sugOpen.value = true
  const token = ++listToken
  props
    .listFn(dir)
    .then((entries) => {
      if (token !== listToken) return
      if (listCache.size > 32) listCache.clear()
      listCache.set(dir.toLowerCase(), entries)
      applyEntries(entries, q, dir)
    })
    .catch(() => {
      if (token !== listToken) return
      suggestions.value = []
      listing.value = false
    })
}

function applyEntries(entries: FileEntry[], q: string, dir: string) {
  suggestions.value = entries
    .filter((e) => (e.kind === 'dir' || e.kind === 'symlink') && e.name.toLowerCase().startsWith(q))
    .sort((a, b) => a.name.localeCompare(b.name))
    .slice(0, 12)
    .map((e) => ({ name: e.name, path: join(dir, e.name) }))
  sugOpen.value = suggestions.value.length > 0
  listing.value = false
}

function onDraftInput() {
  window.clearTimeout(debounce)
  debounce = window.setTimeout(resolveSuggestions, 120)
}

// ---- keyboard ----

function move(delta: number) {
  const n = suggestions.value.length
  if (!n) return
  active.value = active.value === -1 && delta > 0 ? 0 : (active.value + delta + n) % n
  void nextTick(() =>
    document.getElementById('path-sug-active')?.scrollIntoView({ block: 'nearest' }),
  )
}

function completeActive() {
  const s = suggestions.value[active.value === -1 ? 0 : active.value]
  if (!s) return
  const sep = props.sep
  draft.value = s.path.endsWith(sep) ? s.path : s.path + sep
  void nextTick(() => {
    inputEl.value?.setSelectionRange(draft.value.length, draft.value.length)
  })
  resolveSuggestions()
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'Tab') {
    e.preventDefault()
    completeActive()
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const target = suggestions.value[active.value]?.path ?? draft.value.trim()
    if (target) navigate(target)
  } else if (e.key === 'Escape') {
    // Consume it here — a bare Escape must not also close the panel behind
    e.stopPropagation()
    endEdit()
  }
}

function onIdleClick(e: MouseEvent) {
  // Crumb buttons navigate themselves; clicking the bar's empty space edits
  if ((e.target as HTMLElement).closest('button')) return
  beginEdit()
}
</script>

<template>
  <div class="path-bar">
    <Transition name="path-swap" mode="out-in">
      <!-- Idle: breadcrumbs; the whole strip is click-to-edit -->
      <div v-if="!editing" key="crumbs" class="crumbs-row" @click="onIdleClick">
        <template v-if="crumbs.length">
          <template v-for="(c, i) in crumbs" :key="c.path">
            <button
              v-if="i < crumbs.length - 1"
              class="crumb text-ellipsis"
              :title="c.path"
              @click="navigate(c.path)"
            >
              {{ c.name }}
            </button>
            <span v-else class="crumb current text-ellipsis" :title="c.path" aria-current="location">
              {{ c.name }}
            </span>
            <span v-if="i < crumbs.length - 1" class="crumb-sep" aria-hidden="true">›</span>
          </template>
        </template>
        <span v-else class="crumb placeholder">{{ placeholder }}</span>
        <button
          class="crumb-edit"
          :aria-label="`Edit ${placeholder.toLowerCase()}`"
          title="Edit path (Ctrl+L)"
          tabindex="-1"
          @click.stop="beginEdit"
        >
          <Pencil :size="12" :stroke-width="1.5" aria-hidden="true" />
        </button>
      </div>

      <!-- Editing: path input with autocomplete -->
      <div v-else key="input" class="input-wrap">
        <input
          ref="inputEl"
          v-model="draft"
          class="path-input mono"
          type="text"
          :spellcheck="false"
          :autocomplete="'off'"
          aria-label="Path"
          @input="onDraftInput"
          @keydown="onKey"
          @blur="endEdit"
        />
        <span v-if="listing" class="spinner small" aria-hidden="true"></span>

        <Transition name="pop">
          <div v-if="sugOpen && (suggestions.length || listing)" class="sug surface" role="listbox" aria-label="Path suggestions">
            <button
              v-for="(s, i) in suggestions"
              :id="i === active ? 'path-sug-active' : undefined"
              :key="s.path"
              type="button"
              class="sug-item"
              :class="{ active: i === active }"
              role="option"
              :aria-selected="i === active"
              @mousedown.prevent
              @mouseenter="active = i"
              @click="navigate(s.path)"
            >
              <HardDrive v-if="s.drive" :size="14" :stroke-width="1.5" class="sug-icon drive" aria-hidden="true" />
              <Folder v-else :size="14" :stroke-width="1.5" class="sug-icon" aria-hidden="true" />
              <span class="sug-name mono text-ellipsis">
                <span class="sug-typed">{{ s.name.slice(0, highlightLen) }}</span>{{ s.name.slice(highlightLen) }}
              </span>
            </button>
            <div v-if="!suggestions.length && listing" class="sug-empty mono">Listing…</div>
            <div v-else-if="!suggestions.length" class="sug-empty mono">No matching folder</div>
            <div v-if="suggestions.length" class="sug-hint">
              <kbd>Tab</kbd> complete <kbd>Enter</kbd> open
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.path-bar {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
}

/* ---- idle breadcrumbs: the strip reads as one control ---- */

.crumbs-row {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  height: 28px;
  padding: 0 2px;
  border-radius: var(--radius-md);
  cursor: text;
  transition: background-color var(--dur-hover) ease;
}

.crumbs-row:hover {
  background: var(--bg-hover);
}

.crumb {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  color: var(--text-3);
  padding: 3px 5px;
  border-radius: var(--radius-sm);
  max-width: 160px;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

button.crumb:hover {
  background: var(--bg-surface-3);
  color: var(--text-1);
}

.crumb.current {
  color: var(--text-1);
  font-weight: 500;
}

.crumb.placeholder {
  color: var(--text-4);
}

.crumb-sep {
  color: var(--text-4);
  flex-shrink: 0;
}

.crumb-edit {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  margin-left: auto;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  color: var(--text-4);
  opacity: 0;
  cursor: pointer;
  transition:
    opacity var(--dur-hover) ease,
    color var(--dur-hover) ease,
    background-color var(--dur-hover) ease;
}

.crumbs-row:hover .crumb-edit {
  opacity: 1;
}

.crumb-edit:hover {
  color: var(--text-1);
  background: var(--bg-surface-3);
}

/* ---- edit mode ---- */

.input-wrap {
  position: relative;
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  height: 28px;
  padding: 0 8px;
  gap: 6px;
  background: var(--bg-input);
  border: 1px solid var(--accent);
  border-radius: var(--radius-md);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.path-input {
  flex: 1;
  min-width: 0;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-sm);
  color: var(--text-1);
  caret-color: var(--accent);
}

/* ---- suggestions dropdown: scales in from the input's top-left ---- */

.sug {
  position: absolute;
  top: calc(100% + 6px);
  left: 0;
  min-width: 100%;
  max-width: 480px;
  z-index: 40;
  padding: 5px;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  transform-origin: top left;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.sug-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  text-align: left;
  transition: background-color 80ms ease;
}

.sug-item.active {
  background: var(--bg-hover);
}

.sug-icon {
  flex-shrink: 0;
  color: var(--accent);
}

.sug-icon.drive {
  color: var(--text-3);
}

.sug-name {
  min-width: 0;
  color: var(--text-1);
  font-size: var(--text-sm);
}

.sug-typed {
  color: var(--text-4);
}

.sug-empty {
  padding: 8px 10px;
  font-size: var(--text-sm);
  color: var(--text-4);
}

.sug-hint {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 5px 8px 2px;
  font-size: 10px;
  color: var(--text-4);
}

.sug-hint kbd {
  font-family: var(--font-mono);
  background: var(--bg-surface-3);
  border-radius: 3px;
  padding: 0 4px;
}

/* ---- crumb ⇄ input crossfade: opacity only, 100ms ---- */

.path-swap-enter-active,
.path-swap-leave-active {
  transition: opacity 100ms ease;
}

.path-swap-enter-from,
.path-swap-leave-to {
  opacity: 0;
}
</style>
