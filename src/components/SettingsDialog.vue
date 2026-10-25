<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import {
  Sun,
  MoonStar,
  MonitorCog,
  RotateCcw,
  SlidersHorizontal,
  Palette,
  TerminalSquare,
  Keyboard,
} from 'lucide-vue-next'
import { getVersion } from '@tauri-apps/api/app'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { THEMES } from '../services/terminalThemes'
import {
  SHORTCUTS,
  FONT_SIZE_MAX,
  FONT_SIZE_MIN,
  captureActive,
  comboFor,
  comboOf,
  isDefault,
  isModifierOnly,
  keyLabel,
  type ShortcutDef,
} from '../services/shortcuts'

const dialogs = useDialogsStore()
const ui = useUiStore()

// ---- section navigation ----
const SECTIONS = [
  { id: 'general', label: 'General', icon: SlidersHorizontal },
  { id: 'appearance', label: 'Appearance', icon: Palette },
  { id: 'terminal', label: 'Terminal', icon: TerminalSquare },
  { id: 'shortcuts', label: 'Shortcuts', icon: Keyboard },
] as const
type SectionId = (typeof SECTIONS)[number]['id']
const section = ref<SectionId>('general')

const HOST_LAYOUTS = [
  { id: 'modern', name: 'Modern', desc: 'Group chips, fuzzy search, hover Connect & SFTP' },
  { id: 'classic', name: 'Classic', desc: 'Termius-style: group folders you open, then hosts' },
] as const

const appVersion = ref('')
onMounted(() => {
  getVersion()
    .then((v) => (appVersion.value = v))
    .catch(() => {})
})

// ---- terminal ----
// Computed (not a local ref) so the slider tracks font changes made
// through shortcuts while the dialog is closed or open elsewhere
const fontSize = computed({
  get: () => ui.settings.terminalFontSize,
  set: (v) => void ui.updateSettings({ terminalFontSize: v }),
})

function iconFor(id: string) {
  if (id === 'system') return MonitorCog
  return id === 'light' ? Sun : MoonStar
}

// ---- keyboard shortcut rebinding ----
const groups = ['General', 'Terminal', 'Navigation', 'Tabs'] as const
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

const hasCustom = computed(() =>
  SHORTCUTS.some((s) => !isDefault(s.id, ui.settings)),
)

function resetAll() {
  void ui.updateSettings({ keybindings: {} })
}

function onCaptureKey(e: KeyboardEvent) {
  if (!capturingId.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    cancelCapture()
    return
  }
  if (isModifierOnly(e)) {
    // consume mid-combo modifier presses so nothing downstream reacts
    e.preventDefault()
    e.stopPropagation()
    return
  }
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
    :width="700"
    flush
    @close="dialogs.showSettings = false"
  >
    <div class="settings-shell">
      <nav class="settings-nav" aria-label="Settings sections">
        <button
          v-for="s in SECTIONS"
          :key="s.id"
          class="nav-item"
          :class="{ on: section === s.id }"
          :aria-current="section === s.id ? 'true' : undefined"
          @click="section = s.id"
        >
          <component :is="s.icon" :size="15" :stroke-width="1.5" aria-hidden="true" />
          <span>{{ s.label }}</span>
        </button>
        <span v-if="appVersion" class="nav-version">LiteTerm {{ appVersion }}</span>
      </nav>

      <div class="settings-pane">
        <Transition name="section" mode="out-in">
          <!-- ============ General ============ -->
          <div v-if="section === 'general'" key="general" class="pane-page">
            <div class="row">
              <div class="row-text">
                <span class="row-label">Auto-fill sudo password</span>
                <span class="row-desc">
                  Answer sudo password prompts with the host's saved password (only when the
                  login user matches).
                </span>
              </div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="ui.settings.autoSudoPassword"
                  @change="ui.updateSettings({ autoSudoPassword: ($event.target as HTMLInputElement).checked })"
                />
                <span class="track"><span class="thumb"></span></span>
              </label>
            </div>

            <div class="row">
              <div class="row-text">
                <span class="row-label">Reconnect dropped sessions</span>
                <span class="row-desc">
                  When the network drops or the PC wakes from sleep, redial SSH sessions in the
                  same tab — scrollback stays put. Retries for about 3½ minutes.
                </span>
              </div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="ui.settings.autoReconnect"
                  @change="ui.updateSettings({ autoReconnect: ($event.target as HTMLInputElement).checked })"
                />
                <span class="track"><span class="thumb"></span></span>
              </label>
            </div>

            <div class="row">
              <div class="row-text">
                <span class="row-label">Confirm file deletion</span>
                <span class="row-desc">
                  Ask before deleting files or folders in the SFTP file browser.
                </span>
              </div>
              <label class="switch">
                <input
                  type="checkbox"
                  :checked="ui.settings.confirmDeleteFile"
                  @change="ui.updateSettings({ confirmDeleteFile: ($event.target as HTMLInputElement).checked })"
                />
                <span class="track"><span class="thumb"></span></span>
              </label>
            </div>
          </div>

          <!-- ============ Appearance ============ -->
          <div v-else-if="section === 'appearance'" key="appearance" class="pane-page">
            <span class="field-label">Hosts layout</span>
            <div class="layout-grid" role="radiogroup" aria-label="Hosts layout">
              <button
                v-for="l in HOST_LAYOUTS"
                :key="l.id"
                class="layout-card"
                role="radio"
                :aria-checked="ui.settings.hostsLayout === l.id"
                :class="{ on: ui.settings.hostsLayout === l.id }"
                @click="ui.updateSettings({ hostsLayout: l.id })"
              >
                <!-- Miniature of the page, so the choice is visual, not verbal -->
                <span class="lp" :class="l.id" aria-hidden="true">
                  <span class="lp-search"></span>
                  <template v-if="l.id === 'classic'">
                    <span class="lp-row">
                      <span class="lp-folder"></span><span class="lp-folder"></span><span class="lp-folder dashed"></span>
                    </span>
                    <span class="lp-row">
                      <span class="lp-card"></span><span class="lp-card"></span><span class="lp-card"></span>
                    </span>
                  </template>
                  <template v-else>
                    <span class="lp-chips"><span class="on"></span><span></span><span></span><span></span></span>
                    <span class="lp-row">
                      <span class="lp-card live"><i></i></span><span class="lp-card"><i></i></span><span class="lp-card"><i></i></span>
                    </span>
                    <span class="lp-row">
                      <span class="lp-card"><i></i></span><span class="lp-card"><i></i></span><span class="lp-card ghost"></span>
                    </span>
                  </template>
                </span>
                <span class="layout-text">
                  <span class="layout-name">{{ l.name }}</span>
                  <span class="layout-desc">{{ l.desc }}</span>
                </span>
              </button>
            </div>

            <span class="field-label theme-label">Theme</span>
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
                  <component
                    :is="iconFor(t.id)"
                    v-if="t.id === 'system' || t.id === 'dark' || t.id === 'light'"
                    :size="11"
                    :stroke-width="1.5"
                    aria-hidden="true"
                  />
                  {{ t.name }}
                </span>
              </button>
            </div>
          </div>

          <!-- ============ Terminal ============ -->
          <div v-else-if="section === 'terminal'" key="terminal" class="pane-page">
            <div class="row">
              <div class="row-text">
                <span class="row-label">Font size</span>
                <span class="row-desc">{{ fontSize }} px</span>
              </div>
              <input
                v-model.number="fontSize"
                type="range"
                :min="FONT_SIZE_MIN"
                :max="FONT_SIZE_MAX"
                step="1"
                class="range"
                aria-label="Terminal font size"
              />
            </div>
            <div class="font-preview" :style="{ fontSize: `${fontSize}px` }">
              user@web-01:~$ systemctl status nginx
            </div>

            <div class="row">
              <div class="row-text">
                <span class="row-label">GPU text rendering (WebGL)</span>
                <span class="row-desc">
                  Off uses the browser's own text renderer, closer to Windows Terminal's
                  thinner glyphs but heavier on CPU for full-screen TUIs.
                </span>
              </div>
              <label class="switch">
                <input
                  v-model="ui.terminalRender.webgl"
                  type="checkbox"
                  aria-label="GPU text rendering"
                />
                <span class="track"><span class="thumb"></span></span>
              </label>
            </div>

            <div class="row">
              <div class="row-text">
                <span class="row-label">Bold text uses bold weight</span>
                <span class="row-desc">
                  Draw SGR bold with the heavy face, as Windows Terminal does here.
                </span>
              </div>
              <label class="switch">
                <input
                  v-model="ui.terminalRender.realBold"
                  type="checkbox"
                  aria-label="Bold text uses bold weight"
                />
                <span class="track"><span class="thumb"></span></span>
              </label>
            </div>
          </div>

          <!-- ============ Shortcuts ============ -->
          <div v-else key="shortcuts" class="pane-page">
            <div class="kb-head">
              <span class="field-label">Keyboard shortcuts</span>
              <button v-if="hasCustom" class="kb-reset-all" @click="resetAll">
                Reset all
              </button>
            </div>
            <div class="kb-groups">
              <div v-for="g in groups" :key="g" class="kb-group">
                <span class="kb-group-label">{{ g }}</span>
                <div v-for="s in shortcutsFor(g)" :key="s.id" class="kb-row">
                  <div class="row-text">
                    <span class="row-label">{{ s.label }}</span>
                    <span v-if="!isDefault(s.id, ui.settings)" class="row-desc">
                      Default: {{ s.defaultKeys }}
                    </span>
                  </div>
                  <button
                    class="kb-key"
                    :class="{
                      capturing: capturingId === s.id,
                      error: capturingId === s.id && captureError,
                    }"
                    :aria-label="`Change shortcut for ${s.label}`"
                    @click="startCapture(s.id)"
                  >
                    <span v-if="capturingId === s.id" class="kb-hint">
                      {{ captureError || 'Press keys — Esc to cancel' }}
                    </span>
                    <template v-else>
                      <kbd v-for="(part, i) in comboFor(s.id, ui.settings).split('+')" :key="i">{{ keyLabel(part) }}</kbd>
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
        </Transition>
      </div>
    </div>
  </BaseModal>
</template>

<style scoped>
.settings-shell {
  display: flex;
  height: min(460px, calc(100vh - 220px));
  min-height: 300px;
}

/* ---- sidebar ---- */
.settings-nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 168px;
  flex-shrink: 0;
  padding: var(--space-2) var(--space-2) var(--space-3);
  background: var(--bg);
  border-right: 1px solid var(--border);
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-2);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.nav-item.on {
  background: var(--bg-surface-3);
  color: var(--text-1);
}

.nav-version {
  margin-top: auto;
  padding: var(--space-2) var(--space-2) 0;
  font-size: var(--text-xs);
  color: var(--text-4);
}

/* ---- content pane ---- */
.settings-pane {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: var(--space-4) var(--space-5);
}

.pane-page {
  display: flex;
  flex-direction: column;
}

.section-enter-active,
.section-leave-active {
  transition: opacity 120ms ease;
}

.section-enter-from,
.section-leave-to {
  opacity: 0;
}

/* ---- setting rows ---- */
.row {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: 12px 0;
}

.row + .row {
  border-top: 1px solid var(--border);
}

.row-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.row-label {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-1);
}

.row-desc {
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: 1.45;
}

.row .range {
  margin-left: auto;
  width: 180px;
  flex-shrink: 0;
  accent-color: var(--accent);
}

/* ---- toggle switch ---- */
.switch {
  position: relative;
  display: inline-flex;
  width: 36px;
  height: 20px;
  flex-shrink: 0;
  margin-left: auto;
  cursor: pointer;
}

.switch input {
  position: absolute;
  inset: 0;
  margin: 0;
  opacity: 0;
  cursor: pointer;
}

.switch .track {
  position: absolute;
  inset: 0;
  border-radius: 999px;
  background: var(--bg-active);
  transition: background-color var(--dur-hover) ease;
}

.switch .thumb {
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: oklch(1 0 0);
  box-shadow: 0 1px 2px oklch(0 0 0 / 0.3);
  transition: transform var(--dur-hover) ease;
}

.switch input:checked ~ .track {
  background: var(--accent);
}

.switch input:checked ~ .track .thumb {
  transform: translateX(16px);
}

.switch input:focus-visible ~ .track {
  outline: 2px solid var(--border-focus);
  outline-offset: 2px;
}

/* ---- appearance ---- */
.layout-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
  margin: var(--space-2) 0 var(--space-5);
}

.layout-card {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 8px 8px 10px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border);
  text-align: left;
  transition:
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease,
    box-shadow var(--dur-hover) ease,
    transform var(--dur-press) ease-out;
}

.layout-card:hover {
  background: var(--bg-hover);
}

.layout-card:active {
  transform: scale(0.98);
}

.layout-card.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.layout-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 2px;
}

.layout-name {
  font-size: var(--text-sm);
  font-weight: 600;
  color: var(--text-1);
}

.layout-desc {
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: 1.4;
}

/* Miniature page preview — 6px radius inside the 12px card (8px inset) */
.lp {
  display: flex;
  flex-direction: column;
  gap: 5px;
  height: 92px;
  padding: 8px;
  border-radius: 6px;
  background: var(--bg);
  border: 1px solid var(--border);
  overflow: hidden;
}

.lp-search {
  height: 9px;
  border-radius: 3px;
  background: var(--bg-input);
  border: 1px solid var(--border-strong);
}

.lp-row {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 4px;
}

.lp-folder,
.lp-card {
  position: relative;
  height: 17px;
  border-radius: 4px;
  background: var(--bg-surface);
  border: 1px solid var(--border);
}

.lp-folder::before {
  content: '';
  position: absolute;
  left: 3px;
  top: 4px;
  width: 7px;
  height: 7px;
  border-radius: 2px;
  background: var(--accent-bg-hover);
}

.lp-folder.dashed {
  border-style: dashed;
  background: transparent;
}

.lp-folder.dashed::before {
  background: var(--bg-surface-3);
}

.lp.classic .lp-row + .lp-row {
  margin-top: 4px;
}

.lp-card.ghost {
  opacity: 0;
}

/* Modern: the play button and presence dot are the tell */
.lp-card i {
  position: absolute;
  right: 3px;
  top: 4px;
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
  opacity: 0.85;
}

.lp-card.live::before {
  content: '';
  position: absolute;
  left: 3px;
  bottom: 2px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: var(--success);
}

.lp-chips {
  display: flex;
  gap: 3px;
}

.lp-chips span {
  width: 16px;
  height: 7px;
  border-radius: 999px;
  border: 1px solid var(--border-strong);
}

.lp-chips span.on {
  background: var(--accent-bg-hover);
  border-color: var(--accent);
}

.theme-label {
  display: block;
}

.theme-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-2);
  margin-top: var(--space-2);
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
  height: 48px;
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
  top: 11px;
  width: 20px;
  height: 4px;
}

.sw.accent {
  left: 24px;
  top: 22px;
  width: 14px;
  height: 4px;
}

.sw.status {
  left: 24px;
  top: 33px;
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

/* ---- terminal preview ---- */
.font-preview {
  margin-top: var(--space-4);
  padding: var(--space-4);
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  border: 1px solid var(--border);
  font-family: var(--font-mono);
  color: var(--text-2);
  line-height: 1.45;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* ---- shortcuts ---- */
.kb-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-3);
}

.kb-reset-all {
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-3);
  padding: 4px 8px;
  border-radius: var(--radius-sm);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.kb-reset-all:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.kb-groups {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.kb-group {
  display: flex;
  flex-direction: column;
}

.kb-group-label {
  font-size: var(--text-xs);
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-4);
  padding: 0 2px 6px;
}

.kb-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: 7px 2px;
}

.kb-row + .kb-row {
  border-top: 1px solid var(--border);
}

.kb-key {
  margin-left: auto;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 5px;
  min-width: 128px;
  min-height: 28px;
  padding: 4px 9px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  color: var(--text-2);
  transition:
    background-color var(--dur-hover) ease,
    border-color var(--dur-hover) ease,
    color var(--dur-hover) ease;
}

.kb-key:hover {
  background: var(--bg-hover);
  border-color: var(--border-strong);
  color: var(--text-1);
}

.kb-key.capturing {
  justify-content: center;
  min-width: 180px;
  border: 1px dashed var(--accent);
  background: var(--accent-bg);
}

.kb-key.error {
  border-color: var(--danger);
}

.kb-hint {
  font-size: var(--text-xs);
  color: var(--text-2);
  white-space: nowrap;
}

.kb-key.error .kb-hint {
  color: var(--danger);
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
