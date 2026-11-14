// Fish-style suggestions: the rest of the newest command that starts with
// what is typed so far, drawn in grey after the cursor (TerminalPane).

/**
 * What to show after `typed`, or null. History (newest first) wins over
 * snippets; snippets with {{placeholders}} are left out, since their text
 * isn't meant to be run as written.
 */
export function suggestionFor(typed: string, history: readonly string[], snippets: readonly string[]): string | null {
  if (!typed.trim()) return null
  for (const cmd of history) {
    if (cmd.length > typed.length && cmd.startsWith(typed)) return cmd.slice(typed.length)
  }
  for (const cmd of snippets) {
    if (cmd.length > typed.length && cmd.startsWith(typed) && !cmd.includes('\n') && !/\{\{\s*[A-Za-z_]/.test(cmd)) {
      return cmd.slice(typed.length)
    }
  }
  return null
}

/**
 * The next word of a suggestion, for Alt+→: spaces before it included, and a
 * path one folder at a time (`cd /var/www/html` takes `/var/`, then `www/`).
 */
export function nextWord(rest: string): string {
  const m = rest.match(/^\s*\/?[^\s/]*\/?/)
  return m && m[0].trim() ? m[0] : rest
}
