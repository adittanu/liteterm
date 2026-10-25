<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { StickyNote } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useNotesStore } from '../stores/notes'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { comboFor } from '../services/shortcuts'

const dialogs = useDialogsStore()
const notes = useNotesStore()
const ui = useUiStore()

const title = ref('')
const body = ref('')
const noteId = ref<number | null>(null)
const titleEl = ref<HTMLInputElement | null>(null)
const bodyEl = ref<HTMLTextAreaElement | null>(null)

// The note row is created on the first keystroke, never on open — an
// aborted Quick Note leaves no empty note behind.
let creating = false
let unsent: { title: string; body: string } | null = null

const shortcutHint = computed(() => comboFor('quickNote', ui.settings))

// The component stays mounted next to App.vue, so every open starts
// from a clean slate and hands focus to the title.
watch(
  () => dialogs.showQuickNote,
  (open) => {
    if (!open) return
    title.value = ''
    body.value = ''
    noteId.value = null
    creating = false
    unsent = null
    nextTick(() => titleEl.value?.focus())
  },
)

function close() {
  dialogs.showQuickNote = false
}

function onTitleEnter() {
  bodyEl.value?.focus()
}

async function onInput() {
  if (noteId.value !== null) {
    notes.edit(noteId.value, title.value, body.value)
    autogrow()
    return
  }
  if (!title.value.trim() && !body.value.trim()) return
  if (creating) {
    unsent = { title: title.value, body: body.value }
    return
  }
  creating = true
  const payload = { title: title.value, body: body.value }
  try {
    const note = await notes.create(payload.title, payload.body)
    noteId.value = note.id
    if (unsent && (unsent.title !== payload.title || unsent.body !== payload.body)) {
      notes.edit(note.id, unsent.title, unsent.body)
    }
  } catch (e) {
    ui.toast('error', `Could not create note: ${errorText(e)}`)
  } finally {
    creating = false
  }
}

function autogrow() {
  const el = bodyEl.value
  if (!el) return
  el.style.height = 'auto'
  const max = 320
  const h = Math.min(el.scrollHeight, max)
  el.style.height = `${h}px`
  el.style.overflowY = el.scrollHeight > max ? 'auto' : 'hidden'
}
</script>

<template>
  <BaseModal v-if="dialogs.showQuickNote" title="Quick Note" :width="560" @close="close">
    <div class="quick">
      <input
        ref="titleEl"
        v-model="title"
        class="quick-title"
        type="text"
        placeholder="Title"
        aria-label="Note title"
        spellcheck="false"
        @input="onInput"
        @keydown.enter.prevent="onTitleEnter"
      />
      <textarea
        ref="bodyEl"
        v-model="body"
        class="quick-body"
        placeholder="Write something…"
        aria-label="Note body"
        @input="onInput"
      ></textarea>
      <p class="quick-hint">
        <StickyNote :size="12" :stroke-width="1.5" aria-hidden="true" />
        <span>Saves automatically to Notes · {{ shortcutHint }} opens this from anywhere</span>
      </p>
    </div>
  </BaseModal>
</template>

<style scoped>
.quick {
  display: flex;
  flex-direction: column;
}

.quick-title {
  width: 100%;
  background: transparent;
  border: none;
  outline: none;
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  line-height: var(--leading-tight);
  color: var(--text-1);
  padding: 0;
  margin-bottom: var(--space-2);
}

.quick-title::placeholder {
  color: var(--text-4);
}

.quick-body {
  width: 100%;
  min-height: 140px;
  background: transparent;
  border: none;
  outline: none;
  font-size: var(--text-md);
  line-height: 1.6;
  color: var(--text-1);
  resize: none;
  padding: 0;
}

.quick-body::placeholder {
  color: var(--text-4);
}

.quick-hint {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--border);
  font-size: var(--text-xs);
  color: var(--text-4);
}

.quick-hint svg {
  flex-shrink: 0;
}
</style>
