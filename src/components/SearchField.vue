<script setup lang="ts">
import { ref } from 'vue'
import { Search, X } from 'lucide-vue-next'

// Page-level filter field. Escape clears first, then blurs — so the key
// always does something predictable and never closes anything behind it.
defineProps<{ placeholder?: string; label: string }>()
const model = defineModel<string>({ required: true })
const input = ref<HTMLInputElement | null>(null)

function onEscape(e: KeyboardEvent) {
  if (model.value) {
    e.stopPropagation()
    model.value = ''
  } else {
    input.value?.blur()
  }
}

function clear() {
  model.value = ''
  input.value?.focus()
}

defineExpose({ focus: () => input.value?.focus() })
</script>

<template>
  <div class="search-field">
    <Search :size="14" :stroke-width="1.5" class="search-icon" aria-hidden="true" />
    <input
      ref="input"
      v-model="model"
      type="text"
      :placeholder="placeholder ?? 'Search'"
      :aria-label="label"
      spellcheck="false"
      autocomplete="off"
      @keydown.esc="onEscape"
    />
    <button v-if="model" type="button" class="search-clear" aria-label="Clear search" @click="clear">
      <X :size="11" :stroke-width="2" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.search-field {
  display: flex;
  align-items: center;
  gap: 7px;
  min-width: 0;
  height: 32px;
  padding: 0 8px 0 10px;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  color: var(--text-4);
  transition: border-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.search-field:hover {
  border-color: var(--border-strong);
}

.search-field:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
  color: var(--text-3);
}

.search-icon {
  flex-shrink: 0;
}

input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: var(--text-base);
  color: var(--text-1);
}

input::placeholder {
  color: var(--text-4);
}

.search-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 999px;
  background: var(--bg-surface-3);
  color: var(--text-2);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.search-clear:hover {
  background: var(--bg-active);
  color: var(--text-1);
}

.search-clear:active {
  transform: scale(0.9);
}
</style>
