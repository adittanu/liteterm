// The command line being typed at a shell prompt, read off the terminal
// screen. No shell integration is assumed: a prompt is a line whose text
// before the cursor ends in a prompt sign. Used by TerminalPane for
// "command finished" notices and for fish-style suggestions.
import type { IBuffer, IMarker } from '@xterm/xterm'

/** The parts of an xterm Terminal this needs (headless works too) */
export interface TermLike {
  readonly cols: number
  readonly buffer: { readonly active: IBuffer }
  registerMarker(cursorYOffset?: number): IMarker | undefined
}

// The sign must be followed by the space every shell prints after it, so a
// progress line ending in "45%" isn't taken for a prompt; cmd's "C:\>" is
// the one prompt without it.
const PROMPT_END = /[$#%>❯➜λ»] $/
const PROMPT_CMD = /^[A-Za-z]:\\[^>]*>$/
const PROMPT_ZSH_ARROW = /^\s*➜\s/ // oh-my-zsh's default theme ends on the folder name

export function isPromptText(before: string): boolean {
  return PROMPT_END.test(before) || PROMPT_CMD.test(before) || PROMPT_ZSH_ARROW.test(before)
}

export function textBeforeCursor(term: TermLike): string {
  const buf = term.buffer.active
  return buf.getLine(buf.baseY + buf.cursorY)?.translateToString(false, 0, buf.cursorX) ?? ''
}

export interface TypedLine {
  /** From the end of the prompt to the cursor */
  text: string
  /**
   * Nothing sits right after the cursor. False when the cursor is mid-line,
   * or the shell draws its own suggestion there (fish, zsh-autosuggestions,
   * PowerShell's prediction).
   */
  free: boolean
  /** Blank cells after the cursor a suggestion may use */
  room: number
}

export class PromptLine {
  // Where the line starts: noted when the shell shows its prompt, cleared by
  // Enter. A marker keeps the row right while scrollback trims; the line may
  // wrap onto the rows below.
  private start: { marker: IMarker; col: number } | null = null

  constructor(private readonly term: TermLike) {}

  get active(): boolean {
    return this.start !== null
  }

  clear() {
    this.start?.marker.dispose()
    this.start = null
  }

  /** Call after the screen changed: notes a fresh prompt, drops a stale one. */
  track() {
    const buf = this.term.buffer.active
    if (buf.type !== 'normal') {
      this.clear()
      return
    }
    const row = buf.baseY + buf.cursorY
    if (this.start) {
      const line = this.start.marker.line
      // Ctrl+C, a cleared screen, a new prompt: the old start no longer applies
      if (this.start.marker.isDisposed || line < 0 || row < line || !this.rowsContinue(line, row)) this.clear()
    }
    if (!this.start && isPromptText(textBeforeCursor(this.term))) {
      const marker = this.term.registerMarker(0)
      if (marker) this.start = { marker, col: buf.cursorX }
    }
  }

  current(): TypedLine | null {
    const start = this.start
    if (!start || start.marker.isDisposed) return null
    const buf = this.term.buffer.active
    if (buf.type !== 'normal') return null
    const first = start.marker.line
    const row = buf.baseY + buf.cursorY
    if (first < 0 || row < first) return null
    let text = ''
    for (let r = first; r <= row; r++) {
      const line = buf.getLine(r)
      if (!line) return null
      text += line.translateToString(false, r === first ? start.col : 0, r === row ? buf.cursorX : this.term.cols)
    }
    const tail = buf.getLine(row)?.translateToString(false, buf.cursorX) ?? ''
    const gap = tail.match(/^ */)![0].length
    const empty = gap === tail.length
    const below = buf.getLine(row + 1)
    const continues = !!below?.isWrapped && below.translateToString(true) !== ''
    // A right-hand prompt (zsh RPROMPT) leaves a wide gap: suggest into it,
    // one space short of it
    const free = !continues && (empty || gap >= 2)
    return { text, free, room: empty ? this.term.cols - buf.cursorX : gap - 1 }
  }

  /** Rows `from`+1 … `to` are wrapped continuations of row `from` */
  private rowsContinue(from: number, to: number): boolean {
    const buf = this.term.buffer.active
    for (let r = from + 1; r <= to; r++) if (!buf.getLine(r)?.isWrapped) return false
    return true
  }
}
