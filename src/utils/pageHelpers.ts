import { onBeforeUnmount, ref, watch, type WatchSource } from 'vue'
import { useUiStore } from '../stores/ui'

/** Copy text and confirm with a short toast. */
export async function copyText(text: string, what = 'Copied') {
  const ui = useUiStore()
  try {
    await navigator.clipboard.writeText(text)
    ui.toast('success', what, { duration: 1800 })
  } catch {
    ui.toast('error', 'Could not access the clipboard')
  }
}

/**
 * Mirrors `loading`, but only turns on after `delay` ms. Fast loads never
 * flash a spinner — perceived performance beats an honest-but-noisy cue.
 */
export function useDelayedFlag(loading: WatchSource<boolean>, delay = 160) {
  const shown = ref(false)
  let timer: ReturnType<typeof setTimeout> | undefined
  watch(
    loading,
    (on) => {
      clearTimeout(timer)
      if (on) timer = setTimeout(() => (shown.value = true), delay)
      else shown.value = false
    },
    { immediate: true },
  )
  onBeforeUnmount(() => clearTimeout(timer))
  return shown
}

/** Case-insensitive "every word appears somewhere" match. */
export function matchesQuery(query: string, ...fields: (string | null | undefined)[]): boolean {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return true
  const hay = fields.filter(Boolean).join(' ').toLowerCase()
  return words.every((w) => hay.includes(w))
}

/** Parse SQLite 'YYYY-MM-DD HH:MM:SS'. `utc` for datetime('now') columns. */
export function parseSqlDate(s: string, utc = false): Date | null {
  if (!s) return null
  let iso = s.includes('T') ? s : s.replace(' ', 'T')
  if (utc && !/[zZ]|[+-]\d\d:?\d\d$/.test(iso)) iso += 'Z'
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? null : d
}

/** "Today" / "Yesterday" / "Mon, Oct 20" / "Oct 20, 2025" */
export function dayLabel(d: Date, now = new Date()): string {
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const diff = Math.round((start(now) - start(d)) / 86_400_000)
  if (diff === 0) return 'Today'
  if (diff === 1) return 'Yesterday'
  if (diff > 1 && diff < 7) return d.toLocaleDateString([], { weekday: 'long' })
  return d.toLocaleDateString(
    [],
    d.getFullYear() === now.getFullYear()
      ? { weekday: 'short', month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' },
  )
}

/** Short relative age: "just now", "5 min ago", "3 days ago", or a date. */
export function relativeAge(d: Date, now = new Date()): string {
  const secs = Math.round((now.getTime() - d.getTime()) / 1000)
  if (secs < 45) return 'just now'
  const mins = Math.round(secs / 60)
  if (mins < 60) return `${mins} min ago`
  const hours = Math.round(mins / 60)
  if (hours < 24) return `${hours} h ago`
  const days = Math.round(hours / 24)
  if (days < 30) return days === 1 ? 'yesterday' : `${days} days ago`
  return d.toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' })
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`
}
