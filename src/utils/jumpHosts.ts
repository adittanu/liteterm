import type { Host } from '../types'
import { parseHop } from './parseTarget'

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

/**
 * The saved host a `-J` spec names: `user@address:port` (user and port only
 * narrow it down when given), else a host whose label is that name.
 */
export function findSavedHost(hosts: Host[], spec: string): Host | undefined {
  const hop = parseHop(spec)
  if (hop) {
    const hits = hosts
      .filter(
        (h) =>
          h.address.toLowerCase() === hop.host &&
          (hop.port === null || h.port === hop.port) &&
          (hop.username === null || h.username === hop.username),
      )
      .sort((a, b) => (b.last_used_at ?? 0) - (a.last_used_at ?? 0))
    if (hits.length) return hits[0]
  }
  const name = spec.trim().toLowerCase()
  return hosts.find((h) => h.label.trim().toLowerCase() === name || (!!hop && h.label.trim().toLowerCase() === hop.host))
}