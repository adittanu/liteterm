// Keyboard shortcut registry: action ids, default bindings, event
// normalization and matching. Overrides persist in AppSettings.keybindings
// and are read live from ui.settings everywhere.
import { ref } from 'vue'
import type { AppSettings } from '../types'

export interface ShortcutDef {
  id: string
  label: string
  group: 'General' | 'Tabs'
  defaultKeys: string
}

function goTo(n: number): ShortcutDef {
  return { id: `tab${n}`, label: `Go to tab ${n}`, group: 'Tabs', defaultKeys: `Alt+${n}` }
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: 'palette', label: 'Quick connect', group: 'General', defaultKeys: 'Ctrl+K' },
  { id: 'newLocal', label: 'New local terminal', group: 'General', defaultKeys: 'Ctrl+T' },
  { id: 'quickNote', label: 'Quick note', group: 'General', defaultKeys: 'Alt+Q' },
  { id: 'nextTab', label: 'Next tab', group: 'Tabs', defaultKeys: 'Ctrl+Tab' },
  { id: 'prevTab', label: 'Previous tab', group: 'Tabs', defaultKeys: 'Ctrl+Shift+Tab' },
  ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map(goTo),
  { id: 'closeTab', label: 'Close active session', group: 'Tabs', defaultKeys: 'Ctrl+W' },
]

const defaults: Record<string, string> = Object.fromEntries(
  SHORTCUTS.map((s) => [s.id, s.defaultKeys]),
)

// True while the settings dialog listens for a replacement binding;
// global handlers must stand down so captured keys don't fire actions.
export const captureActive = ref(false)

const MODIFIER_KEYS = new Set(['Control', 'Alt', 'Shift', 'Meta'])

export function isModifierOnly(e: KeyboardEvent): boolean {
  return MODIFIER_KEYS.has(e.key)
}

function keyToken(e: KeyboardEvent): string {
  const code = e.code
  if (code.startsWith('Key')) return code.slice(3) // layout-independent letters
  if (code.startsWith('Digit')) return code.slice(5)
  const key = e.key
  if (key === ' ') return 'Space'
  return key.length === 1 ? key.toUpperCase() : key // Tab, F5, ArrowUp…
}

// Canonical combo string ("Ctrl+Shift+Tab"), or null when the event can
// never be a shortcut: a lone modifier press, or a key with no modifiers
// that isn't an F-key
export function comboOf(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null
  const parts: string[] = []
  if (e.ctrlKey) parts.push('Ctrl')
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  if (e.metaKey) parts.push('Meta')
  const isFKey = /^F\d+$/.test(e.key)
  if (!parts.length && !isFKey) return null
  parts.push(keyToken(e))
  return parts.join('+')
}

export function comboFor(id: string, settings: AppSettings): string {
  return settings.keybindings?.[id] || defaults[id]
}

export function isDefault(id: string, settings: AppSettings): boolean {
  return !settings.keybindings?.[id]
}

// Action bound to this key event, or null. TerminalPane runs the same
// lookup so bound combos never leak into the shell as control bytes
// (^K, ESC-prefixed Alt sequences, …).
export function actionFor(e: KeyboardEvent, settings: AppSettings): string | null {
  const combo = comboOf(e)
  if (!combo) return null
  for (const def of SHORTCUTS) {
    if (comboFor(def.id, settings) === combo) return def.id
  }
  return null
}
