// OSC 52: a terminal program asks the terminal to set the clipboard.
//   ESC ] 52 ; <targets> ; <base64 text> BEL
// TUIs such as opencode, Claude Code, Neovim and tmux copy this way, and over
// SSH it is the only way their copy can reach this computer's clipboard.
// xterm.js ignores the sequence unless a handler is registered.

/** The text to put on the clipboard, or null when the request is not a write */
export function osc52Text(data: string): string | null {
  const semi = data.indexOf(';')
  if (semi < 0) return null
  const payload = data.slice(semi + 1).replace(/\s+/g, '')
  // "?" asks the terminal to send the clipboard back. Never answered: a
  // remote host has no business reading this computer's clipboard.
  // An empty payload asks to clear it; leaving the clipboard alone is safer.
  if (payload === '' || payload === '?') return null
  try {
    const bin = atob(payload)
    const bytes = new Uint8Array(bin.length)
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i)
    return new TextDecoder('utf-8', { fatal: false }).decode(bytes)
  } catch {
    // not base64
    return null
  }
}
