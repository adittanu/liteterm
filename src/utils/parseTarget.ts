// Parse a "quick connect" string into an SSH target: `user@host:port`,
// `host`, bare IP, `sub.domain.tld`, optionally prefixed with `ssh://`.
// Anything that doesn't look like a connectable target (plain search words
// like "prod") returns null so the regular search flow stays untouched.

export interface SmartTarget {
  username: string | null
  host: string
  port: number | null
  kind: 'ip' | 'domain'
}

const HOST_LABEL = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/

export function isIpv4(s: string): boolean {
  const parts = s.split('.')
  if (parts.length !== 4) return false
  return parts.every((p) => {
    if (!/^\d{1,3}$/.test(p)) return false
    const n = Number(p)
    return n >= 0 && n <= 255
  })
}

function isHostname(s: string): boolean {
  if (s.length > 253) return false
  const labels = s.split('.')
  return labels.every((l) => HOST_LABEL.test(l))
}

export function parseTarget(raw: string): SmartTarget | null {
  let s = raw.trim()
  if (!s || /\s/.test(s)) return null

  const scheme = /^ssh:\/\//i
  if (scheme.test(s)) s = s.replace(scheme, '')
  if (!s) return null

  // IPv6 literals contain multiple colons — out of scope for v1
  if ((s.match(/:/g) ?? []).length > 1) return null

  let port: number | null = null
  const portMatch = s.match(/:(\d{1,5})$/)
  if (portMatch) {
    const p = Number(portMatch[1])
    if (p < 1 || p > 65535) return null
    port = p
    s = s.slice(0, -portMatch[0].length)
  }
  if (!s) return null

  let username: string | null = null
  const at = s.lastIndexOf('@')
  if (at >= 0) {
    username = s.slice(0, at)
    s = s.slice(at + 1)
    if (!username || !s) return null
  }

  const host = s.toLowerCase()
  if (isIpv4(host)) return { username, host, port, kind: 'ip' }

  // Hostname must be dot-qualified, or carry an explicit user@ — a single
  // word like "prod" stays a plain search query
  if (!host.includes('.') && username === null) return null
  if (!isHostname(host)) return null
  return { username, host, port, kind: 'domain' }
}

export function targetDisplay(t: SmartTarget): string {
  const user = t.username ? `${t.username}@` : ''
  const port = t.port ? `:${t.port}` : ''
  return `${user}${t.host}${port}`
}
