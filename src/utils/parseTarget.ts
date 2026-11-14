// Parse a "quick connect" string into an SSH target: `user@host:port`,
// `host`, bare IP, `sub.domain.tld`, optionally prefixed with `ssh://`, or given as a web URL
// (`http://` / `https://`, with an optional trailing path or slash). A whole
// pasted `ssh` command line (`ssh -p 2222 -i ~/.ssh/key -J bastion deploy@host`)
// works too: its port, user, key and jump host are picked up.
// Anything that doesn't look like a connectable target (plain search words
// like "prod") returns null so the regular search flow stays untouched.

export interface SmartTarget {
  username: string | null
  host: string
  port: number | null
  kind: 'ip' | 'domain'
  /** How it was written: a bare address, a URL (ssh:// or http(s)://), or an `ssh` command */
  from: 'plain' | 'url' | 'command'
  /** `-i` / IdentityFile from an ssh command, as written (may start with ~) */
  keyPath?: string
  /** `-J` / ProxyJump from an ssh command: the hop nearest the target */
  jump?: string
  /**
   * `ssh prod`: a single word with nothing else to go on. It may name a saved
   * host, but on its own it's as likely a search ("ssh config"), so callers
   * only offer it when it matches one.
   */
  alias?: boolean
}

/** A `[user@]host[:port]` hop, as written in `-J`. */
export interface HopSpec {
  username: string | null
  host: string
  port: number | null
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

/**
 * `[scheme://][user@]host[:port][/path]` without whitespace. `bareWord`
 * allows a single word like `prod` as the host (an ssh command or a jump
 * spec names it on purpose; a search box query doesn't).
 */
function parseAddress(raw: string, bareWord: boolean): (HopSpec & { kind: 'ip' | 'domain'; scheme: string | null }) | null {
  let s = raw
  if (!s || /\s/.test(s)) return null

  // A pasted web URL (https://hris.dev-dq.web.id/app?x=1#top) is treated as
  // "connect to that server": drop the scheme and any path/query/fragment.
  // Its port is the web port, not SSH, so it's ignored for http(s).
  const m = s.match(/^(ssh|https?):\/\//i)
  const scheme = m ? m[1].toLowerCase() : null
  const isWeb = !!scheme && scheme !== 'ssh'
  if (m) s = s.slice(m[0].length)
  if (m || /[/?#]/.test(s)) s = s.split(/[/?#]/, 1)[0]
  if (!s) return null

  // IPv6 literals contain multiple colons — out of scope for v1
  if ((s.match(/:/g) ?? []).length > 1) return null

  let port: number | null = null
  const portMatch = s.match(/:(\d{1,5})$/)
  if (portMatch) {
    const p = Number(portMatch[1])
    if (p < 1 || p > 65535) return null
    if (!isWeb) port = p
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
  if (isIpv4(host)) return { username, host, port, kind: 'ip', scheme }

  // Hostname must be dot-qualified, or carry an explicit user@ — a single
  // word like "prod" stays a plain search query
  if (!bareWord && !host.includes('.') && username === null) return null
  if (!isHostname(host)) return null
  return { username, host, port, kind: 'domain', scheme }
}

export function parseTarget(raw: string): SmartTarget | null {
  const s = raw.trim()
  if (/^ssh\s/i.test(s)) return parseSshCommand(s)
  const a = parseAddress(s, false)
  if (!a) return null
  return { username: a.username, host: a.host, port: a.port, kind: a.kind, from: a.scheme ? 'url' : 'plain' }
}

/** Parse one `-J` hop (`user@host:port`, `ssh://…`, or a bare name). */
export function parseHop(raw: string): HopSpec | null {
  const a = parseAddress(raw.trim(), true)
  return a ? { username: a.username, host: a.host, port: a.port } : null
}

// ---- ssh command line ----

/** Split like a POSIX shell would for the simple cases people paste: spaces, '…' and "…". */
function shellWords(s: string): string[] | null {
  const out: string[] = []
  let cur = ''
  let inWord = false
  let quote: '"' | "'" | null = null
  for (const ch of s) {
    if (quote) {
      if (ch === quote) quote = null
      else cur += ch
    } else if (ch === '"' || ch === "'") {
      quote = ch
      inWord = true
    } else if (/\s/.test(ch)) {
      if (inWord) out.push(cur)
      cur = ''
      inWord = false
    } else {
      cur += ch
      inWord = true
    }
  }
  if (quote) return null // unbalanced: still being typed
  if (inWord) out.push(cur)
  return out
}

// OpenSSH's option letters: these take a value, the rest are plain flags
const WITH_VALUE = new Set('BbcDEeFIiJLlmOoPpQRSWw')

function parseSshCommand(s: string): SmartTarget | null {
  const words = shellWords(s)
  if (!words || words[0].toLowerCase() !== 'ssh') return null

  let user: string | null = null
  let port: number | null = null
  let keyPath: string | undefined
  let jump: string | undefined
  let dest: string | null = null
  let badPort = false
  let sawOption = false

  const setPort = (v: string) => {
    const p = Number(v)
    if (!/^\d{1,5}$/.test(v) || p < 1 || p > 65535) badPort = true
    else port ??= p
  }
  const option = (letter: string, value: string) => {
    switch (letter) {
      case 'p': return setPort(value)
      case 'l': user ??= value; return
      case 'i': keyPath ??= value; return
      case 'J': jump ??= value; return
      case 'o': {
        const m = value.match(/^\s*([A-Za-z]+)\s*(?:=|\s)\s*(.+?)\s*$/)
        if (!m) return
        const key = m[1].toLowerCase()
        if (key === 'port') setPort(m[2])
        else if (key === 'user') user ??= m[2]
        else if (key === 'identityfile') keyPath ??= m[2]
        else if (key === 'proxyjump' && m[2].toLowerCase() !== 'none') jump ??= m[2]
      }
    }
  }

  // Like OpenSSH, options may come before or right after the destination;
  // the first plain word after the destination starts the remote command
  let i = 1
  let optionsDone = false
  while (i < words.length) {
    const w = words[i++]
    if (!optionsDone && w === '--') {
      optionsDone = true
      continue
    }
    if (!optionsDone && w.length > 1 && w.startsWith('-')) {
      sawOption = true
      for (let k = 1; k < w.length; k++) {
        const letter = w[k]
        if (!WITH_VALUE.has(letter)) continue
        const rest = w.slice(k + 1)
        const value = rest || words[i++]
        if (value === undefined) return null // `-p` still waiting for its value
        option(letter, value)
        break
      }
      continue
    }
    if (dest !== null) break
    dest = w
  }
  if (dest === null || badPort) return null

  const a = parseAddress(dest, true)
  if (!a) return null
  // An explicit -l / -p wins over user@ / :port in the destination, as in OpenSSH
  const target: SmartTarget = {
    username: user ?? a.username,
    host: a.host,
    port: port ?? a.port,
    kind: a.kind,
    from: 'command',
  }
  if (a.kind === 'domain' && !a.host.includes('.') && !a.username && !a.scheme && !sawOption) target.alias = true
  if (keyPath) target.keyPath = keyPath
  // A chain (-J a,b) reaches the target through its last hop
  const lastHop = jump?.split(',').pop()?.trim()
  if (lastHop) target.jump = lastHop
  return target
}

export function targetDisplay(t: SmartTarget): string {
  const user = t.username ? `${t.username}@` : ''
  const port = t.port ? `:${t.port}` : ''
  return `${user}${t.host}${port}`
}
