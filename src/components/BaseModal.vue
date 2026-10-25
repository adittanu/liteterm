<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { X } from 'lucide-vue-next'

withDefaults(
  defineProps<{
    title: string
    width?: number
    closeOnScrim?: boolean
    /** Render the slot edge-to-edge (no padding, no scroll) — for shells that manage their own panes */
    flush?: boolean
  }>(),
  { width: 480, closeOnScrim: true, flush: false },
)

const emit = defineEmits<{ close: [] }>()

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.stopPropagation()
    emit('close')
  }
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <!-- Modals are occasional → standard animation per motion contract -->
    <Transition name="modal" appear>
      <div class="modal-scrim" @click.self="closeOnScrim && emit('close')">
        <Transition name="modal-content" appear>
          <div
            class="modal surface"
            role="dialog"
            aria-modal="true"
            :aria-label="title"
            :style="{ maxWidth: `${width}px` }"
          >
            <header class="modal-header">
              <h3 class="modal-title">{{ title }}</h3>
              <button class="btn-icon" aria-label="Close dialog" @click="emit('close')">
                <X :size="15" :stroke-width="1.5" aria-hidden="true" />
              </button>
            </header>
            <div class="modal-body" :class="{ flush }">
              <slot />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.modal-scrim {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-5);
}

.modal {
  width: 100%;
  display: flex;
  flex-direction: column;
  max-height: calc(100vh - 2 * var(--space-6));
  /* modal is centered — not origin-anchored, per design contract */
  box-shadow: var(--shadow-3);
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding: var(--space-4) var(--space-5) var(--space-3);
}

.modal-title {
  font-size: var(--text-lg);
  font-weight: 600;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.modal-body {
  padding: 0 var(--space-5) var(--space-5);
  overflow-y: auto;
  min-height: 0;
}

.modal-body.flush {
  padding: 0;
  overflow: hidden;
}
</style>
