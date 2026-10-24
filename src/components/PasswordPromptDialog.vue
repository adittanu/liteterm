<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { KeyRound } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useSessionsStore } from '../stores/sessions'

const dialogs = useDialogsStore()
const sessions = useSessionsStore()

const value = ref('')
const submitting = ref(false)

const prompt = computed(() => dialogs.passwordPrompt)

watch(prompt, (p) => {
  if (p) value.value = ''
})

async function submit() {
  if (!prompt.value || !value.value) return
  submitting.value = true
  try {
    const hostId = prompt.value.hostId
    dialogs.closePasswordPrompt()
    await sessions.connect(hostId, value.value)
  } finally {
    submitting.value = false
    value.value = ''
  }
}

function cancel() {
  dialogs.closePasswordPrompt()
}
</script>

<template>
  <BaseModal
    v-if="prompt"
    :title="prompt.kind === 'password' ? 'Password' : 'Key passphrase'"
    :width="380"
    :close-on-scrim="true"
    @close="cancel"
  >
    <form class="prompt" @submit.prevent="submit">
      <p class="prompt-lead">
        <KeyRound :size="14" :stroke-width="1.5" aria-hidden="true" />
        {{ prompt.kind === 'password' ? 'Enter the password for' : 'Enter the passphrase for' }}
        <strong>{{ prompt.label }}</strong>
      </p>
      <input
        v-model="value"
        class="field-input"
        type="password"
        :placeholder="prompt.kind === 'password' ? 'Password' : 'Passphrase'"
        autocomplete="new-password"
        autofocus
      />
      <footer class="prompt-actions">
        <button type="button" class="btn btn-ghost" @click="cancel">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="!value || submitting">
          {{ submitting ? 'Connecting…' : 'Connect' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.prompt {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.prompt-lead {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-base);
  color: var(--text-2);
}

.prompt-lead strong {
  color: var(--text-1);
  font-weight: 500;
}

.prompt-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}
</style>
