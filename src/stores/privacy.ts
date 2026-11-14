// Privacy mode: hide host names, addresses, logins and group names on screen
// so a screenshot can be shared. Display only — stored data never changes.
//
// Two styles:
//   dummy — every known value is swapped for a stable stand-in (server-01,
//           Group A, user2, 10.0.0.5…), so a screenshot still reads naturally
//   blur  — the real text stays in place but is blurred (class "pv")
//
// Per-machine (localStorage), like terminalRender: it describes this screen,
// not the user's data, and must never ride along with cloud sync.
import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { useHostsStore } from './hosts'
import { useCredentialsStore } from './credentials'

export type PrivacyStyle = 'dummy' | 'blur'

export interface PvSegment {
  text: string
  secret: boolean
}

const STORAGE_KEY = 'liteterm.privacy'

// Logins that say nothing about whose server it is; masking them would only
// make screenshots look less real.
const GENERIC_USERS = new Set([
  'root', 'admin', 'administrator', 'user', 'ubuntu', 'debian', 'centos', 'fedora',
  'ec2-user', 'opc', 'azureuser', 'pi', 'alpine', 'arch', 'rocky', 'almalinux', 'core', 'git',
])

// Addresses that identify nobody
const PUBLIC_ADDRESS = /^(localhost|127(\.\d{1,3}){3}|0\.0\.0\.0|::1?)$/i

const IPV4 = String.raw`(?:\d{1,3}\.){3}\d{1,3}`
const IPV4_ONLY = new RegExp(`^${IPV4}$`)

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

// FNV-1a: a stable stand-in for values we have no table entry for
function hash(s: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 0x01000193) >>> 0
  }
  return h
}

function letters(n: number): string {
  // 0 -> A … 25 -> Z, 26 -> AA …
  let s = ''
  n += 1
  while (n > 0) {
    const r = (n - 1) % 26
    s = String.fromCharCode(65 + r) + s
    n = Math.floor((n - 1) / 26)
  }
  return s
}

function fakeIp(n: number): string {
  // 10.0.0.1 … 10.0.0.254, 10.0.1.1 …
  return `10.0.${Math.floor(n / 254)}.${(n % 254) + 1}`
}

function readStored(): { enabled: boolean; style: PrivacyStyle } {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) || '{}')
    return {
      enabled: raw.enabled === true,
      style: raw.style === 'blur' ? 'blur' : 'dummy',
    }
  } catch {
    return { enabled: false, style: 'dummy' }
  }
}

export const usePrivacyStore = defineStore('privacy', () => {
  const hosts = useHostsStore()
  const credentials = useCredentialsStore()

  const stored = readStored()
  const enabled = ref(stored.enabled)
  const style = ref<PrivacyStyle>(stored.style)

  const dummy = computed(() => enabled.value && style.value === 'dummy')
  const blur = computed(() => enabled.value && style.value === 'blur')

  watch(
    [enabled, style],
    ([on, s]) => {
      localStorage.setItem(STORAGE_KEY, JSON.stringify({ enabled: on, style: s }))
      // CSS hook: html[data-privacy="blur"] .pv { filter: blur() }
      const root = document.documentElement
      if (on) root.dataset.privacy = s
      else delete root.dataset.privacy
    },
    { immediate: true },
  )

  function toggle() {
    enabled.value = !enabled.value
  }

  // real value -> stand-in, for every name the app knows about. Ordered by
  // id so a host keeps its stand-in while the list re-sorts.
  const table = computed(() => {
    const map = new Map<string, string>()
    const put = (real: string | null | undefined, fake: string) => {
      const k = real?.trim()
      if (!k || PUBLIC_ADDRESS.test(k) || map.has(k)) return
      map.set(k, fake)
    }

    const list = [...hosts.hosts].sort((a, b) => a.id - b.id)
    const width = Math.max(2, String(list.length).length)
    list.forEach((h, i) => {
      const name = `server-${String(i + 1).padStart(width, '0')}`
      const address = IPV4_ONLY.test(h.address) ? fakeIp(i) : `${name}.example.com`
      // A host named after its own address shows the stand-in address
      put(h.label, h.label === h.address ? address : name)
      put(h.address, address)
    })

    ;[...hosts.groups]
      .sort((a, b) => a.sort - b.sort || a.id - b.id)
      .forEach((g, i) => put(g.name, `Group ${letters(i)}`))

    ;[...credentials.credentials]
      .sort((a, b) => a.id - b.id)
      .forEach((c, i) => put(c.name, `credential-${i + 1}`))

    const users = new Set<string>()
    for (const h of hosts.hosts) users.add(h.username)
    for (const c of credentials.credentials) users.add(c.username)
    ;[...users]
      .filter((u) => u && !GENERIC_USERS.has(u))
      .sort()
      .forEach((u, i) => put(u, i === 0 ? 'deploy' : `deploy${i + 1}`))

    return map
  })

  // One pass over the text: known values (longest first, so "db-prod" wins
  // over "db"), then any other IPv4 address. Matches only whole tokens, so
  // "dot" doesn't eat into "bersamadot"; a trailing sentence dot still ends one.
  const matcher = computed(() => {
    const keys = [...table.value.keys()].sort((a, b) => b.length - a.length).map(escapeRe)
    const alts = [...keys, IPV4].join('|')
    return new RegExp(`(^|[^\\w.-])(${alts})(?=$|[^\\w-]|\\.(?!\\w))`, 'g')
  })

  function standIn(real: string): string {
    const known = table.value.get(real)
    if (known) return known
    // An IPv4 address nobody saved (resolved IPs, error messages…)
    if (PUBLIC_ADDRESS.test(real)) return real
    return `192.0.2.${(hash(real) % 254) + 1}`
  }

  /** The text split into plain and secret parts, with secrets already
   *  swapped in dummy style. Feeds <PvText>. */
  function segments(text: string | null | undefined): PvSegment[] {
    const s = text ?? ''
    if (!enabled.value || !s) return [{ text: s, secret: false }]
    const out: PvSegment[] = []
    const re = matcher.value
    re.lastIndex = 0
    let last = 0
    for (let m = re.exec(s); m; m = re.exec(s)) {
      const start = m.index + m[1].length
      const real = m[2]
      // An IPv4 that isn't really one (version strings like 1.2.3.4567 are
      // already excluded by the token rules; this guards 999.1.1.1)
      if (!table.value.has(real) && real.split('.').some((p) => Number(p) > 255)) continue
      if (PUBLIC_ADDRESS.test(real)) continue
      if (start > last) out.push({ text: s.slice(last, start), secret: false })
      out.push({ text: dummy.value ? standIn(real) : real, secret: true })
      last = start + real.length
    }
    if (last < s.length) out.push({ text: s.slice(last), secret: false })
    return out
  }

  /** Text with every known value swapped for its stand-in whenever privacy
   *  mode is on, in either style. For places that can't blur: tooltips,
   *  aria labels, window titles. */
  function t(text: string | null | undefined): string {
    const s = text ?? ''
    if (!enabled.value || !s) return s
    const re = matcher.value
    re.lastIndex = 0
    return s.replace(re, (whole, pre: string, real: string) => {
      if (!table.value.has(real) && real.split('.').some((p) => Number(p) > 255)) return whole
      return pre + standIn(real)
    })
  }

  /** True when the text holds something privacy mode hides. */
  function has(text: string | null | undefined): boolean {
    if (!enabled.value || !text) return false
    return segments(text).some((p) => p.secret)
  }

  /** A string that is a host identifier as a whole (known_hosts entries,
   *  forward targets, log rows of deleted hosts): when nothing in it is
   *  known, it still gets a stable stand-in instead of showing through. */
  function hostText(text: string | null | undefined): string {
    const s = text ?? ''
    if (!dummy.value || !s) return s
    const swapped = t(s)
    if (swapped !== s || PUBLIC_ADDRESS.test(s)) return swapped
    // Keep a "[...]:port" shape readable
    const bracket = /^\[(.+)\](:\d+)$/.exec(s)
    if (bracket) return `[${hostText(bracket[1])}]${bracket[2]}`
    return `host-${hash(s).toString(16).padStart(8, '0').slice(0, 4)}.example.com`
  }

  /** A host key fingerprint: shown as a stable fake in dummy style. */
  function fingerprint(fp: string | null | undefined): string {
    const s = fp ?? ''
    if (!dummy.value || !s) return s
    const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
    let out = ''
    let h = hash(s)
    for (let i = 0; i < 43; i++) {
      h = Math.imul(h ^ (h >>> 15), 0x2c1b3c6d) >>> 0
      out += alphabet[h % 64]
    }
    return s.startsWith('MD5:') ? `MD5:${out.slice(0, 32)}` : `SHA256:${out}`
  }

  return { enabled, style, dummy, blur, toggle, segments, t, has, hostText, fingerprint }
})
