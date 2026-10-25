<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useId, watch } from 'vue'
import { Check, ChevronDown, Search } from 'lucide-vue-next'

export interface ComboOption {
  value: string | number | null
  label: string
  /** Secondary text matched by search, shown right-aligned in mono */
  hint?: string
  /** Dim the label — for the "No group"-style reset entries */
  muted?: boolean
}

const props = withDefaults(
  defineProps<{
    modelValue: string | number | null
    options: ComboOption[]
    /** undefined = show search only when there are more than 7 options */
    searchable?: boolean
    compact?: boolean
  }>(),
  { searchable: undefined, compact: false },
)

defineOptions({ inheritAttrs: false })

const emit = defineEmits<{
  'update:modelValue': [value: string | number | null]
  change: [value: string | number | null]
}>()

const listId = useId()
const open = ref(false)
const query = ref('')
const active = ref(0)
const triggerEl = ref<HTMLButtonElement | null>(null)
const popEl = ref<HTMLDivElement | null>(null)
const searchEl = ref<HTMLInputElement | null>(null)

const showSearch = computed(() => props.searchable ?? props.options.length > 7)
const selected = computed(() => props.options.find((o) => o.value === props.modelValue))
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter(
    (o) =>
      o.label.toLowerCase().includes(q) ||
      (o.hint ?? '').toLowerCase().includes(q),
  )
})

// ---- positioning: anchored to the trigger, flips above when space runs out ----

const popStyle = ref<Record<string, string>>({})
const POP_MARGIN = 6
const POP_MAX_H = 264

function updatePos() {
  const r = triggerEl.value?.getBoundingClientRect()
  const pop = popEl.value
  if (!r || !pop) return
  const h = Math.min(pop.offsetHeight, POP_MAX_H)
  const below = window.innerHeight - r.bottom - POP_MARGIN
  const flip = below < h && r.top > below
  const left = Math.min(Math.max(r.left, 8), Math.max(8, window.innerWidth - r.width - 8))
  popStyle.value = {
    left: `${left}px`,
    top: flip ? `${r.top - h - POP_MARGIN}px` : `${r.bottom + POP_MARGIN}px`,
    minWidth: `${r.width}px`,
    transformOrigin: flip ? 'center bottom' : 'center top',
  }
}

// List height changes as the query filters — keep the popover glued to the trigger
watch(filtered, () => void nextTick(updatePos))

function onScroll(e: Event) {
  if (popEl.value?.contains(e.target as Node)) return
  updatePos()
}

function onPointerDown(e: PointerEvent) {
  const t = e.target as Node
  if (triggerEl.value?.contains(t) || popEl.value?.contains(t)) return
  closePop()
}

function openPop() {
  if (open.value) return
  open.value = true
  query.value = ''
  active.value = Math.max(
    0,
    filtered.value.findIndex((o) => o.value === props.modelValue),
  )
  document.addEventListener('pointerdown', onPointerDown, true)
  window.addEventListener('resize', updatePos)
  window.addEventListener('scroll', onScroll, true)
  void nextTick(() => {
    updatePos()
    if (showSearch.value) searchEl.value?.focus()
    else popEl.value?.focus()
  })
}

function closePop(refocus = false) {
  if (!open.value) return
  open.value = false
  document.removeEventListener('pointerdown', onPointerDown, true)
  window.removeEventListener('resize', updatePos)
  window.removeEventListener('scroll', onScroll, true)
  if (refocus) triggerEl.value?.focus()
}

function choose(i: number) {
  const opt = filtered.value[i]
  if (!opt) return
  emit('update:modelValue', opt.value)
  emit('change', opt.value)
  closePop(true)
}

function move(delta: number) {
  const n = filtered.value.length
  if (!n) return
  active.value = (active.value + delta + n) % n
  void nextTick(() => {
    popEl.value?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' })
  })
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    choose(active.value)
  } else if (e.key === 'Escape') {
    // Consume it here — a bare Escape must not also close the panel/dialog behind
    e.stopPropagation()
    closePop(true)
  }
}

// Arrow keys open the list straight from the closed trigger
function onTriggerKey(e: KeyboardEvent) {
  if (open.value) return
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    openPop()
  }
}

watch(query, () => {
  active.value = 0
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onPointerDown, true)
  window.removeEventListener('resize', updatePos)
  window.removeEventListener('scroll', onScroll, true)
})
</script>

<template>
  <button
    ref="triggerEl"
    v-bind="$attrs"
    type="button"
    class="combo-trigger"
    :class="{ compact, open }"
    :aria-expanded="open"
    aria-haspopup="listbox"
    @click="open ? closePop(true) : openPop()"
    @keydown="onTriggerKey"
  >
    <span class="combo-value" :class="{ muted: selected?.muted }">{{ selected?.label ?? 'Select…' }}</span>
    <ChevronDown :size="14" :stroke-width="1.5" class="combo-chev" :class="{ open }" aria-hidden="true" />
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      ref="popEl"
      class="combo-pop"
      :style="popStyle"
      tabindex="-1"
      @keydown="onKey"
    >
      <div v-if="showSearch" class="combo-search">
        <Search :size="13" :stroke-width="1.5" aria-hidden="true" />
        <input
          ref="searchEl"
          v-model="query"
          type="text"
          placeholder="Search…"
          spellcheck="false"
          aria-label="Search options"
        />
      </div>
      <div class="combo-list" role="listbox" :id="listId">
        <button
          v-for="(o, i) in filtered"
          :key="String(o.value)"
          type="button"
          role="option"
          class="combo-item"
          :class="{ active: i === active, selected: o.value === modelValue }"
          :aria-selected="o.value === modelValue"
          :data-active="i === active"
          @mouseenter="active = i"
          @click="choose(i)"
        >
          <span class="combo-item-label" :class="{ muted: o.muted }">{{ o.label }}</span>
          <span v-if="o.hint" class="combo-item-hint mono">{{ o.hint }}</span>
          <Check :size="13" :stroke-width="2" class="combo-check" aria-hidden="true" />
        </button>
        <div v-if="!filtered.length" class="combo-empty">No matches for “{{ query.trim() }}”</div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.combo-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 7px 10px;
  font-size: var(--text-md);
  color: var(--text-1);
  cursor: pointer;
  text-align: left;
  transition:
    border-color var(--dur-hover) ease,
    background-color var(--dur-hover) ease,
    box-shadow var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.combo-trigger:hover {
  border-color: var(--border-strong);
}

.combo-trigger:active {
  transform: scale(0.98);
}

.combo-trigger:focus-visible,
.combo-trigger.open {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

/* Compact variant — toolbar-sized, mono (SFTP drive switcher) */
.combo-trigger.compact {
  width: auto;
  flex-shrink: 0;
  padding: 5px 7px 5px 9px;
  font-family: var(--font-mono);
  font-size: var(--text-sm);
}

.combo-value {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.combo-value.muted {
  color: var(--text-3);
}

.combo-chev {
  flex-shrink: 0;
  color: var(--text-3);
  transition: transform var(--dur-hover) var(--ease-out);
}

.combo-chev.open {
  transform: rotate(180deg);
}

/* ---- popover: origin-aware scale-in from the trigger, like ContextMenu ---- */

.combo-pop {
  position: fixed;
  z-index: 95;
  display: flex;
  flex-direction: column;
  padding: 5px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  animation: combo-pop-in var(--dur-pop) var(--ease-out);
  outline: none;
}

@keyframes combo-pop-in {
  from {
    opacity: 0;
    transform: scale(0.97);
  }
}

.combo-search {
  display: flex;
  align-items: center;
  gap: 7px;
  margin: -5px -5px 5px;
  padding: 7px 10px;
  border-bottom: 1px solid var(--border);
  color: var(--text-3);
}

.combo-search input {
  flex: 1;
  min-width: 0;
  background: none;
  border: none;
  outline: none;
  font-size: var(--text-base);
  color: var(--text-1);
}

.combo-search input::placeholder {
  color: var(--text-4);
}

.combo-list {
  max-height: 264px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 1px;
}

.combo-item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  color: var(--text-1);
  text-align: left;
  transition: background-color 80ms ease;
}

.combo-item.active {
  background: var(--bg-hover);
}

.combo-item-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.combo-item-label.muted {
  color: var(--text-3);
}

.combo-item-hint {
  flex-shrink: 0;
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.combo-check {
  flex-shrink: 0;
  width: 14px;
  color: var(--accent);
  opacity: 0;
}

.combo-item.selected .combo-check {
  opacity: 1;
}

.combo-empty {
  padding: var(--space-3) var(--space-2);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
}
</style>
