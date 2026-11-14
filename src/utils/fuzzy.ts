// Small fuzzy matcher for pickers: contiguous matches beat scattered ones,
// matches at the start of a word beat matches mid-word. Returns the matched
// character indices so the UI can highlight why a row matched.

export interface FuzzyHit {
  score: number
  hits: number[]
}

const BOUNDARY = /[\s._\-/@:]/

export function fuzzy(query: string, text: string): FuzzyHit | null {
  const q = query.trim().toLowerCase()
  if (!q) return { score: 0, hits: [] }
  const t = text.toLowerCase()

  // Contiguous substring: best case
  const idx = t.indexOf(q)
  if (idx !== -1) {
    const atWord = idx === 0 || BOUNDARY.test(t[idx - 1])
    const hits = Array.from({ length: q.length }, (_, i) => idx + i)
    return { score: 1000 - idx * 2 + (idx === 0 ? 300 : atWord ? 150 : 0) + (q.length === t.length ? 200 : 0), hits }
  }

  // Subsequence: every query char in order
  const hits: number[] = []
  let score = 0
  let ti = 0
  let prev = -2
  for (const ch of q) {
    if (ch === ' ') continue
    const found = t.indexOf(ch, ti)
    if (found === -1) return null
    const atWord = found === 0 || BOUNDARY.test(t[found - 1])
    score += found === prev + 1 ? 12 : atWord ? 8 : 1
    hits.push(found)
    prev = found
    ti = found + 1
  }
  return { score: score - (hits[hits.length - 1] - hits[0]), hits }
}

/** Split text into highlighted / plain runs for rendering. */
export function highlight(text: string, hits: number[]): { text: string; hit: boolean }[] {
  if (!hits.length) return [{ text, hit: false }]
  const set = new Set(hits)
  const out: { text: string; hit: boolean }[] = []
  for (let i = 0; i < text.length; i++) {
    const hit = set.has(i)
    const last = out[out.length - 1]
    if (last && last.hit === hit) last.text += text[i]
    else out.push({ text: text[i], hit })
  }
  return out
}

/**
 * Typo distance: insertions, deletions, substitutions and swapped neighbours
 * ("prdo" → "prod") each cost 1 (optimal string alignment). Gives up and
 * returns `max + 1` once the distance can only exceed `max`.
 */
export function editDistance(a: string, b: string, max = Infinity): number {
  if (Math.abs(a.length - b.length) > max) return max + 1
  let prev2: number[] = []
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j)
  for (let i = 1; i <= a.length; i++) {
    const cur = [i]
    let rowMin = i
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1
      let d = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + cost)
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d = Math.min(d, prev2[j - 2] + 1)
      cur.push(d)
      rowMin = Math.min(rowMin, d)
    }
    if (rowMin > max) return max + 1
    prev2 = prev
    prev = cur
  }
  return prev[b.length]
}
