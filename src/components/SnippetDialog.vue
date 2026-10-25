<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import BaseModal from './BaseModal.vue'
import { useSnippetsStore } from '../stores/extras'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { Snippet } from '../types'

const props = defineProps<{ snippet: Snippet | null }>()
const emit = defineEmits<{ close: [] }>()

const snippets = useSnippetsStore()
const ui = useUiStore()

const name = ref(props.snippet?.name ?? '')
const command = ref(props.snippet?.command ?? '')
const submitted = ref(false)
const saving = ref(false)
const commandEl = ref<HTMLTextAreaElement | null>(null)

// An empty name falls back to the command's first line — one less field to fill
const derivedName = computed(() => {
  const first = command.value.trim().split('\n')[0]?.trim() ?? ''
  return first.length > 40 ? `${first.slice(0, 40)}…` : first
})
const commandError = computed(() => (command.value.trim() ? undefined : 'Enter a command'))
const rows = computed(() => Math.min(10, Math.max(3, command.value.split('\n').length)))

async function save() {
  submitted.value = true
  if (commandError.value || saving.value) return
  saving.value = true
  try {
    await snippets.save(name.value.trim() || derivedName.value, command.value.replace(/\s+$/, ''), props.snippet?.id ?? null)
    emit('close')
  } catch (e) {
    ui.toast('error', `Could not save: ${errorText(e)}`)
  } finally {
    saving.value = false
  }
}

onMounted(() => nextTick(() => commandEl.value?.focus()))
</script>

<template>
  <BaseModal :title="snippet ? 'Edit Snippet' : 'New Snippet'" :width="520" @close="emit('close')">
    <form class="snip-form" novalidate @submit.prevent="save">
      <div class="field">
        <label class="field-label" for="sn-cmd">Command</label>
        <textarea
          id="sn-cmd"
          ref="commandEl"
          v-model="command"
          class="field-input mono cmd-input"
          :class="{ invalid: submitted && commandError }"
          :rows="rows"
          placeholder="sudo systemctl restart nginx"
          spellcheck="false"
          autocapitalize="off"
          @keydown.ctrl.enter.prevent="save"
          @keydown.meta.enter.prevent="save"
        ></textarea>
        <span v-if="submitted && commandError" class="field-error">{{ commandError }}</span>
        <span v-else class="field-hint">Multiple lines run one after another. Ctrl+Enter to save.</span>
      </div>

      <div class="field">
        <label class="field-label" for="sn-name">Name</label>
        <input
          id="sn-name"
          v-model="name"
          class="field-input"
          type="text"
          :placeholder="derivedName || 'Restart nginx'"
          spellcheck="false"
          @keydown.ctrl.enter.prevent="save"
        />
      </div>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="saving">
          {{ saving ? 'Saving…' : snippet ? 'Save changes' : 'Add snippet' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.snip-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.cmd-input {
  resize: vertical;
  min-height: 72px;
  font-size: var(--text-sm);
  line-height: 1.55;
}

.field-input.invalid {
  border-color: var(--danger);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
