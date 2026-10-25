import type { Host } from '../types'

/**
 * Hosts that `selfId` may hop through: everything except itself and any
 * host whose own jump chain already passes through `selfId` (picking one
 * would make a loop). `selfId` null = a host that isn't saved yet.
 */
export function jumpCandidates(hosts: Host[], selfId: number | null): Host[] {
  if (selfId === null) return hosts
  const byId = new Map(hosts.map((h) => [h.id, h]))
  return hosts.filter((h) => {
    if (h.id === selfId) return false
    const seen = new Set<number>()
    let cur: Host | undefined = h
    while (cur?.jump_host_id != null && !seen.has(cur.id)) {
      if (cur.jump_host_id === selfId) return false
      seen.add(cur.id)
      cur = byId.get(cur.jump_host_id)
    }
    return true
  })
}
