// Snippet placeholders: `{{name}}`, or `{{name:default}}` with a value to
// start from. A name starts with a letter or underscore, so Go and Helm
// templates (`docker ps --format '{{.Names}}'`, `{{ json . }}`) pass
// through untouched.

const VAR = /\{\{\s*([A-Za-z_][\w-]*)\s*(?::([^{}]*))?\}\}/g

export interface SnippetVar {
  name: string
  /** From `{{name:default}}`; '' when none was given */
  fallback: string
}

/** Each placeholder once, in order of first appearance. */
export function snippetVars(command: string): SnippetVar[] {
  const seen = new Map<string, SnippetVar>()
  for (const m of command.matchAll(VAR)) {
    const name = m[1]
    const fallback = m[2]?.trim() ?? ''
    const known = seen.get(name)
    if (!known) seen.set(name, { name, fallback })
    else if (!known.fallback && fallback) known.fallback = fallback
  }
  return [...seen.values()]
}

/** The command with every placeholder swapped for its value. */
export function fillSnippet(command: string, values: Record<string, string>): string {
  return command.replace(VAR, (_whole, name: string, fallback?: string) => values[name] ?? fallback?.trim() ?? '')
}

// Last values typed per snippet, so a rerun starts from them. Per machine:
// they are often hostnames or branch names of this setup.
const MEMORY_KEY = 'liteterm.snippetVars'

export function rememberedValues(snippetId: number): Record<string, string> {
  try {
    const all = JSON.parse(localStorage.getItem(MEMORY_KEY) || '{}')
    const mine = all[String(snippetId)]
    return mine && typeof mine === 'object' ? mine : {}
  } catch {
    return {}
  }
}

export function rememberValues(snippetId: number, values: Record<string, string>) {
  try {
    const all = JSON.parse(localStorage.getItem(MEMORY_KEY) || '{}')
    all[String(snippetId)] = values
    localStorage.setItem(MEMORY_KEY, JSON.stringify(all))
  } catch {
    // storage full or blocked: the next run just starts blank
  }
}
