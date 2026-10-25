import {
  File as FileIcon,
  FileArchive,
  FileAudio,
  FileCode2,
  FileImage,
  FileJson,
  FileKey,
  FileSpreadsheet,
  FileTerminal,
  FileText,
  FileVideo,
  Folder,
  Link2,
  type LucideIcon,
} from 'lucide-vue-next'
import type { FileEntry } from '../../types'

export type PaneKind = 'local' | 'ssh'

// ---- paths ----

export function joinRemote(dir: string, name: string): string {
  return dir.endsWith('/') ? `${dir}${name}` : `${dir}/${name}`
}

export function joinLocal(dir: string, name: string): string {
  const sep = dir.includes('/') && !dir.includes('\\') ? '/' : '\\'
  return dir.endsWith(sep) ? `${dir}${name}` : `${dir}${sep}${name}`
}

export function joinPath(kind: PaneKind, dir: string, name: string): string {
  return kind === 'ssh' ? joinRemote(dir, name) : joinLocal(dir, name)
}

export function remoteParent(p: string): string | null {
  if (p === '/' || p === '') return null
  const trimmed = p.replace(/\/+$/, '')
  const idx = trimmed.lastIndexOf('/')
  return idx <= 0 ? '/' : trimmed.slice(0, idx)
}

export function localParent(p: string): string | null {
  const norm = p.replace(/\//g, '\\').replace(/\\+$/, '')
  if (/^[A-Z]:$/i.test(norm) || !norm) return null
  const idx = norm.lastIndexOf('\\')
  if (idx === -1) return null
  if (idx <= 2) return norm.slice(0, 2) + '\\'
  return norm.slice(0, idx)
}

export function parentOf(kind: PaneKind, p: string): string | null {
  return kind === 'ssh' ? remoteParent(p) : localParent(p)
}

export function baseName(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean)
  return parts[parts.length - 1] ?? p
}

/** Same location, ignoring trailing separators (and case on Windows). */
export function samePath(kind: PaneKind, a: string, b: string): boolean {
  const norm = (s: string) => {
    const t = s.replace(/[\\/]+$/, '') || s
    return kind === 'local' ? t.replace(/\//g, '\\').toLowerCase() : t
  }
  return norm(a) === norm(b)
}

export function crumbsFor(kind: PaneKind, path: string): { name: string; path: string }[] {
  if (kind === 'ssh') {
    const parts = path.split('/').filter(Boolean)
    return [
      { name: '/', path: '/' },
      ...parts.map((p, i) => ({ name: p, path: '/' + parts.slice(0, i + 1).join('/') })),
    ]
  }
  if (!path) return []
  const parts = path.replace(/\//g, '\\').split('\\').filter(Boolean)
  return parts.map((part, i) => ({
    name: part,
    path: i === 0 ? `${part}\\` : parts.slice(0, i + 1).join('\\'),
  }))
}

/** POSIX single-quote a path for a shell `cd`. */
export function shellQuote(p: string): string {
  return `'${p.replace(/'/g, `'\\''`)}'`
}

// ---- presentation ----

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`
}

export function fmtModified(mtime: number, now = Date.now()): string {
  if (!mtime) return '—'
  const d = new Date(mtime * 1000)
  const today = new Date(now)
  if (d.toDateString() === today.toDateString()) {
    return `Today ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
  }
  const yesterday = new Date(now - 86_400_000)
  if (d.toDateString() === yesterday.toDateString()) {
    return `Yesterday ${d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}`
  }
  return d.toLocaleDateString([], {
    year: d.getFullYear() === today.getFullYear() ? undefined : 'numeric',
    month: 'short',
    day: 'numeric',
  })
}

/** 0o755 → "rwxr-xr-x" */
export function fmtPerms(mode: number | null): string {
  if (mode === null) return ''
  const bits = mode & 0o777
  const sym = 'rwxrwxrwx'
  let out = ''
  for (let i = 0; i < 9; i++) out += bits & (1 << (8 - i)) ? sym[i] : '-'
  return out
}

export function octalPerms(mode: number | null): string {
  return mode === null ? '' : ((mode >>> 0) & 0o7777).toString(8).padStart(4, '0')
}

const EXT_GROUPS: [LucideIcon, string, string[]][] = [
  [FileImage, 'image', ['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif', 'heic']],
  [FileVideo, 'video', ['mp4', 'mkv', 'mov', 'avi', 'webm']],
  [FileAudio, 'audio', ['mp3', 'wav', 'flac', 'ogg', 'm4a']],
  [FileArchive, 'archive', ['zip', 'tar', 'gz', 'tgz', 'bz2', 'xz', '7z', 'rar', 'zst', 'deb', 'rpm']],
  [FileTerminal, 'script', ['sh', 'bash', 'zsh', 'fish', 'ps1', 'bat', 'cmd']],
  [FileJson, 'data', ['json', 'yaml', 'yml', 'toml', 'xml', 'ini', 'conf', 'cfg', 'env']],
  [FileKey, 'key', ['pem', 'key', 'crt', 'cer', 'pub', 'ppk', 'p12', 'pfx']],
  [FileSpreadsheet, 'sheet', ['csv', 'tsv', 'xls', 'xlsx']],
  [FileText, 'text', ['txt', 'md', 'log', 'rst', 'pdf', 'doc', 'docx']],
  [
    FileCode2,
    'code',
    ['js', 'ts', 'jsx', 'tsx', 'vue', 'py', 'rb', 'go', 'rs', 'java', 'c', 'h', 'cpp', 'cs', 'php', 'html', 'css', 'scss', 'sql', 'lua', 'kt', 'swift'],
  ],
]

export function fileVisual(e: FileEntry): { icon: LucideIcon; tone: string } {
  if (e.kind === 'dir') return { icon: Folder, tone: 'dir' }
  if (e.kind === 'symlink') return { icon: Link2, tone: 'link' }
  const lower = e.name.toLowerCase()
  if (lower.startsWith('id_') || lower === 'authorized_keys' || lower === 'known_hosts') {
    return { icon: FileKey, tone: 'key' }
  }
  if (lower === 'dockerfile' || lower === 'makefile') return { icon: FileCode2, tone: 'code' }
  const ext = lower.includes('.') ? lower.split('.').pop()! : ''
  for (const [icon, tone, exts] of EXT_GROUPS) if (exts.includes(ext)) return { icon, tone }
  return { icon: FileIcon, tone: 'file' }
}

export function isHidden(e: FileEntry): boolean {
  return e.name.startsWith('.')
}

export type SortKey = 'name' | 'mtime' | 'size'

export function sortEntries(list: FileEntry[], key: SortKey, dir: 1 | -1): FileEntry[] {
  return [...list].sort((a, b) => {
    // Folders always lead, whatever the column — like Finder and Explorer
    const fa = a.kind === 'dir' ? 0 : 1
    const fb = b.kind === 'dir' ? 0 : 1
    if (fa !== fb) return fa - fb
    let c = 0
    if (key === 'size') c = a.size - b.size
    else if (key === 'mtime') c = a.mtime - b.mtime
    if (c === 0) c = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: 'base' })
    return c * dir
  })
}

/** Selection range on the rendered order, anchor..target inclusive. */
export function rangeNames(order: FileEntry[], anchor: string, target: string): string[] {
  const a = order.findIndex((e) => e.name === anchor)
  const b = order.findIndex((e) => e.name === target)
  if (a === -1 || b === -1) return [target]
  const [lo, hi] = a < b ? [a, b] : [b, a]
  return order.slice(lo, hi + 1).map((e) => e.name)
}
