<script setup lang="ts">
import { CheckCircle2, AlertCircle, Info, X } from 'lucide-vue-next'
import { useUiStore } from '../stores/ui'

const ui = useUiStore()

const icons = {
  success: CheckCircle2,
  error: AlertCircle,
  info: Info,
}
</script>

<template>
  <Teleport to="body">
    <div class="toasts" aria-live="polite">
      <TransitionGroup name="toast">
        <div
          v-for="t in ui.toasts"
          :key="t.id"
          class="toast surface"
          :class="t.kind"
          role="status"
        >
          <component :is="icons[t.kind]" :size="15" :stroke-width="1.5" class="toast-icon" aria-hidden="true" />
          <span class="toast-message">{{ t.message }}</span>
          <button
            v-if="t.action"
            class="btn btn-ghost toast-action"
            @click="t.action?.(); ui.dismissToast(t.id)"
          >
            {{ t.actionLabel ?? 'Undo' }}
          </button>
          <button class="btn-icon toast-close" aria-label="Dismiss" @click="ui.dismissToast(t.id)">
            <X :size="12" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toasts {
  position: fixed;
  right: var(--space-4);
  bottom: var(--space-4);
  z-index: 80;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  pointer-events: none;
}

.toast {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 9px 10px 9px 12px;
  min-width: 260px;
  max-width: 380px;
  box-shadow: var(--shadow-pop);
  pointer-events: auto;
  font-size: var(--text-sm);
}

.toast.success .toast-icon { color: var(--success); }
.toast.error .toast-icon { color: var(--danger); }
.toast.info .toast-icon { color: var(--accent); }

.toast-message {
  flex: 1;
  color: var(--text-1);
  line-height: 1.35;
}

.toast-action {
  padding: 3px 8px;
  font-size: var(--text-xs);
  color: var(--accent);
}

.toast-close {
  width: 22px;
  height: 22px;
}
</style>
