<script setup lang="ts">
import { computed, nextTick, reactive, ref, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { fillSnippet, rememberValues, rememberedValues, snippetVars } from '../utils/snippetVars'

// Asks for a snippet's {{placeholders}} right before it runs. Enter in the
// last field (or Ctrl/Cmd+Enter anywhere) sends it; Esc cancels.

const dialogs = useDialogsStore()
const state = computed(() => dialogs.snippetFill)
const vars = computed(() => (state.value ? snippetVars(state.value.snippet.command) : []))
const values = reactive<Record<string, string>>({})
const firstInput = ref<HTMLInputElement[]>([])

watch(
  state,
  (s) => {
    for (const k of Object.keys(values)) delete values[k]
    if (!s) return
    const last = rememberedValues(s.snippet.id)
    for (const v of vars.value) values[v.name] = last[v.name] ?? v.fallback
    void nextTick(() => {
      const el = firstInput.value[0]
      el?.focus()
      el?.select()
    })
  },
  { immediate: true },
)

const preview = computed(() => (state.value ? fillSnippet(state.value.snippet.command, values) : ''))

function submit() {
  const s = state.value
  if (!s) return
  rememberValues(s.snippet.id, { ...values })
  dialogs.answerSnippetFill(preview.value)
}

function onEnter(i: number, e: KeyboardEvent) {
  // Enter moves on to the next value; the last one sends
  if (e.ctrlKey || e.metaKey || i === vars.value.length - 1) return submit()
  const inputs = firstInput.value
  inputs[i + 1]?.focus()
  inputs[i + 1]?.select()
}

function label(name: string): string {
  return name.replace(/[_-]+/g, ' ')
}
</script>

<template>
  <BaseModal
    v-if="state"
    :title="state.snippet.name"
    :width="480"
    @close="dialogs.answerSnippetFill(null)"
  >
    <form class="fill-form" novalidate @submit.prevent="submit">
      <div v-for="(v, i) in vars" :key="v.name" class="field">
        <label class="field-label" :for="`sv-${v.name}`">{{ label(v.name) }}</label>
        <input
          :id="`sv-${v.name}`"
          ref="firstInput"
          v-model="values[v.name]"
          class="field-input mono"
          type="text"
          :placeholder="v.fallback"
          spellcheck="false"
          autocapitalize="off"
          autocomplete="off"
          @keydown.enter.prevent="onEnter(i, $event)"
        />
      </div>

      <div class="field">
        <span class="field-label">Command</span>
        <pre class="fill-preview mono">{{ preview }}</pre>
      </div>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="dialogs.answerSnippetFill(null)">Cancel</button>
        <button type="submit" class="btn btn-primary">{{ state.verb }}</button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.fill-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.field-label {
  text-transform: capitalize;
}

.fill-preview {
  margin: 0;
  padding: var(--space-2) var(--space-3);
  max-height: 160px;
  overflow: auto;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--bg-surface-2);
  font-size: var(--text-sm);
  line-height: 1.55;
  color: var(--text-2);
  white-space: pre-wrap;
  word-break: break-all;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
