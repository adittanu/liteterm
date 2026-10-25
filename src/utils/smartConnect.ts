// Quick-connect matcher: parses the search box, resolves the target via DNS
// (debounced + cached), and matches it against saved hosts — by exact address
// or by resolved IP, so typing a domain finds a host saved by its IP.
import { computed, onBeforeUnmount, ref, watch, type Ref } from 'vue'
import type { Host } from '../types'
import { api } from '../services/api'
import { useHostsStore } from '../stores/hosts'
import { isIpv4, parseTarget } from './parseTarget'

export interface SmartMatch {
  host: Host
  reason: 'address' | 'ip'
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

export function useSmartConnect(query: Ref<string>) {
  const hosts = useHostsStore()

  const target = computed(() => parseTarget(query.value))
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

    if (!t) {
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
    const t = target.value
    if (!t) return []
    const portOk = (h: Host) => t.port === null || t.port === h.port
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
      }
    }
    const rank = (m: SmartMatch) => (m.reason === 'address' ? 0 : 1)
    return out.sort(
      (a, b) =>
        rank(a) - rank(b) ||
        (b.host.last_used_at ?? 0) - (a.host.last_used_at ?? 0) ||
        a.host.label.localeCompare(b.host.label),
    )
  })

  return { target, ips, resolving, resolveFailed, matches }
}
