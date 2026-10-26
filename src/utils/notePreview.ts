import type { Note } from '../types'

const MARKER = /^(#{1,6} |[-*] \[[xX ]\] |[-*] |\d+\. |>\s?)/
const SEPARATOR_LINE = /^\s*\|?[-:\s|]+\|?\s*$/
// Inline images are stored as markdown `![alt](url)`; the URL is noise in a
// one-line preview, so each image collapses to a short text placeholder.
const IMAGE = /!\[[^\]]*\]\([^)]*\)/g

/** One-line plain-text preview of a note body (markdown markers stripped). */
export function previewOf(n: Note): string {
  return n.body
    .split('\n')
    .filter((l) => l.trim() !== '' && !SEPARATOR_LINE.test(l) && !l.trimStart().startsWith('```'))
    .map((l) => l.replace(IMAGE, ' Image ').replace(MARKER, '').replace(/(\*\*|~~|\*|`|\|)/g, ''))
    .join(' ')
    .replace(/\s+/g, ' ')
    .trim()
}
