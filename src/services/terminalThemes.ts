// Terminal color palettes per app theme (xterm ITheme, hex).
// Official palettes where they exist (Catppuccin), crafted to match
// each theme's accent hues elsewhere.

export interface TerminalPalette {
  bg: string
  fg: string
  cursor: string
  cursorAccent: string
  selectionBackground: string
  black: string
  red: string
  green: string
  yellow: string
  blue: string
  magenta: string
  cyan: string
  white: string
  brightBlack: string
  brightRed: string
  brightGreen: string
  brightYellow: string
  brightBlue: string
  brightMagenta: string
  brightCyan: string
  brightWhite: string
}

export interface ThemeMeta {
  id: string
  name: string
  dark: boolean
  swatches: [string, string, string, string, string] // bg, surface, text, accent, status
}

// bg swatches are the exact rendered values of each theme's --bg token
// (oklch converted to sRGB), so previews tell the truth
export const THEMES: ThemeMeta[] = [
  { id: 'system', name: 'System', dark: true, swatches: ['#090c11', '#242533', '#e8e8ee', '#6c8cff', '#5fd49a'] },
  { id: 'dark', name: 'Dark', dark: true, swatches: ['#090c11', '#1e2029', '#e8e8ee', '#6c8cff', '#5fd49a'] },
  { id: 'light', name: 'Light', dark: false, swatches: ['#f2f3f5', '#ffffff', '#26262e', '#4a6ee0', '#22854f'] },
  {
    id: 'catppuccin-mocha',
    name: 'Catppuccin Mocha',
    dark: true,
    swatches: ['#1e1e2e', '#313244', '#cdd6f4', '#89b4fa', '#a6e3a1'],
  },
  {
    id: 'catppuccin-latte',
    name: 'Catppuccin Latte',
    dark: false,
    swatches: ['#eff1f5', '#ffffff', '#4c4f69', '#1e66f5', '#40a02b'],
  },
  { id: 'vscode-dark', name: 'VS Code Dark', dark: true, swatches: ['#1f1f1f', '#2d2d2d', '#cccccc', '#0078d4', '#23d18b'] },
  { id: 'cursor-dark', name: 'Cursor Dark', dark: true, swatches: ['#141414', '#232323', '#e0e0e0', '#5ab0ff', '#56d364'] },
  { id: 'notion', name: 'Notion', dark: false, swatches: ['#ffffff', '#f7f7f5', '#37352f', '#2383e2', '#448361'] },
  { id: 'claude', name: 'Claude', dark: false, swatches: ['#f5f4ee', '#efede4', '#3d3d3a', '#d97757', '#7d9a68'] },
  { id: 'linear-dark', name: 'Linear Dark', dark: true, swatches: ['#08090a', '#1c1d1f', '#f7f8f8', '#5e6ad2', '#4cb782'] },
]

const PALETTES: Record<string, TerminalPalette> = {
  // Terminal bg = the theme's rendered --bg (exact oklch→sRGB conversion),
  // so the canvas blends into the app chrome with no visible seam
  dark: {
    bg: '#090c11', fg: '#e8e8ee', cursor: '#6c8cff', cursorAccent: '#090c11', selectionBackground: '#6c8cff38',
    black: '#3a3a46', red: '#ff6b6b', green: '#5fd49a', yellow: '#e8c468', blue: '#6c9bff',
    magenta: '#c792ea', cyan: '#5cd6d6', white: '#d6d6de',
    brightBlack: '#5a5a6a', brightRed: '#ff8b8b', brightGreen: '#7ee8b4', brightYellow: '#f5d98a',
    brightBlue: '#8fb5ff', brightMagenta: '#dab3f5', brightCyan: '#7ee3e3', brightWhite: '#f0f0f6',
  },
  light: {
    bg: '#f2f3f5', fg: '#1c1c24', cursor: '#4a6ee0', cursorAccent: '#f2f3f5', selectionBackground: '#4a6ee038',
    black: '#48485a', red: '#c73838', green: '#1d8a54', yellow: '#a06a10', blue: '#2b5fd9',
    magenta: '#8a3fc7', cyan: '#0f7d8c', white: '#b0b0c0',
    brightBlack: '#7a7a8c', brightRed: '#e04a4a', brightGreen: '#28a862', brightYellow: '#c08020',
    brightBlue: '#4a7ae8', brightMagenta: '#a55fd9', brightCyan: '#1f9aa8', brightWhite: '#e8e8f0',
  },
  // Catppuccin official terminal palettes
  'catppuccin-mocha': {
    bg: '#1e1e2e', fg: '#cdd6f4', cursor: '#f5e0dc', cursorAccent: '#1e1e2e', selectionBackground: '#585b7080',
    black: '#45475a', red: '#f38ba8', green: '#a6e3a1', yellow: '#f9e2af', blue: '#89b4fa',
    magenta: '#f5c2e7', cyan: '#94e2d5', white: '#bac2de',
    brightBlack: '#585b70', brightRed: '#f38ba8', brightGreen: '#a6e3a1', brightYellow: '#f9e2af',
    brightBlue: '#89b4fa', brightMagenta: '#f5c2e7', brightCyan: '#94e2d5', brightWhite: '#a6adc8',
  },
  'catppuccin-latte': {
    bg: '#eff1f5', fg: '#4c4f69', cursor: '#dc8a78', cursorAccent: '#eff1f5', selectionBackground: '#9ca0b080',
    black: '#5c5f77', red: '#d20f39', green: '#40a02b', yellow: '#df8e1d', blue: '#1e66f5',
    magenta: '#8839ef', cyan: '#179299', white: '#acb0be',
    brightBlack: '#6c6f85', brightRed: '#d20f39', brightGreen: '#40a02b', brightYellow: '#df8e1d',
    brightBlue: '#1e66f5', brightMagenta: '#8839ef', brightCyan: '#179299', brightWhite: '#acb0be',
  },
  'vscode-dark': {
    bg: '#1f1f1f', fg: '#cccccc', cursor: '#23d18b', cursorAccent: '#1f1f1f', selectionBackground: '#0078d455',
    black: '#3a3a3a', red: '#f14c4c', green: '#23d18b', yellow: '#f5f543', blue: '#3b8eea',
    magenta: '#d670d6', cyan: '#29b8db', white: '#e5e5e5',
    brightBlack: '#666666', brightRed: '#f14c4c', brightGreen: '#23d18b', brightYellow: '#f5f543',
    brightBlue: '#3b8eea', brightMagenta: '#d670d6', brightCyan: '#29b8db', brightWhite: '#e5e5e5',
  },
  'cursor-dark': {
    bg: '#141414', fg: '#dcdcdc', cursor: '#5ab0ff', cursorAccent: '#141414', selectionBackground: '#5ab0ff40',
    black: '#3c3c3c', red: '#f97583', green: '#4ec98a', yellow: '#e8c468', blue: '#5ab0ff',
    magenta: '#c792ea', cyan: '#56b6c2', white: '#d0d0d0',
    brightBlack: '#6a6a6a', brightRed: '#ff8b95', brightGreen: '#6fdc9f', brightYellow: '#f5d98a',
    brightBlue: '#7ec2ff', brightMagenta: '#dab3f5', brightCyan: '#7ad0dc', brightWhite: '#f0f0f0',
  },
  notion: {
    bg: '#ffffff', fg: '#37352f', cursor: '#2383e2', cursorAccent: '#ffffff', selectionBackground: '#2383e233',
    black: '#4b4b47', red: '#e03e3e', green: '#448361', yellow: '#d9730d', blue: '#2383e2',
    magenta: '#9065b0', cyan: '#0f7b8a', white: '#c8c7c2',
    brightBlack: '#787774', brightRed: '#ff5c5c', brightGreen: '#559a74', brightYellow: '#eb8a2f',
    brightBlue: '#4a99ec', brightMagenta: '#a678c4', brightCyan: '#2a97a6', brightWhite: '#ecece9',
  },
  claude: {
    bg: '#262624', fg: '#f5f4ee', cursor: '#d97757', cursorAccent: '#262624', selectionBackground: '#d9775744',
    black: '#46464a', red: '#e0705f', green: '#8fb573', yellow: '#d6a756', blue: '#7ba3cc',
    magenta: '#c594a8', cyan: '#77b3b8', white: '#c8c6bc',
    brightBlack: '#6b6a64', brightRed: '#ec8776', brightGreen: '#a3c788', brightYellow: '#e6ba6e',
    brightBlue: '#8fb3d9', brightMagenta: '#d3a8ba', brightCyan: '#8ac2c7', brightWhite: '#eeece4',
  },
  'linear-dark': {
    bg: '#08090a', fg: '#f7f8f8', cursor: '#5e6ad2', cursorAccent: '#08090a', selectionBackground: '#5e6ad255',
    black: '#2a2c2f', red: '#ec5d5e', green: '#4cb782', yellow: '#f2c94c', blue: '#7a86e0',
    magenta: '#a682ff', cyan: '#4cc3d9', white: '#c9ced4',
    brightBlack: '#5c6068', brightRed: '#f07273', brightGreen: '#5fce94', brightYellow: '#f5d56e',
    brightBlue: '#8d97ea', brightMagenta: '#b69aff', brightCyan: '#66d0e4', brightWhite: '#eef0f2',
  },
}

export function terminalThemeFor(themeSetting: string, systemDark: boolean): TerminalPalette {
  const resolved = themeSetting === 'system' ? (systemDark ? 'dark' : 'light') : themeSetting
  return PALETTES[resolved] ?? PALETTES.dark
}

// xterm's ITheme keys are `background`/`foreground` — the palette's `bg`/`fg`
// names silently no-op if passed straight through, leaving the canvas on
// xterm's default black instead of the theme color.
export function xtermThemeFor(themeSetting: string, systemDark: boolean) {
  const p = terminalThemeFor(themeSetting, systemDark)
  return {
    background: p.bg,
    foreground: p.fg,
    cursor: p.cursor,
    cursorAccent: p.cursorAccent,
    selectionBackground: p.selectionBackground,
    black: p.black,
    red: p.red,
    green: p.green,
    yellow: p.yellow,
    blue: p.blue,
    magenta: p.magenta,
    cyan: p.cyan,
    white: p.white,
    brightBlack: p.brightBlack,
    brightRed: p.brightRed,
    brightGreen: p.brightGreen,
    brightYellow: p.brightYellow,
    brightBlue: p.brightBlue,
    brightMagenta: p.brightMagenta,
    brightCyan: p.brightCyan,
    brightWhite: p.brightWhite,
  }
}
