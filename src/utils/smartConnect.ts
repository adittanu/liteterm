// Quick-connect matcher: parses the search box, resolves the target via DNS
// (debounced + cached), and matches it against saved hosts — by exact address,
// by resolved IP (so typing a domain finds a host saved by its IP), or by
// label for a bare name from an ssh command (`ssh prod`).
import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import type { Host } from '../types'
import type { HostPrefill } from '../stores/dialogs'
import { api } from '../services/api'
import { useHostsStore } from '../stores/hosts'
import { editDistance } from './fuzzy'
import { findSavedHost } from './jumpHosts'
import { isIpv4, parseTarget } from './parseTarget'

export interface SmartMatch {
  host: Host
  reason: 'address' | 'ip' | 'label'
  ip?: string
}

const CACHE_TTL = 60_000
const dnsCache = new Map<string, { ips: string[]; ts: number }>()

async function resolveCached(domain: string): Promise<string[]> {
  const hit = dnsCache.get(domain)
  if (hit && Date.now() - hit.ts < CACHE_TTL) return hit.ips
  const ips = (await api.resolveHost(domain)).map((ip) => ip.toLowerCase())
  dnsCache.set(domain, { ips, ts: Date.now() })
  return ips
}

export interface SmartOptions {
  /**
   * Look the target up in DNS. Off for text the user didn't type (the
   * clipboard): a copied file name like `notes.txt` shouldn't reach a resolver.
   */
  resolve?: boolean
}

export function useSmartConnect(query: Readonly<Ref<string>>, opts: SmartOptions = {}) {
  const hosts = useHostsStore()
  const resolve = opts.resolve ?? true

  const parsed = computed(() => parseTarget(query.value))
  const ips = ref<string[]>([])
  const resolving = ref(false)
  const resolveFailed = ref(false)
  // Reverse map (typed-IP case): saved domain address -> resolved IPs
  const domainIps = ref<Record<string, string[]>>({})

  let timer: number | undefined
  let seq = 0

  function schedule(t: ReturnType<typeof parseTarget>, delay: number) {
    clearTimeout(timer)
    const mySeq = ++seq
    ips.value = []
    resolveFailed.value = false
    domainIps.value = {}

    if (!t || !resolve || t.alias) {
      resolving.value = false
      return
    }

    if (t.kind === 'ip') {
      ips.value = [t.host]
      // Which saved domain hosts point at this IP?
      const domains = [...new Set(
        hosts.hosts
          .map((h) => h.address.toLowerCase())
          .filter((a) => !isIpv4(a) && a !== t!.host),
      )]
      if (!domains.length) {
        resolving.value = false
        return
      }
      resolving.value = true
      timer = window.setTimeout(async () => {
        const out: Record<string, string[]> = {}
        await Promise.all(
          domains.map(async (d) => {
            out[d] = await resolveCached(d).catch(() => [])
          }),
        )
        if (mySeq !== seq) return
        domainIps.value = out
        resolving.value = false
      }, delay)
      return
    }

    resolving.value = true
    timer = window.setTimeout(async () => {
      try {
        const result = await resolveCached(t!.host)
        if (mySeq !== seq) return
        ips.value = result
      } catch {
        if (mySeq !== seq) return
        resolveFailed.value = true
      } finally {
        if (mySeq === seq) resolving.value = false
      }
    }, delay)
  }

  // Re-run when the query changes or the host list loads/changes underneath
  watch(
    () => [parseTarget(query.value), hosts.hosts.length] as const,
    ([t]) => schedule(t, 250),
    { immediate: true },
  )

  onBeforeUnmount(() => clearTimeout(timer))

  const matches = computed<SmartMatch[]>(() => {
    const t = parsed.value
    if (!t) return []
    const portOk = (h: Host) => t.port === null || t.port === h.port
    const bareName = t.kind === 'domain' && !t.host.includes('.')
    const out: SmartMatch[] = []
    for (const h of hosts.hosts) {
      const addr = h.address.toLowerCase()
      if (addr === t.host && portOk(h)) {
        out.push({ host: h, reason: 'address' })
      } else if (t.kind === 'domain' && isIpv4(addr) && portOk(h) && ips.value.includes(addr)) {
        out.push({ host: h, reason: 'ip', ip: addr })
      } else if (t.kind === 'ip' && !isIpv4(addr) && portOk(h)) {
        if (domainIps.value[addr]?.includes(t.host)) {
          out.push({ host: h, reason: 'ip', ip: t.host })
        }
      } else if (bareName && h.label.trim().toLowerCase() === t.host && portOk(h)) {
        out.push({ host: h, reason: 'label' })
      }
    }
    const rank = (m: SmartMatch) => (m.reason === 'address' ? 0 : m.reason === 'label' ? 1 : 2)
    return out.sort(
      (a, b) =>
        rank(a) - rank(b) ||
        (b.host.last_used_at ?? 0) - (a.host.last_used_at ?? 0) ||
        a.host.label.localeCompare(b.host.label),
    )
  })

  // `ssh prod` only counts as a target when "prod" is a saved host
  const target = computed(() => {
    const t = parsed.value
    return t?.alias && !matches.value.length ? null : t
  })

  /** The saved host an ssh command's `-J` names, if any. */
  const jump = computed(() => {
    const spec = target.value?.jump
    return spec ? { spec, host: findSavedHost(hosts.hosts, spec) } : null
  })

  /** Fields for "Add & connect" from the current target. */
  function prefill(): HostPrefill | null {
    const t = target.value
    if (!t) return null
    return {
      address: t.host,
      port: t.port ?? 22,
      username: t.username,
      ips: t.kind === 'domain' ? ips.value : [],
      keyPath: t.keyPath ?? null,
      jumpHostId: jump.value?.host?.id ?? null,
    }
  }

  return { target, parsed, ips, resolving, resolveFailed, matches, jump, prefill }
}

/**
 * Saved hosts one or two typos away from what was typed ("prdo-api" →
 * prod-api), for a search that found nothing or an address that didn't
 * resolve. Compares the whole label and address and each longer word in them.
 */
export function suggestHosts(hosts: Host[], typed: string, exclude: Set<number> = new Set(), limit = 3): Host[] {
  const q = typed.trim().toLowerCase()
  if (q.length < 3) return []
  const max = q.length < 5 ? 1 : q.length < 9 ? 2 : 3
  const scored: { host: Host; d: number }[] = []
  for (const h of hosts) {
    if (exclude.has(h.id)) continue
    const label = h.label.trim().toLowerCase()
    const addr = h.address.toLowerCase()
    const fields = new Set([label, addr, `${h.username.toLowerCase()}@${addr}`])
    for (const w of `${label} ${addr}`.split(/[\s._\-@:/]+/)) if (w.length >= 3) fields.add(w)
    let d = max + 1
    for (const f of fields) d = Math.min(d, editDistance(q, f, max))
    if (d <= max) scored.push({ host: h, d })
  }
  return scored
    .sort((a, b) => a.d - b.d || (b.host.last_used_at ?? 0) - (a.host.last_used_at ?? 0))
    .slice(0, limit)
    .map((s) => s.host)
}
