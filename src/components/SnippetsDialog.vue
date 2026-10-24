<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { Plus, Play, Pencil, Trash2, X, SquareTerminal } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useSnippetsStore } from '../stores/extras'
import { useSessionsStore } from '../stores/sessions'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'

const dialogs = useDialogsStore()
const snippets = useSnippetsStore()
const sessions = useSessionsStore()
const ui = useUiStore()

const editingId = ref<number | null>(null)
const name = ref('')
const command = ref('')
const nameInput = ref<HTMLInputElement | null>(null)

function startCreate() {
  editingId.value = null
  name.value = ''
  command.value = ''
  void nextTick(() => nameInput.value?.focus())
}

function startEdit(id: number, n: string, c: string) {
  editingId.value = id
  name.value = n
  command.value = c
  void nextTick(() => nameInput.value?.focus())
}

function save() {
  if (!name.value.trim() || !command.value.trim()) return
  void snippets
    .save(name.value.trim(), command.value, editingId.value)
    .then(startCreate)
    .catch((e) => ui.toast('error', `Save failed: ${errorText(e)}`))
}

async function remove(id: number, snippetName: string) {
  const ok = await dialogs.confirm({
    title: `Delete snippet “${snippetName}”?`,
    message: 'This cannot be undone.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  await snippets.remove(id)
}

function sendToActive(commandText: string, execute: boolean) {
  const tab = sessions.tabs.find((t) => t.id === sessions.activeId)
  if (!tab || tab.status !== 'connected') {
    ui.toast('info', 'Connect to a host first, then run snippets from its terminal')
    return
  }
  sessions.write(tab.id, execute ? commandText + '\n' : commandText)
  dialogs.showSnippets = false
}

// Shift+click inserts without executing
function onRun(e: MouseEvent, commandText: string) {
  sendToActive(commandText, !e.shiftKey)
}

watch(
  () => dialogs.showSnippets,
  (o) => {
    if (o) startCreate()
  },
)
</script>

<template>
  <BaseModal
    v-if="dialogs.showSnippets"
    title="Snippets"
    :width="560"
    @close="dialogs.showSnippets = false"
  >
    <div class="snippets">
      <!-- Editor -->
      <form class="snippet-editor" @submit.prevent="save">
        <div class="editor-row">
          <input
            ref="nameInput"
            v-model="name"
            class="field-input"
            type="text"
            placeholder="Snippet name"
            spellcheck="false"
          />
          <button type="submit" class="btn btn-primary" :disabled="!name.trim() || !command.trim()">
            <Plus :size="14" :stroke-width="2" aria-hidden="true" />
            {{ editingId !== null ? 'Update' : 'Add' }}
          </button>
          <button
            v-if="editingId !== null"
            type="button"
            class="btn-icon"
            aria-label="Cancel editing"
            @click="startCreate"
          >
            <X :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
        <textarea
          v-model="command"
          class="field-input mono command-input"
          rows="3"
          placeholder="command to run"
          spellcheck="false"
        ></textarea>
      </form>

      <div class="divider"></div>

      <!-- List -->
      <div v-if="!snippets.snippets.length" class="snippet-empty">
        <SquareTerminal :size="20" :stroke-width="1.5" aria-hidden="true" />
        <p>No snippets yet. Save a command above and run it on any connected host.</p>
      </div>

      <ul v-else class="snippet-list">
        <li v-for="s in snippets.snippets" :key="s.id" class="snippet">
          <div class="snippet-info">
            <span class="snippet-name">{{ s.name }}</span>
            <code class="snippet-command text-ellipsis">{{ s.command }}</code>
          </div>
          <div class="snippet-actions">
            <button class="btn-icon" :aria-label="`Run ${s.name}`" title="Run in active terminal (Shift+click: insert only)" @click="onRun($event, s.command)">
              <Play :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon" :aria-label="`Edit ${s.name}`" title="Edit" @click="startEdit(s.id, s.name, s.command)">
              <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <button class="btn-icon snippet-delete" :aria-label="`Delete ${s.name}`" title="Delete" @click="remove(s.id, s.name)">
              <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
        </li>
      </ul>

      <p class="snippet-hint">
        <strong>Run</strong> executes in the active terminal immediately.
        <strong>Shift+click</strong> inserts the command without executing.
      </p>
    </div>
  </BaseModal>
</template>

<style scoped>
.snippets {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.editor-row {
  display: flex;
  gap: var(--space-2);
  align-items: center;
}

.editor-row .field-input {
  flex: 1;
}

.command-input {
  margin-top: var(--space-2);
  width: 100%;
  resize: vertical;
  font-size: var(--text-sm);
}

.snippet-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-5);
  color: var(--text-4);
  font-size: var(--text-sm);
  text-align: center;
}

.snippet-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 280px;
  overflow-y: auto;
}

.snippet {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 7px 10px;
  border-radius: var(--radius-md);
  transition: background-color var(--dur-hover) ease;
}

.snippet:hover {
  background: var(--bg-hover);
}

.snippet-info {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex: 1;
  min-width: 0;
}

.snippet-name {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
}

.snippet-command {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--text-3);
  user-select: text;
}

.snippet-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.snippet:hover .snippet-actions,
.snippet:focus-within .snippet-actions {
  opacity: 1;
}

.snippet-delete:hover {
  color: var(--danger);
}

.snippet-hint {
  font-size: var(--text-xs);
  color: var(--text-4);
  line-height: var(--leading-normal);
}

.snippet-hint strong {
  color: var(--text-3);
  font-weight: 500;
}
</style>
