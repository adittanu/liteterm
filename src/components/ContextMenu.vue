<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { Check } from 'lucide-vue-next'

export interface MenuItem {
  label: string
  icon?: unknown // Lucide component
  danger?: boolean
  disabled?: boolean
  checked?: boolean // radio-style choice: shows a check on the right
  action: () => void
}

const props = defineProps<{
  x: number
  y: number
  items: MenuItem[]
}>()

const emit = defineEmits<{ close: [] }>()

// Keep the menu inside the viewport
const pos = computed(() => ({
  left: Math.min(props.x, window.innerWidth - 210),
  top: Math.min(props.y, Math.max(10, window.innerHeight - 48 - props.items.length * 32)),
}))

function run(item: MenuItem) {
  if (item.disabled) return
  emit('close')
  item.action()
}

function onWindowClick() {
  emit('close')
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => {
  // Defer so the opening click doesn't immediately close the menu
  setTimeout(() => {
    window.addEventListener('click', onWindowClick)
    window.addEventListener('keydown', onKey)
  }, 0)
})

onUnmounted(() => {
  window.removeEventListener('click', onWindowClick)
  window.removeEventListener('keydown', onKey)
})
</script>

<template>
  <Teleport to="body">
    <div
      class="ctx surface"
      :style="{ left: `${pos.left}px`, top: `${pos.top}px` }"
      role="menu"
      @click.stop
      @contextmenu.prevent
    >
      <button
        v-for="item in items"
        :key="item.label"
        class="ctx-item"
        :class="{ danger: item.danger, disabled: item.disabled }"
        :role="item.checked === undefined ? 'menuitem' : 'menuitemradio'"
        :aria-checked="item.checked"
        :disabled="item.disabled"
        @click="run(item)"
      >
        <component :is="item.icon" v-if="item.icon" :size="14" :stroke-width="1.5" aria-hidden="true" />
        {{ item.label }}
        <Check v-if="item.checked" class="ctx-check" :size="13" :stroke-width="2" aria-hidden="true" />
      </button>
    </div>
  </Teleport>
</template>

<style scoped>
.ctx {
  position: fixed;
  z-index: 90;
  min-width: 180px;
  padding: 5px;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  border: 1px solid var(--border);
  animation: ctx-in 120ms var(--ease-out);
  transform-origin: top left;
}

@keyframes ctx-in {
  from {
    opacity: 0;
    transform: scale(0.97);
  }
}

.ctx-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 6px 10px;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  color: var(--text-1);
  text-align: left;
  transition: background-color var(--dur-hover) ease;
}

.ctx-item:hover:not(.disabled) {
  background: var(--bg-hover);
}

.ctx-item.danger {
  color: var(--danger);
}

.ctx-item.danger:hover:not(.disabled) {
  background: var(--danger-bg);
}

.ctx-item.disabled {
  opacity: 0.45;
  cursor: default;
}

.ctx-check {
  margin-left: auto;
  color: var(--accent);
}
</style>
