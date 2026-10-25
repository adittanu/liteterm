<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { FolderPlus } from 'lucide-vue-next'

const dialogs = useDialogsStore()
const hosts = useHostsStore()
const ui = useUiStore()

const open = computed(() => dialogs.groupDialog.open)
const editing = computed(() => dialogs.groupDialog.group)

const name = ref('')
const error = ref('')
const saving = ref(false)
const nameEl = ref<HTMLInputElement | null>(null)

watch(open, (o) => {
  if (!o) return
  name.value = editing.value?.name ?? ''
  error.value = ''
  saving.value = false
  nextTick(() => nameEl.value?.focus())
})

async function save() {
  const trimmed = name.value.trim()
  if (!trimmed) {
    error.value = 'Give this group a name'
    nameEl.value?.focus()
    return
  }
  saving.value = true
  try {
    await hosts.saveGroup(editing.value?.id ?? null, trimmed)
    dialogs.closeGroupDialog()
  } catch (e) {
    ui.toast('error', `Could not save group: ${errorText(e)}`)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <BaseModal
    v-if="open"
    :title="editing ? 'Rename Group' : 'New Group'"
    :width="400"
    @close="dialogs.closeGroupDialog()"
  >
    <form class="group-form" @submit.prevent="save">
      <div class="field">
        <label class="field-label" for="gd-name">Name</label>
        <input
          id="gd-name"
          ref="nameEl"
          v-model="name"
          class="field-input"
          :class="{ invalid: error }"
          type="text"
          placeholder="e.g. Production"
          spellcheck="false"
        />
        <span v-if="error" class="field-error">{{ error }}</span>
        <span v-else class="field-hint">Hosts inside a group show up when you open it.</span>
      </div>
      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="dialogs.closeGroupDialog()">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="saving">
          <FolderPlus :size="14" :stroke-width="1.5" aria-hidden="true" />
          {{ saving ? 'Saving…' : editing ? 'Save changes' : 'Create group' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.group-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.field-input.invalid {
  border-color: var(--danger);
}

.field-input.invalid:focus {
  box-shadow: 0 0 0 3px var(--danger-bg);
}
</style>
