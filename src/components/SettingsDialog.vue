<script setup lang="ts">
import { ref, watch } from 'vue'
import { Sun, MoonStar, MonitorCog } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { THEMES } from '../services/terminalThemes'

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
</style>
