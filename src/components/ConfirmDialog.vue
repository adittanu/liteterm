<script setup lang="ts">
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'

const dialogs = useDialogsStore()
</script>

<template>
  <BaseModal
    v-if="dialogs.confirmState.open"
    :title="dialogs.confirmState.title"
    :width="400"
    @close="dialogs.answerConfirm(false)"
  >
    <div class="confirm">
      <p class="confirm-message">{{ dialogs.confirmState.message }}</p>
      <footer class="confirm-actions">
        <button class="btn btn-ghost" @click="dialogs.answerConfirm(false)">Cancel</button>
        <button
          class="btn"
          :class="dialogs.confirmState.danger ? 'btn-danger' : 'btn-primary'"
          @click="dialogs.answerConfirm(true)"
        >
          {{ dialogs.confirmState.confirmLabel }}
        </button>
      </footer>
    </div>
  </BaseModal>
</template>

<style scoped>
.confirm {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.confirm-message {
  color: var(--text-2);
  line-height: var(--leading-normal);
}

.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}
</style>
