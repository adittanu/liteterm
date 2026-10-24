<script setup lang="ts">
import { computed, ref } from 'vue'
import { Plus, Play, Pencil, Trash2, ScrollText } from 'lucide-vue-next'
import { useSnippetsStore } from '../stores/extras'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'

const snippets = useSnippetsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const editingId = ref<number | null>(null)
const name = ref('')
const command = ref('')

const valid = computed(() => name.value.trim() && command.value.trim())

function startCreate() {
  editingId.value = null
  name.value = ''
  command.value = ''
}

function startEdit(id: number, n: string, c: string) {
  editingId.value = id
  name.value = n
  command.value = c
}

function save() {
  if (!valid.value) return
  void snippets
    .save(name.value.trim(), command.value, editingId.value)
    .then(() => startCreate())
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
  const tab = sessions.tabs.find((t) => t.status === 'connected')
  if (!tab) {
    ui.toast('info', 'Connect to a host first, then run snippets from its terminal')
    return
  }
  sessions.write(tab.id, execute ? commandText + '\n' : commandText)
  ui.toast('success', `${execute ? 'Sent' : 'Inserted'} “${commandText.slice(0, 30)}${commandText.length > 30 ? '…' : ''}” to ${tab.label}`)
}

function onRun(e: MouseEvent, commandText: string) {
  sendToActive(commandText, !e.shiftKey)
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h2 class="page-title">Snippets</h2>
        <p class="page-sub">Save commands you type often. Run sends them to a connected terminal.</p>
      </div>
    </header>

    <div class="page-scroll">
      <!-- Editor -->
      <form class="snippet-editor" @submit.prevent="save">
        <div class="editor-row">
          <input
            v-model="name"
            class="field-input"
            type="text"
            placeholder="Snippet name"
            spellcheck="false"
            aria-label="Snippet name"
          />
          <button type="submit" class="btn btn-primary" :disabled="!valid">
            <Plus :size="14" :stroke-width="2" aria-hidden="true" />
            {{ editingId !== null ? 'Update' : 'Add' }}
          </button>
          <button
            v-if="editingId !== null"
            type="button"
            class="btn btn-ghost"
            @click="startCreate"
          >
            Cancel
          </button>
        </div>
        <textarea
          v-model="command"
          class="field-input mono command-input"
          rows="3"
          placeholder="command to run"
          spellcheck="false"
          aria-label="Command"
        ></textarea>
      </form>

      <!-- List -->
      <div v-if="!snippets.snippets.length" class="empty-state page-empty">
        <ScrollText :size="22" :stroke-width="1.5" aria-hidden="true" />
        <p>No snippets yet. Save a command above and run it on any connected host.</p>
      </div>

      <ul v-else class="snippet-list">
        <li v-for="s in snippets.snippets" :key="s.id" class="snippet-card">
          <div class="snippet-info">
            <span class="snippet-name">{{ s.name }}</span>
            <code class="snippet-command text-ellipsis">{{ s.command }}</code>
          </div>
          <div class="snippet-actions">
            <button class="btn-icon" :aria-label="`Run ${s.name}`" title="Run in a connected terminal (Shift+click: insert only)" @click="onRun($event, s.command)">
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
    </div>
  </div>
</template>

<style scoped>
@import '../styles/page.css';

.snippet-editor {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  padding: var(--space-4);
  margin-bottom: var(--space-5);
  box-shadow: var(--shadow-1);
}

.editor-row {
  display: flex;
  gap: var(--space-2);
}

.editor-row .field-input {
  flex: 1;
}

.command-input {
  resize: vertical;
  font-size: var(--text-sm);
}

.snippet-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.snippet-card {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 12px 14px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-1);
  transition: background-color var(--dur-hover) ease;
}

.snippet-card:hover {
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
  font-weight: 600;
  color: var(--text-1);
}

.snippet-command {
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

.snippet-card:hover .snippet-actions,
.snippet-card:focus-within .snippet-actions {
  opacity: 1;
}

.snippet-delete:hover {
  color: var(--danger);
}
</style>
