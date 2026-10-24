<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { Search, Server, SquareTerminal, CornerDownLeft } from 'lucide-vue-next'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { useDialogsStore } from '../stores/dialogs'

const hosts = useHostsStore()
const sessions = useSessionsStore()
const dialogs = useDialogsStore()

const query = ref('')
const activeIndex = ref(0)
const listEl = ref<HTMLElement | null>(null)
const inputEl = ref<HTMLInputElement | null>(null)

// autofocus attribute doesn't fire on dynamically mounted elements
watch(
  () => dialogs.showPalette,
  (open) => {
    if (open) {
      void nextTick(() => {
        inputEl.value?.focus()
        inputEl.value?.select()
      })
    }
  },
)

// Index 0 is the local-terminal action when it's visible
const showLocalAction = computed(() => {
  const q = query.value.trim().toLowerCase()
  return !q || 'local terminal'.includes(q)
})

const results = computed(() => {
  const q = query.value.trim().toLowerCase()
  const all = hosts.hosts
  if (!q) return all
  return all.filter(
    (h) =>
      h.label.toLowerCase().includes(q) ||
      h.address.toLowerCase().includes(q) ||
      h.username.toLowerCase().includes(q),
  )
})

const itemCount = computed(() => results.value.length + (showLocalAction.value ? 1 : 0))

// Palette is keyboard-initiated: mounts and responds instantly, no animation
watch([query, results], () => {
  activeIndex.value = 0
})

function move(delta: number) {
  if (!itemCount.value) return
  activeIndex.value = (activeIndex.value + delta + itemCount.value) % itemCount.value
  void nextTick(() => {
    listEl.value
      ?.querySelector('[data-active="true"]')
      ?.scrollIntoView({ block: 'nearest' })
  })
}

function choose() {
  const i = activeIndex.value
  if (showLocalAction.value && i === 0) {
    close()
    void sessions.connectLocal()
    return
  }
  const host = results.value[i - (showLocalAction.value ? 1 : 0)]
  if (!host) return
  close()
  void sessions.connect(host.id)
}

function close() {
  dialogs.showPalette = false
  query.value = ''
  activeIndex.value = 0
}

function onKey(e: KeyboardEvent) {
  if (!dialogs.showPalette) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    choose()
  } else if (e.key === 'Escape') {
    close()
  }
}

onMounted(() => window.addEventListener('keydown', onKey))
onUnmounted(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div
      v-if="dialogs.showPalette"
      class="palette-scrim"
      @click.self="close"
    >
      <div class="palette surface instant" role="dialog" aria-modal="true" aria-label="Connect to host">
        <div class="palette-search">
          <Search :size="15" :stroke-width="1.5" aria-hidden="true" />
          <input
            ref="inputEl"
            v-model="query"
            class="palette-input"
            type="text"
            placeholder="Type a host name…"
            spellcheck="false"
            aria-label="Search hosts"
          />
        </div>

        <div ref="listEl" class="palette-list" role="listbox" aria-label="Hosts">
          <div v-if="!itemCount" class="palette-empty">No hosts found</div>
          <template v-else>
            <button
              v-if="showLocalAction"
              class="palette-item"
              :class="{ active: activeIndex === 0 }"
              :data-active="activeIndex === 0"
              role="option"
              :aria-selected="activeIndex === 0"
              @mousemove="activeIndex = 0"
              @click="choose"
            >
              <SquareTerminal :size="14" :stroke-width="1.5" aria-hidden="true" />
              <span class="palette-label">Open local terminal</span>
              <span class="palette-addr mono">this PC</span>
              <CornerDownLeft v-if="activeIndex === 0" :size="12" :stroke-width="1.5" class="palette-enter" aria-hidden="true" />
            </button>
            <button
              v-for="(h, i) in results"
              :key="h.id"
              class="palette-item"
              :class="{ active: i + (showLocalAction ? 1 : 0) === activeIndex }"
              :data-active="i + (showLocalAction ? 1 : 0) === activeIndex"
              role="option"
              :aria-selected="i + (showLocalAction ? 1 : 0) === activeIndex"
              @mousemove="activeIndex = i + (showLocalAction ? 1 : 0)"
              @click="choose"
            >
              <Server :size="14" :stroke-width="1.5" aria-hidden="true" />
              <span class="palette-label">{{ h.label }}</span>
              <span class="palette-addr mono">{{ h.username }}@{{ h.address }}</span>
              <CornerDownLeft v-if="i + (showLocalAction ? 1 : 0) === activeIndex" :size="12" :stroke-width="1.5" class="palette-enter" aria-hidden="true" />
            </button>
          </template>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.palette-scrim {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 12vh;
  background: var(--scrim);
}

.palette {
  width: 460px;
  max-width: calc(100vw - 2 * var(--space-5));
  box-shadow: var(--shadow-3);
  overflow: hidden;
  display: flex;
  flex-direction: column;
  max-height: 60vh;
}

.palette-search {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 12px 14px;
  border-bottom: 1px solid var(--border);
  color: var(--text-3);
}

.palette-input {
  flex: 1;
  border: none;
  background: none;
  font-size: var(--text-md);
  color: var(--text-1);
}

.palette-input:focus {
  outline: none;
}

.palette-input::placeholder {
  color: var(--text-4);
}

.palette-list {
  overflow-y: auto;
  padding: 6px;
}

.palette-empty {
  padding: var(--space-5);
  text-align: center;
  color: var(--text-4);
  font-size: var(--text-sm);
}

.palette-item {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  color: var(--text-2);
  text-align: left;
}

.palette-item.active {
  background: var(--accent-bg);
  color: var(--text-1);
}

.palette-label {
  font-weight: 500;
  color: var(--text-1);
}

.palette-addr {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.palette-enter {
  color: var(--text-4);
  flex-shrink: 0;
}
</style>
