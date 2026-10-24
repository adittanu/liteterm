<script setup lang="ts">
import { onUnmounted, ref, watch } from 'vue'
import { Sun, MoonStar, MonitorCog, RotateCcw } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { THEMES } from '../services/terminalThemes'
import {
  SHORTCUTS,
  captureActive,
  comboFor,
  comboOf,
  isDefault,
  isModifierOnly,
  type ShortcutDef,
} from '../services/shortcuts'

const dialogs = useDialogsStore()
const ui = useUiStore()

const fontSize = ref(ui.settings.terminalFontSize)

watch(fontSize, (v) => {
  void ui.updateSettings({ terminalFontSize: v })
})

function iconFor(id: string) {
  if (id === 'system') return MonitorCog
  return id === 'light' ? Sun : MoonStar
}

// ---- keyboard shortcut rebinding ----
const groups = ['General', 'Tabs'] as const
const capturingId = ref<string | null>(null)
const captureError = ref('')

function shortcutsFor(group: (typeof groups)[number]): ShortcutDef[] {
  return SHORTCUTS.filter((s) => s.group === group)
}

function startCapture(id: string) {
  if (capturingId.value === id) {
    cancelCapture()
    return
  }
  capturingId.value = id
  captureError.value = ''
  captureActive.value = true
}

function cancelCapture() {
  capturingId.value = null
  captureError.value = ''
  captureActive.value = false
}

function resetBinding(id: string) {
  const rest = { ...ui.settings.keybindings }
  delete rest[id]
  void ui.updateSettings({ keybindings: rest })
}

function onCaptureKey(e: KeyboardEvent) {
  if (!capturingId.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    cancelCapture()
    return
  }
  if (isModifierOnly(e)) return // mid-combo — wait for the full press
  e.preventDefault()
  e.stopPropagation()
  const combo = comboOf(e)
  const acceptable =
    combo && (e.ctrlKey || e.altKey || e.metaKey || /^F\d+$/.test(e.key))
  if (!acceptable) {
    captureError.value = 'Needs Ctrl or Alt (or an F-key) so typing stays free'
    return
  }
  const clash = SHORTCUTS.find(
    (s) => s.id !== capturingId.value && comboFor(s.id, ui.settings) === combo,
  )
  if (clash) {
    captureError.value = `Already used by “${clash.label}”`
    return
  }
  void ui.updateSettings({
    keybindings: { ...ui.settings.keybindings, [capturingId.value]: combo! },
  })
  cancelCapture()
}

window.addEventListener('keydown', onCaptureKey, true)
onUnmounted(() => {
  window.removeEventListener('keydown', onCaptureKey, true)
  cancelCapture()
})
</script>

<template>
  <BaseModal
    v-if="dialogs.showSettings"
    title="Settings"
    :width="520"
    @close="dialogs.showSettings = false"
  >
    <div class="settings">
      <div class="field">
        <span class="field-label">Theme</span>
        <div class="theme-grid" role="radiogroup" aria-label="Theme">
          <button
            v-for="t in THEMES"
            :key="t.id"
            class="theme-card"
            role="radio"
            :aria-checked="ui.settings.theme === t.id"
            :class="{ on: ui.settings.theme === t.id }"
            @click="ui.updateSettings({ theme: t.id })"
          >
            <span class="theme-preview" :style="{ background: t.swatches[0] }">
              <span class="sw side" :style="{ background: t.swatches[1] }"></span>
              <span class="sw text" :style="{ background: t.swatches[2] }"></span>
              <span class="sw accent" :style="{ background: t.swatches[3] }"></span>
              <span class="sw status" :style="{ background: t.swatches[4] }"></span>
            </span>
            <span class="theme-name">
              <component :is="iconFor(t.id)" v-if="t.id === 'system' || t.id === 'dark' || t.id === 'light'" :size="11" :stroke-width="1.5" aria-hidden="true" />
              {{ t.name }}
            </span>
          </button>
        </div>
      </div>

      <div class="field">
        <label class="field-label" for="set-font">Terminal font size — {{ fontSize }}px</label>
        <input id="set-font" v-model.number="fontSize" type="range" min="10" max="20" step="1" class="range" />
      </div>

      <label class="check">
        <input
          type="checkbox"
          :checked="ui.settings.autoSudoPassword"
          @change="ui.updateSettings({ autoSudoPassword: ($event.target as HTMLInputElement).checked })"
        />
        Auto-fill sudo prompts with the saved password (when the login user matches)
      </label>

      <div class="field">
        <span class="field-label">Keyboard shortcuts</span>
        <div class="kb-groups">
          <div v-for="g in groups" :key="g" class="kb-group">
            <span class="kb-group-label">{{ g }}</span>
            <div v-for="s in shortcutsFor(g)" :key="s.id" class="kb-row">
              <span class="kb-label">{{ s.label }}</span>
              <button
                class="kb-key"
                :class="{
                  capturing: capturingId === s.id,
                  error: capturingId === s.id && captureError,
                }"
                :aria-label="`Change shortcut for ${s.label}`"
                @click="startCapture(s.id)"
              >
                <span v-if="capturingId === s.id" class="kb-hint">{{ captureError || 'Press keys…' }}</span>
                <template v-else>
                  <kbd v-for="(part, i) in comboFor(s.id, ui.settings).split('+')" :key="i">{{ part }}</kbd>
                </template>
              </button>
              <button
                v-if="!isDefault(s.id, ui.settings)"
                class="kb-reset"
                :aria-label="`Reset ${s.label} to default`"
                title="Reset to default"
                @click="resetBinding(s.id)"
              >
                <RotateCcw :size="12" :stroke-width="1.5" aria-hidden="true" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </BaseModal>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.theme-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-2);
}

.theme-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 6px;
  border-radius: var(--radius-md);
  border: 1px solid transparent;
  transition: background-color var(--dur-hover) ease, border-color var(--dur-hover) ease;
}

.theme-card:hover {
  background: var(--bg-hover);
}

.theme-card.on {
  border-color: var(--accent);
  background: var(--accent-bg);
}

.theme-preview {
  position: relative;
  display: block;
  height: 44px;
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid var(--border);
}

.sw {
  position: absolute;
  border-radius: 2px;
}

.sw.side {
  left: 6px;
  top: 6px;
  bottom: 6px;
  width: 12px;
}

.sw.text {
  left: 24px;
  top: 10px;
  width: 20px;
  height: 4px;
}

.sw.accent {
  left: 24px;
  top: 20px;
  width: 14px;
  height: 4px;
}

.sw.status {
  left: 24px;
  top: 30px;
  width: 24px;
  height: 4px;
}

.theme-name {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-2);
  padding: 0 2px;
}

.theme-card.on .theme-name {
  color: var(--text-1);
}

.range {
  accent-color: var(--accent);
  width: 100%;
}

.check {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
}

.kb-groups {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.kb-group {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.kb-group-label {
  font-size: var(--text-xs);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-4);
  padding: 2px 2px 4px;
}

.kb-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 3px 2px;
  border-radius: var(--radius-sm);
}

.kb-label {
  font-size: var(--text-sm);
  color: var(--text-2);
}

.kb-key {
  margin-left: auto;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  min-width: 118px;
  min-height: 26px;
  padding: 3px 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--text-2);
  transition: background-color var(--dur-hover) ease, border-color var(--dur-hover) ease;
}

.kb-key:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.kb-key.capturing {
  border-color: var(--accent);
  border-style: dashed;
  color: var(--text-2);
  justify-content: center;
}

.kb-key.error {
  border-color: var(--danger);
  color: var(--danger);
}

.kb-hint {
  font-size: var(--text-xs);
  white-space: nowrap;
}

.kb-key kbd {
  font-family: inherit;
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-1);
  background: var(--bg-hover);
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 5px;
  padding: 1px 6px;
  min-width: 18px;
  text-align: center;
}

.kb-reset {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  flex-shrink: 0;
  border-radius: var(--radius-sm);
  color: var(--text-4);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.kb-reset:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}
</style>
