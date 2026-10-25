// Tag identity helpers — a tag always resolves to the same hue, so
// its dot reads as the same "folder" color across list, cards and chips.

function tagHue(tag: string): number {
  let h = 0
  for (let i = 0; i < tag.length; i++) h = (h * 31 + tag.charCodeAt(i)) >>> 0
  return h % 360
}

export function tagColor(tag: string): string {
  return `oklch(0.72 0.15 ${tagHue(tag)})`
}

export function normalizeTag(raw: string): string {
  return raw
    .replace(/^#+/, '')
    .trim()
    .replace(/\s+/g, ' ')
    .slice(0, 32)
}

// Tags live in the note body as `#word` (start of line or after
// whitespace). Code spans and fenced blocks are not scanned — a
// command like `make #target` is code, not a tag.
export function extractTags(body: string): string[] {
  const noCode = body.replace(/```[\s\S]*?```/g, (m) => ' '.repeat(m.length))
  const out: string[] = []
  for (const line of noCode.split('\n')) {
    const stripped = line.replace(/`[^`]*`/g, (m) => ' '.repeat(m.length))
    for (const m of stripped.matchAll(/(?:^|\s)#([A-Za-z0-9][\w-]*)/g)) {
      if (!out.includes(m[1])) out.push(m[1])
    }
  }
  return out
}
