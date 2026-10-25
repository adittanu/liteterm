import { useSessionsStore } from '../stores/sessions'
import type { SessionTab } from '../types'

/**
 * The connected terminal a command should go to: a tab on `preferHostId`
 * when one is live, else the last focused terminal, else the newest
 * connected one. Null when nothing is connected.
 */
export function pickRunTarget(preferHostId: number | null): SessionTab | null {
  const sessions = useSessionsStore()
  const live = sessions.tabs.filter((t) => t.status === 'connected')
  if (preferHostId !== null) {
    const onHost = live.find((t) => t.hostId === preferHostId)
    if (onHost) return onHost
  }
  const active = live.find((t) => t.id === sessions.activeId)
  return active ?? live[live.length - 1] ?? null
}

/**
 * Code copied from docs or a console transcript often carries "$ " prompts.
 * Strip them only when every non-empty line has one, so a real `$VAR` line
 * is never touched.
 */
export function stripPrompts(code: string): string {
  const lines = code.replace(/\s+$/, '').split('\n')
  const filled = lines.filter((l) => l.trim() !== '')
  if (filled.length && filled.every((l) => /^\s*\$ /.test(l))) {
    return lines.map((l) => l.replace(/^\s*\$ /, '')).join('\n')
  }
  return lines.join('\n')
}
