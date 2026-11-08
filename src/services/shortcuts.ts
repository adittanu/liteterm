// Keyboard shortcut registry: action ids, default bindings, event
// normalization and matching. Overrides persist in AppSettings.keybindings
// and are read live from ui.settings everywhere.
import { ref } from 'vue'
import type { ViewId } from '../stores/ui'
import type { AppSettings } from '../types'
import { isMac } from '../utils/platform'

export interface ShortcutDef {
  id: string
  label: string
  group: 'General' | 'Terminal' | 'Navigation' | 'Tabs'
  defaultKeys: string
}

// Same bounds as the font-size slider in Settings
export const FONT_SIZE_MIN = 10
export const FONT_SIZE_MAX = 20

function goTo(n: number): ShortcutDef {
  return { id: `tab${n}`, label: `Go to tab ${n}`, group: 'Tabs', defaultKeys: `Alt+${n}` }
}

// 'notes' -> 'viewNotes' (the action id that opens that view)
export function viewShortcutId(view: ViewId): string {
  return `view${view[0].toUpperCase()}${view.slice(1)}`
}

function view(id: ViewId, label: string, keys: string): ShortcutDef {
  return { id: viewShortcutId(id), label, group: 'Navigation', defaultKeys: keys }
}

export const SHORTCUTS: ShortcutDef[] = [
  { id: 'palette', label: 'Quick connect', group: 'General', defaultKeys: 'Ctrl+K' },
  { id: 'newLocal', label: 'New local terminal', group: 'General', defaultKeys: 'Ctrl+T' },
  { id: 'quickNote', label: 'Quick note', group: 'General', defaultKeys: 'Alt+Q' },
  { id: 'openSettings', label: 'Open settings', group: 'General', defaultKeys: 'Ctrl+,' },
  { id: 'fontSizeUp', label: 'Increase terminal font', group: 'General', defaultKeys: 'Ctrl+Plus' },
  { id: 'fontSizeDown', label: 'Decrease terminal font', group: 'General', defaultKeys: 'Ctrl+-' },
  // Windows Terminal's binding; Go to Port Forwarding moved to Ctrl+Shift+U
  { id: 'terminalFind', label: 'Find in terminal', group: 'Terminal', defaultKeys: 'Ctrl+Shift+F' },
  view('hosts', 'Go to Hosts', 'Ctrl+Shift+H'),
  view('keychain', 'Go to Keychain', 'Ctrl+Shift+K'),
  view('forwards', 'Go to Port Forwarding', 'Ctrl+Shift+U'),
  view('snippets', 'Go to Snippets', 'Ctrl+Shift+P'),
  view('notes', 'Go to Notes', 'Ctrl+Shift+N'),
  view('known', 'Go to Known Hosts', 'Ctrl+Shift+O'),
  view('logs', 'Go to Logs', 'Ctrl+Shift+L'),
  view('sftp', 'Go to SFTP', 'Ctrl+Shift+S'),
  { id: 'nextTab', label: 'Next tab', group: 'Tabs', defaultKeys: 'Ctrl+Tab' },
  { id: 'prevTab', label: 'Previous tab', group: 'Tabs', defaultKeys: 'Ctrl+Shift+Tab' },
  ...[1, 2, 3, 4, 5, 6, 7, 8, 9].map(goTo),
  { id: 'closeTab', label: 'Close active session', group: 'Tabs', defaultKeys: 'Ctrl+W' },
]

// The table above is written with Ctrl, as on Windows and Linux. On macOS the
// app's own commands move to Cmd (stored as "Meta"), where people expect them
// and where Ctrl+<letter> stays free for the shell. Ctrl+Tab is the Mac
// convention too, and Quick note keeps its Option key (Cmd+Q would quit).
export function platformKeys(id: string, keys: string, mac = isMac): string {
  if (!mac || id === 'nextTab' || id === 'prevTab' || id === 'quickNote') return keys
  if (/^tab[1-9]$/.test(id)) return keys.replace(/^Alt\+/, 'Meta+')
  return keys.replace(/^Ctrl\+/, 'Meta+')
}

const defaults: Record<string, string> = Object.fromEntries(
  SHORTCUTS.map((s) => [s.id, platformKeys(s.id, s.defaultKeys)]),
)

/** The built-in binding for this OS (what "Reset" returns to). */
export function defaultFor(id: string): string {
  return defaults[id]
}

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
  // Zoom keys: Ctrl+= and Ctrl++ (Shift+=) are one shortcut, 'Ctrl+Plus'
  if (key === '+' || key === '=') return 'Plus'
  return key.length === 1 ? key.toUpperCase() : key // Tab, F5, ArrowUp…
}

const MAC_GLYPHS: Record<string, string> = { Ctrl: '⌃', Alt: '⌥', Shift: '⇧', Meta: '⌘' }

export function keyLabelFor(part: string, mac: boolean): string {
  if (part === 'Plus') return '+'
  return mac ? (MAC_GLYPHS[part] ?? part) : part
}

// Pretty token for kbd display: the canonical 'Plus' token renders as '+', and
// on a Mac the modifiers become ⌃ ⌥ ⇧ ⌘. One argument on purpose — it is
// passed straight to .map(), which would hand a second one in as the index.
export function keyLabel(part: string): string {
  return keyLabelFor(part, isMac)
}

/** A whole combo as tooltip text: "Ctrl+K" as stored, "⌘K" on a Mac. */
export function comboText(combo: string, mac = isMac): string {
  if (!mac) return combo
  return combo
    .split('+')
    .filter(Boolean)
    .map((part) => keyLabelFor(part, true))
    .join('')
}

// Canonical combo string ("Ctrl+Shift+Tab"), or null when the event can
// never be a shortcut: a lone modifier press, or a key with no modifiers
// that isn't an F-key
export function comboOf(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null
  const parts: string[] = []
  if (e.ctrlKey) parts.push('Ctrl')
  if (e.altKey) parts.push('Alt')
  if (e.metaKey) parts.push('Meta')
  const token = keyToken(e)
  // '+' implies Shift on most layouts — Ctrl+= and Ctrl++ are one combo
  if (e.shiftKey && token !== 'Plus') parts.push('Shift')
  const isFKey = /^F\d+$/.test(e.key)
  if (!parts.length && !isFKey) return null
  parts.push(token)
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
