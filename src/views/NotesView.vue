<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from 'vue'
import {
  Archive,
  ArchiveRestore,
  ArrowUpDown,
  Copy,
  Search,
  X,
  SquarePen,
  Pin,
  PinOff,
  StickyNote,
  LayoutGrid,
  Rows3,
  Trash2,
} from 'lucide-vue-next'
import { useNotesStore } from '../stores/notes'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { tagColor } from '../utils/tags'
import { previewOf } from '../utils/notePreview'
import { copyText } from '../utils/pageHelpers'
import type { Note } from '../types'
import NoteEditor from '../components/NoteEditor.vue'
import ContextMenu, { type MenuItem } from '../components/ContextMenu.vue'

const notes = useNotesStore()
const ui = useUiStore()

type Layout = 'split' | 'gallery'
const layout = ref<Layout>(
  localStorage.getItem('notes.layout') === 'gallery' ? 'gallery' : 'split',
)
watch(layout, (v) => localStorage.setItem('notes.layout', v))

const query = ref('')
const activeTag = ref<string | null>(null)
const selectedId = ref<number | null>(null) // split mode selection
const galleryId = ref<number | null>(null) // gallery mode open note
const editorComp = ref<InstanceType<typeof NoteEditor> | null>(null)
const menu = ref<{ x: number; y: number; items: MenuItem[] } | null>(null)
// Archive is a folder of its own: archived notes leave All Notes entirely
const showArchived = ref(false)
// Drop target shown while dragging a note onto the Archive
const archiveZone = ref<HTMLElement | null>(null)

// ---- sort ----

type SortMode = 'edited' | 'created' | 'title' | 'manual'
const SORT_OPTIONS: { value: SortMode; label: string }[] = [
  { value: 'edited', label: 'Date edited' },
  { value: 'created', label: 'Date created' },
  { value: 'title', label: 'Title' },
  { value: 'manual', label: 'Manual order' },
]
const savedSort = localStorage.getItem('notes.sort')
const sortMode = ref<SortMode>(
  SORT_OPTIONS.some((o) => o.value === savedSort) ? (savedSort as SortMode) : 'edited',
)
watch(sortMode, (v) => localStorage.setItem('notes.sort', v))
const sortLabel = computed(() => SORT_OPTIONS.find((o) => o.value === sortMode.value)?.label ?? '')

const titleOf = (n: Note) => n.title || 'New Note'

// Pinned notes always lead. Within each group the chosen order applies;
// ties fall back to recency, so the list stays stable. SQLite timestamps
// sort lexicographically: 'YYYY-MM-DD HH:MM:SS'.
const sorted = computed(() =>
  [...notes.notes].sort((a, b) => {
    if (a.pinned !== b.pinned) return a.pinned ? -1 : 1
    let c = 0
    if (sortMode.value === 'manual') c = a.sort - b.sort
    else if (sortMode.value === 'title')
      c = titleOf(a).localeCompare(titleOf(b), undefined, { sensitivity: 'base', numeric: true })
    else if (sortMode.value === 'created') c = a.created_at < b.created_at ? 1 : a.created_at > b.created_at ? -1 : 0
    if (c) return c
    if (a.updated_at !== b.updated_at) return a.updated_at < b.updated_at ? 1 : -1
    return b.id - a.id
  }),
)

const tagList = computed(() => {
  const counts = new Map<string, number>()
  for (const n of notes.notes) {
    if (n.archived) continue
    for (const t of n.tags) counts.set(t, (counts.get(t) ?? 0) + 1)
  }
  return [...counts.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
})

const activeCount = computed(() => notes.notes.filter((n) => !n.archived).length)
const archivedCount = computed(() => notes.notes.filter((n) => n.archived).length)

const searching = computed(() => query.value.trim().length > 0)

const matches = computed(() => {
  let list = sorted.value.filter((n) => n.archived === showArchived.value)
  if (activeTag.value) list = list.filter((n) => n.tags.includes(activeTag.value as string))
  if (searching.value) {
    const q = query.value.trim().toLowerCase()
    list = list.filter(
      (n) =>
        n.title.toLowerCase().includes(q) ||
        n.body.toLowerCase().includes(q) ||
        n.tags.some((t) => t.toLowerCase().includes(q)),
    )
  }
  return list
})

// Pinned/Notes sections only in the unfiltered All Notes view — inside
// a search, a tag folder or the Archive the list reads better flat
const sectioned = computed(
  () => !searching.value && activeTag.value === null && !showArchived.value,
)
const pinnedNotes = computed(() =>
  sectioned.value ? matches.value.filter((n) => n.pinned) : [],
)
const otherNotes = computed(() =>
  sectioned.value ? matches.value.filter((n) => !n.pinned) : matches.value,
)

// One flat list (labels + rows) so a single TransitionGroup can glide
// rows between sections when a note is pinned, edited or deleted
type ListItem =
  | { kind: 'label'; key: string; text: string }
  | { kind: 'note'; key: string; note: Note }

const listItems = computed<ListItem[]>(() => {
  const out: ListItem[] = []
  const pinned = pinnedNotes.value
  const other = otherNotes.value
  if (pinned.length) out.push({ kind: 'label', key: 'label:pinned', text: 'Pinned' })
  for (const n of pinned) out.push({ kind: 'note', key: `n${n.id}`, note: n })
  if (pinned.length && other.length) out.push({ kind: 'label', key: 'label:notes', text: 'Notes' })
  for (const n of other) out.push({ kind: 'note', key: `n${n.id}`, note: n })
  return out
})

const visibleIds = computed(() => [...pinnedNotes.value, ...otherNotes.value].map((n) => n.id))

const selectedNote = computed(() => notes.notes.find((n) => n.id === selectedId.value) ?? null)
const galleryNote = computed(() => notes.notes.find((n) => n.id === galleryId.value) ?? null)

const listScroll = ref<HTMLElement | null>(null)
const galleryScroll = ref<HTMLElement | null>(null)

// ---- motion policy ----

// Rows glide to their new slot when the *data* reorders them (an edit
// bumps a note to the top, pin/unpin, delete, undo). Filtering is typed
// — keyboard-initiated — so search/tag changes re-render instantly.
const rowMotion = ref(true)
watch([query, activeTag, showArchived], () => {
  rowMotion.value = false
  nextTick(() => {
    rowMotion.value = true
  })
})

// Gallery cards stagger in once when the gallery appears — never again
// while typing in search (that would replay on every keystroke).
const galleryIntro = ref(false)
let introTimer: ReturnType<typeof setTimeout> | undefined
watch(
  layout,
  (v) => {
    clearTimeout(introTimer)
    galleryIntro.value = v === 'gallery'
    // swap out (130ms) + max stagger (336ms) + card-in (280ms), with slack
    if (v === 'gallery') introTimer = setTimeout(() => (galleryIntro.value = false), 900)
  },
  { immediate: true },
)
onBeforeUnmount(() => clearTimeout(introTimer))

// The full-page editor zooms out of the card that opened it and back
// into that card on close (spatial consistency). Escape closes instantly.
const pageOrigin = ref('50% 30%')
const instantClose = ref(false)
watch(galleryId, (v) => {
  if (v !== null) instantClose.value = false
})

function originFrom(el: Element) {
  const root = el.closest('.gallery-root')
  if (!root) return
  const a = root.getBoundingClientRect()
  const b = el.getBoundingClientRect()
  const x = Math.min(a.width, Math.max(0, b.left + b.width / 2 - a.left))
  const y = Math.min(a.height, Math.max(0, b.top + b.height / 2 - a.top))
  pageOrigin.value = `${Math.round(x)}px ${Math.round(y)}px`
}

function openCard(e: MouseEvent, id: number) {
  originFrom(e.currentTarget as Element)
  galleryId.value = id
}

function closeGallery(instant = false) {
  const id = galleryId.value
  if (id === null) return
  // The note may have moved while being edited — return to where its
  // card lives now, scrolled into view under the page first.
  const card = galleryScroll.value?.querySelector<HTMLElement>(`.card[data-note-id="${id}"]`)
  if (card) {
    card.scrollIntoView({ block: 'nearest' })
    originFrom(card)
  }
  instantClose.value = instant
  galleryId.value = null
  if (card) nextTick(() => card.focus({ preventScroll: true }))
}

onMounted(async () => {
  if (!notes.loaded) await notes.load()
  // A requested note (watch below) wins over the default first note
  if (ui.noteRequest !== null || selectedId.value !== null) return
  if (layout.value === 'split' && sorted.value[0]) selectedId.value = sorted.value[0].id
})

// Opened from a host's details panel: show that note, whatever the filters
watch(
  () => [ui.noteRequest, notes.loaded] as const,
  ([id, loaded]) => {
    if (id === null || !loaded) return
    ui.noteRequest = null
    if (!notes.notes.some((n) => n.id === id)) return
    query.value = ''
    activeTag.value = null
    if (layout.value === 'split') selectedId.value = id
    else galleryId.value = id
  },
  { immediate: true },
)

function toggleTag(name: string) {
  showArchived.value = false
  activeTag.value = activeTag.value === name ? null : name
}

function showAll() {
  showArchived.value = false
  activeTag.value = null
}

function openArchive() {
  activeTag.value = null
  showArchived.value = !showArchived.value
}

// Arrow keys walk the list like Apple Notes; selection follows focus
// instantly (keyboard navigation never animates).
function step(dir: -1 | 0 | 1) {
  const ids = visibleIds.value
  if (!ids.length) return
  const i = selectedId.value === null ? -1 : ids.indexOf(selectedId.value)
  const next = i === -1 ? ids[0] : ids[Math.min(ids.length - 1, Math.max(0, i + dir))]
  selectedId.value = next
  nextTick(() => {
    const el = listScroll.value?.querySelector<HTMLElement>(`[data-note-id="${next}"]`)
    el?.focus({ preventScroll: true })
    el?.scrollIntoView({ block: 'nearest' })
  })
}

function onSearchEscape(e: KeyboardEvent) {
  if (!query.value) return
  e.stopPropagation()
  query.value = ''
}

async function createNote(e?: Event) {
  const trigger = e?.currentTarget instanceof Element ? e.currentTarget : null
  try {
    const note = await notes.create()
    if (layout.value === 'split') selectedId.value = note.id
    else {
      if (trigger) originFrom(trigger)
      galleryId.value = note.id
    }
    nextTick(() => editorComp.value?.focusTitle())
  } catch (e) {
    ui.toast('error', `Could not create note: ${errorText(e)}`)
  }
}

async function removeNote(id: number) {
  const list = matches.value
  const idx = list.findIndex((n) => n.id === id)
  try {
    const undo = await notes.remove(id)
    ui.toast('info', 'Note deleted', { actionLabel: 'Undo', action: undo, duration: 6000 })
  } catch (e) {
    ui.toast('error', `Could not delete note: ${errorText(e)}`)
    return
  }
  if (layout.value === 'gallery') {
    if (galleryId.value === id) closeGallery()
    return
  }
  // Deleting from a row's context menu leaves the open note alone
  if (selectedId.value !== id) return
  const next = list[idx + 1] ?? list[idx - 1] ?? null
  selectedId.value = next ? next.id : null
}

// Archive or unarchive, with Undo. Selection moves on like after a delete.
async function archiveNote(id: number, archived = true) {
  const list = matches.value
  const idx = list.findIndex((n) => n.id === id)
  try {
    await notes.setArchived(id, archived)
  } catch (e) {
    ui.toast('error', `Could not ${archived ? 'archive' : 'unarchive'} note: ${errorText(e)}`)
    return
  }
  ui.toast('info', archived ? 'Note archived' : 'Note unarchived', {
    actionLabel: 'Undo',
    duration: 6000,
    action: () => void notes.setArchived(id, !archived).catch((e) => ui.toast('error', errorText(e))),
  })
  if (layout.value === 'gallery') {
    if (galleryId.value === id) closeGallery()
    return
  }
  if (selectedId.value !== id) return
  const next = list[idx + 1] ?? list[idx - 1] ?? null
  selectedId.value = next ? next.id : null
}

async function togglePinned(n: Note) {
  try {
    await notes.setPinned(n.id, !n.pinned)
  } catch (e) {
    ui.toast('error', `Could not update note: ${errorText(e)}`)
  }
}

function openMenu(n: Note, e: MouseEvent) {
  menu.value = {
    x: e.clientX,
    y: e.clientY,
    items: [
      {
        label: n.pinned ? 'Unpin' : 'Pin',
        icon: n.pinned ? PinOff : Pin,
        action: () => void togglePinned(n),
      },
      { label: 'Copy', icon: Copy, action: () => void copyText([n.title, n.body].filter(Boolean).join('\n\n'), 'Note copied') },
      {
        label: n.archived ? 'Unarchive' : 'Archive',
        icon: n.archived ? ArchiveRestore : Archive,
        action: () => void archiveNote(n.id, !n.archived),
      },
      { label: 'Delete', icon: Trash2, danger: true, action: () => void removeNote(n.id) },
    ],
  }
}

// ---- drag to reorder ----

// Only in Manual order with nothing filtering the list. Pointer events, not
// HTML5 drag: Tauri's Windows webview swallows HTML5 drag unless its native
// file-drop is disabled, and SFTP needs that for OS file drops.
const reorderable = computed(() => sortMode.value === 'manual' && sectioned.value)

type Pt = { x: number; y: number }
type Drag = { id: number; overId: number | null; after: boolean; dx: number; dy: number; overArchive: boolean }
// Cell positions when a drag starts, in scroll-content coordinates so that
// auto-scroll doesn't skew them. `order` is the displayed order at that time.
type Shot = { order: number[]; pos: Map<number, Pt>; center: Map<number, Pt> }

const drag = ref<Drag | null>(null)
const shot = shallowRef<Shot | null>(null)
let press: { id: number; x: number; y: number; scroll: number } | null = null
let suppressClick = false
let scrollStep = 0
let scrollTimer: ReturnType<typeof setInterval> | undefined

function scopeEl(): HTMLElement | null {
  return layout.value === 'split' ? listScroll.value : galleryScroll.value
}

// The order the list would take if dropped now, or null when nothing moves
// (dropped on itself, or across the pinned/unpinned split)
function previewOrder(d: Drag, s: Shot): number[] | null {
  // Only Manual order reorders; other sorts just lift the cell
  if (sortMode.value !== 'manual' || d.overArchive) return null
  const overId = d.overId
  if (overId === null || overId === d.id) return null
  const from = notes.notes.find((n) => n.id === d.id)
  const over = notes.notes.find((n) => n.id === overId)
  if (!from || !over || from.pinned !== over.pinned) return null
  const ids = s.order.filter((id) => id !== d.id)
  ids.splice(ids.indexOf(overId) + (d.after ? 1 : 0), 0, d.id)
  return ids.every((id, i) => id === s.order[i]) ? null : ids
}

// While one cell is held, every other cell slides into the slot its new
// position occupies — the gap opens exactly where the drop will land
const displacement = computed(() => {
  const out = new Map<number, Pt>()
  const d = drag.value
  const s = shot.value
  const order = d && s ? previewOrder(d, s) : null
  if (!d || !s || !order) return out
  order.forEach((id, i) => {
    const from = s.pos.get(id)
    const to = s.pos.get(s.order[i])
    if (id !== d.id && from && to) out.set(id, { x: to.x - from.x, y: to.y - from.y })
  })
  return out
})

function cellClass(id: number) {
  return { shift: !!drag.value && drag.value.id !== id, 'drag-src': drag.value?.id === id }
}

function cellStyle(id: number) {
  const v = displacement.value.get(id)
  return v ? { transform: `translate(${v.x}px, ${v.y}px)` } : undefined
}

// The held cell follows the pointer through `translate`, leaving `transform`
// to the cell wrapper's slide, so the two never fight
function liftStyle(id: number) {
  const d = drag.value
  return d && d.id === id ? { translate: `${d.dx}px ${d.dy}px` } : undefined
}

function takeShot(scope: HTMLElement): Shot {
  const sr = scope.getBoundingClientRect()
  const pos = new Map<number, Pt>()
  const center = new Map<number, Pt>()
  scope.querySelectorAll<HTMLElement>('[data-note-id]').forEach((el) => {
    const id = Number(el.dataset.noteId)
    const r = (el.closest<HTMLElement>('[data-cell]') ?? el).getBoundingClientRect()
    const x = r.left - sr.left
    const y = r.top - sr.top + scope.scrollTop
    pos.set(id, { x, y })
    center.set(id, { x: x + r.width / 2, y: y + r.height / 2 })
  })
  return { order: matches.value.map((n) => n.id), pos, center }
}

const SETTLE_MS = 280
// --ease-move: strong in-out, the curve the list uses for on-screen moves
const SETTLE_EASE = 'cubic-bezier(0.77, 0, 0.175, 1)'
function animateBack(el: Element | null | undefined, frames: Keyframe[]) {
  if (!el || matchMedia('(prefers-reduced-motion: reduce)').matches) return
  el.animate(frames, { duration: SETTLE_MS, easing: SETTLE_EASE })
}

function onRowPointerDown(e: PointerEvent, n: Note) {
  if (e.button !== 0) return
  press = { id: n.id, x: e.clientX, y: e.clientY, scroll: scopeEl()?.scrollTop ?? 0 }
  window.addEventListener('pointermove', onDragMove)
  window.addEventListener('pointerup', onDragEnd)
  window.addEventListener('pointercancel', onDragEnd)
}

function onDragMove(e: PointerEvent) {
  if (!press) return
  const scope = scopeEl()
  if (!drag.value) {
    // A few px of travel separates a drag from a click
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < 5) return
    if (!sectioned.value) {
      // Say why nothing moves instead of failing silently
      stopDrag()
      ui.toast(
        'info',
        showArchived.value ? 'Open All Notes to drag notes' : 'Clear the search or tag filter to drag notes',
        { duration: 4000 },
      )
      return
    }
    if (!scope) return
    shot.value = takeShot(scope)
    drag.value = { id: press.id, overId: null, after: false, dx: 0, dy: 0, overArchive: false }
    document.body.classList.add('notes-dragging')
    scrollTimer = setInterval(() => {
      const s = scopeEl()
      if (s && scrollStep) s.scrollTop += scrollStep
    }, 16)
  }
  const d = drag.value
  const s = shot.value
  if (!scope || !d || !s) return
  // Offset from the press point with scroll included, so the cell stays
  // under the pointer while the list auto-scrolls
  d.dx = e.clientX - press.x
  d.dy = e.clientY - press.y + (scope.scrollTop - press.scroll)
  // Over the Archive zone the list stops previewing and stops auto-scrolling
  const z = archiveZone.value?.getBoundingClientRect()
  d.overArchive =
    !!z && e.clientX >= z.left && e.clientX <= z.right && e.clientY >= z.top && e.clientY <= z.bottom
  // Scroll while the pointer rests near the top or bottom edge
  const r = scope.getBoundingClientRect()
  scrollStep = d.overArchive ? 0 : e.clientY < r.top + 40 ? -8 : e.clientY > r.bottom - 40 ? 8 : 0
  // Target = the cell whose centre is nearest the held cell's centre. Uses the
  // drag-start snapshot rather than hit-testing, so displaced cells can't flicker it.
  const own = s.center.get(d.id)
  if (!own) return
  const cx = own.x + d.dx
  const cy = own.y + d.dy
  let best = own
  let bestId: number | null = null
  let bestDist = Infinity
  for (const [id, c] of s.center) {
    const dist = (c.x - cx) ** 2 + (c.y - cy) ** 2
    if (dist < bestDist) {
      bestDist = dist
      bestId = id
      best = c
    }
  }
  d.overId = bestId
  // Rows stack vertically; gallery cards flow left to right
  d.after = layout.value === 'split' ? cy > best.y : cx > best.x
}

function stopDrag(): Drag | null {
  window.removeEventListener('pointermove', onDragMove)
  window.removeEventListener('pointerup', onDragEnd)
  window.removeEventListener('pointercancel', onDragEnd)
  press = null
  scrollStep = 0
  clearInterval(scrollTimer)
  document.body.classList.remove('notes-dragging')
  const d = drag.value
  drag.value = null
  shot.value = null
  return d
}

function onDragEnd() {
  // Decide what the drop does before the state driving it is cleared
  const d = drag.value
  const s = shot.value
  const order = d && s ? previewOrder(d, s) : null
  const held = displacement.value
  const scope = scopeEl()
  stopDrag()
  if (!d) return
  // The click that follows this pointer-up belongs to the drag, not a select
  suppressClick = true
  setTimeout(() => (suppressClick = false), 0)
  if (d.overArchive) {
    archiveDragged(d, scope)
    return
  }
  if (order) {
    persistOrder(order)
  } else if (sortMode.value !== 'manual') {
    ui.toast('info', 'Drag reorders notes in Manual order', {
      actionLabel: 'Switch',
      action: () => (sortMode.value = 'manual'),
      duration: 5000,
    })
  }
  // Runs after Vue has removed the inline offsets, so these animations pick
  // up exactly where the drag left off. A committed drop needs none for the
  // other cells: the list's FLIP moves them into their new slots.
  nextTick(() => {
    animateBack(scope?.querySelector(`[data-note-id="${d.id}"]`), [
      { translate: `${d.dx}px ${d.dy}px`, scale: '1.02' },
      { translate: '0px 0px', scale: '1' },
    ])
    if (order) return
    // Cancelled: the displaced cells glide back to where they were
    for (const [id, v] of held) {
      animateBack(scope?.querySelector(`[data-note-id="${id}"]`)?.closest('[data-cell]'), [
        { transform: `translate(${v.x}px, ${v.y}px)` },
        { transform: 'translate(0px, 0px)' },
      ])
    }
  })
}

// Dropped on the Archive: the note fades into the zone, then gets archived
function archiveDragged(d: Drag, scope: HTMLElement | null) {
  const el = scope?.querySelector<HTMLElement>(`[data-note-id="${d.id}"]`)
  const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches
  const flight =
    el && !reduced
      ? el.animate(
          [
            { translate: `${d.dx}px ${d.dy}px`, scale: '1.02', opacity: 1 },
            { translate: `${d.dx}px ${d.dy + 24}px`, scale: '0.9', opacity: 0 },
          ],
          { duration: 200, easing: 'cubic-bezier(0.23, 1, 0.32, 1)' }, // --ease-out
        )
      : null
  void (flight ? flight.finished.catch(() => {}) : Promise.resolve()).then(() => archiveNote(d.id))
}
onBeforeUnmount(() => {
  stopDrag()
})

// Save a new manual order for the visible notes. Archived notes aren't on
// screen, so they keep their relative order after the visible ones.
function persistOrder(visibleIds: number[]) {
  const archivedIds = sorted.value.filter((n) => n.archived).map((n) => n.id)
  notes.reorder([...visibleIds, ...archivedIds]).catch((e) => ui.toast('error', `Could not reorder notes: ${errorText(e)}`))
}

// Place `fromId` just before or after `overId` among the visible notes
function moveNote(fromId: number, overId: number, after: boolean) {
  const from = notes.notes.find((n) => n.id === fromId)
  const over = notes.notes.find((n) => n.id === overId)
  // Pinned and unpinned notes keep their own sections
  if (!from || !over || from.pinned !== over.pinned) return
  const ids = matches.value.map((n) => n.id).filter((id) => id !== fromId)
  ids.splice(ids.indexOf(overId) + (after ? 1 : 0), 0, fromId)
  persistOrder(ids)
}

// Alt+Up/Down moves the focused row, the keyboard route to the same reorder
function onRowArrow(e: KeyboardEvent, dir: -1 | 1, n: Note) {
  if (!e.altKey) return step(dir)
  if (!reorderable.value) return
  const list = matches.value
  const other = list[list.findIndex((x) => x.id === n.id) + dir]
  if (!other || other.pinned !== n.pinned) return
  moveNote(n.id, other.id, dir > 0)
  // Moving the row can drop focus; put it back on the same note
  nextTick(() =>
    listScroll.value?.querySelector<HTMLElement>(`[data-note-id="${n.id}"]`)?.focus({ preventScroll: true }),
  )
}

function onRowClick(id: number) {
  if (!suppressClick) selectedId.value = id
}

function onCardClick(e: MouseEvent, id: number) {
  if (!suppressClick) openCard(e, id)
}

function openSortMenu(e: MouseEvent) {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
  menu.value = {
    x: r.right - 180,
    y: r.bottom + 6,
    items: SORT_OPTIONS.map((o) => ({
      label: o.label,
      checked: sortMode.value === o.value,
      action: () => (sortMode.value = o.value),
    })),
  }
}

// Escape closes the full-page editor opened from the gallery — instantly,
// since it's keyboard-initiated
function onKeydown(e: KeyboardEvent) {
  if (e.defaultPrevented) return
  if (e.key === 'Escape' && layout.value === 'gallery' && galleryId.value !== null) {
    closeGallery(true)
  }
}
onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))

// ---- dates (SQLite 'YYYY-MM-DD HH:MM:SS', local time) ----

function parseDate(s: string): Date | null {
  const d = new Date(s.replace(' ', 'T'))
  return Number.isNaN(d.getTime()) ? null : d
}

function listDate(s: string): string {
  const d = parseDate(s)
  if (!d) return ''
  const now = new Date()
  if (d.toDateString() === now.toDateString()) {
    return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }
  const yesterday = new Date(now)
  yesterday.setDate(now.getDate() - 1)
  if (d.toDateString() === yesterday.toDateString()) return 'Yesterday'
  const opts: Intl.DateTimeFormatOptions =
    d.getFullYear() === now.getFullYear()
      ? { month: 'short', day: 'numeric' }
      : { year: 'numeric', month: 'short', day: 'numeric' }
  return d.toLocaleDateString([], opts)
}
</script>

<template>
  <div class="notes">
    <Transition name="swap" mode="out-in">
      <!-- ==== Split: list column + editor ==== -->
      <div v-if="layout === 'split'" key="split" class="split-root">
        <aside class="notes-list">
          <div class="list-toolbar">
            <div class="search-box">
              <Search :size="13" :stroke-width="1.5" aria-hidden="true" />
              <input
                v-model="query"
                type="text"
                placeholder="Search"
                aria-label="Search notes"
                spellcheck="false"
                @keydown.esc="onSearchEscape"
                @keydown.down.prevent="step(0)"
              />
              <button
                v-if="query"
                class="search-clear"
                aria-label="Clear search"
                @click="query = ''"
              >
                <X :size="11" :stroke-width="2" aria-hidden="true" />
              </button>
            </div>
            <button
              class="btn-icon sort-btn"
              aria-label="Sort notes"
              aria-haspopup="menu"
              :title="`Sort: ${sortLabel}`"
              @click="openSortMenu"
            >
              <ArrowUpDown :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <div class="layout-toggle" role="group" aria-label="Notes layout">
              <button
                class="on"
                aria-label="List layout"
                title="List layout"
                @click="layout = 'split'"
              >
                <Rows3 :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
              <button aria-label="Gallery layout" title="Gallery layout" @click="layout = 'gallery'">
                <LayoutGrid :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
            </div>
            <button class="btn-icon compose" aria-label="New note" title="New note" @click="createNote">
              <SquarePen :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>

          <div ref="listScroll" class="list-scroll">
            <button
              class="tag-row"
              :class="{ on: activeTag === null && !showArchived }"
              @click="showAll"
            >
              <StickyNote :size="12" :stroke-width="1.75" aria-hidden="true" />
              <span class="text-ellipsis">All Notes</span>
              <span class="tag-count">{{ activeCount }}</span>
            </button>
            <button class="tag-row" :class="{ on: showArchived }" @click="openArchive">
              <Archive :size="12" :stroke-width="1.75" aria-hidden="true" />
              <span class="text-ellipsis">Archive</span>
              <span class="tag-count">{{ archivedCount }}</span>
            </button>
            <template v-if="tagList.length">
              <p class="section-label list-label">Tags</p>
              <button
                v-for="t in tagList"
                :key="t.name"
                class="tag-row"
                :class="{ on: activeTag === t.name }"
                @click="toggleTag(t.name)"
              >
                <i class="tag-dot" :style="{ background: tagColor(t.name) }" aria-hidden="true"></i>
                <span class="text-ellipsis">{{ t.name }}</span>
                <span class="tag-count">{{ t.count }}</span>
              </button>
            </template>

            <TransitionGroup
              v-if="listItems.length"
              tag="div"
              class="note-list"
              :name="rowMotion ? 'row' : 'row-off'"
            >
              <div
                v-for="item in listItems"
                :key="item.key"
                class="list-item"
                data-cell
                :class="[
                  item.kind === 'note'
                    ? { 'is-note': true, 'is-selected': item.note.id === selectedId }
                    : null,
                  item.kind === 'note' ? cellClass(item.note.id) : null,
                ]"
                :style="item.kind === 'note' ? cellStyle(item.note.id) : undefined"
              >
                <p v-if="item.kind === 'label'" class="section-label list-label">{{ item.text }}</p>
                <!-- Select on press, like a native list: feedback on pointer-down -->
                <button
                  v-else
                  class="note-row"
                  :class="{
                    selected: item.note.id === selectedId,
                    dragging: drag?.id === item.note.id,
                  }"
                  :style="liftStyle(item.note.id)"
                  :data-note-id="item.note.id"
                  :aria-current="item.note.id === selectedId ? 'true' : undefined"
                  @mousedown.left="selectedId = item.note.id"
                  @pointerdown="onRowPointerDown($event, item.note)"
                  @click="onRowClick(item.note.id)"
                  @contextmenu.prevent="openMenu(item.note, $event)"
                  @keydown.down.prevent="onRowArrow($event, 1, item.note)"
                  @keydown.up.prevent="onRowArrow($event, -1, item.note)"
                >
                  <span class="row-title text-ellipsis">{{ item.note.title || 'New Note' }}</span>
                  <span class="row-sub">
                    <span class="row-date">{{ listDate(item.note.updated_at) }}</span>
                    {{ previewOf(item.note) || 'No additional text' }}
                  </span>
                </button>
              </div>
            </TransitionGroup>

            <div v-else-if="searching || activeTag || showArchived" class="list-empty">
              <p v-if="searching">No notes match “{{ query.trim() }}”</p>
              <p v-else-if="showArchived">No archived notes</p>
              <p v-else>No notes with this tag</p>
            </div>
            <div v-else class="list-empty">
              <p>No notes yet</p>
            </div>
          </div>

          <div
            ref="archiveZone"
            class="archive-zone"
            :class="{ on: drag !== null, over: drag?.overArchive }"
            aria-hidden="true"
          >
            <Archive :size="14" :stroke-width="1.75" aria-hidden="true" />
            {{ drag?.overArchive ? 'Release to archive' : 'Drop here to archive' }}
          </div>
        </aside>

        <section class="notes-editor">
          <NoteEditor
            v-if="selectedNote"
            ref="editorComp"
            :note="selectedNote"
            @remove="removeNote(selectedNote.id)"
          />
          <div v-else-if="notes.notes.length" class="editor-empty empty-state">
            <StickyNote :size="22" :stroke-width="1.5" aria-hidden="true" />
            <p>Select a note</p>
          </div>
          <div v-else class="editor-empty empty-state">
            <StickyNote :size="26" :stroke-width="1.5" aria-hidden="true" />
            <p class="empty-title">No Notes</p>
            <p class="empty-sub">Keep commands, server details and anything worth remembering.</p>
            <button class="btn btn-primary" @click="createNote">
              <SquarePen :size="13" :stroke-width="2" aria-hidden="true" />
              New note
            </button>
          </div>
        </section>
      </div>

      <!-- ==== Gallery: card grid + full-page editor ==== -->
      <div v-else key="gallery" class="gallery-root">
        <header class="gallery-toolbar">
          <div class="gallery-row">
            <div class="search-box">
              <Search :size="13" :stroke-width="1.5" aria-hidden="true" />
              <input
                v-model="query"
                type="text"
                placeholder="Search"
                aria-label="Search notes"
                spellcheck="false"
                @keydown.esc="onSearchEscape"
              />
              <button
                v-if="query"
                class="search-clear"
                aria-label="Clear search"
                @click="query = ''"
              >
                <X :size="11" :stroke-width="2" aria-hidden="true" />
              </button>
            </div>
            <div class="gallery-spacer"></div>
            <button
              class="btn-icon sort-btn"
              aria-label="Sort notes"
              aria-haspopup="menu"
              :title="`Sort: ${sortLabel}`"
              @click="openSortMenu"
            >
              <ArrowUpDown :size="14" :stroke-width="1.5" aria-hidden="true" />
            </button>
            <div class="layout-toggle" role="group" aria-label="Notes layout">
              <button aria-label="List layout" title="List layout" @click="layout = 'split'">
                <Rows3 :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
              <button
                class="on"
                aria-label="Gallery layout"
                title="Gallery layout"
                @click="layout = 'gallery'"
              >
                <LayoutGrid :size="13" :stroke-width="1.75" aria-hidden="true" />
              </button>
            </div>
            <button class="btn-icon compose" aria-label="New note" title="New note" @click="createNote">
              <SquarePen :size="15" :stroke-width="1.5" aria-hidden="true" />
            </button>
          </div>
          <div class="gallery-tags">
            <button
              class="gchip"
              :class="{ on: activeTag === null && !showArchived }"
              @click="showAll"
            >
              All
              <span class="gchip-count">{{ activeCount }}</span>
            </button>
            <button class="gchip" :class="{ on: showArchived }" @click="openArchive">
              <Archive :size="11" :stroke-width="1.75" aria-hidden="true" />
              Archive
              <span class="gchip-count">{{ archivedCount }}</span>
            </button>
            <button
              v-for="t in tagList"
              :key="t.name"
              class="gchip"
              :class="{ on: activeTag === t.name }"
              @click="toggleTag(t.name)"
            >
              <i class="tag-dot" :style="{ background: tagColor(t.name) }" aria-hidden="true"></i>
              {{ t.name }}
              <span class="gchip-count">{{ t.count }}</span>
            </button>
          </div>
        </header>

        <div ref="galleryScroll" class="gallery-scroll">
          <TransitionGroup
            v-if="matches.length"
            tag="div"
            class="card-grid"
            :class="{ intro: galleryIntro }"
            :name="rowMotion ? 'row' : 'row-off'"
          >
            <div
              v-for="(n, i) in matches"
              :key="n.id"
              class="card-cell"
              data-cell
              :class="cellClass(n.id)"
              :style="cellStyle(n.id)"
            >
            <button
              class="card"
              :class="{ dragging: drag?.id === n.id }"
              :data-note-id="n.id"
              :style="[{ '--d': `${Math.min(i, 12) * 28}ms` }, liftStyle(n.id)]"
              @click="onCardClick($event, n.id)"
              @pointerdown="onRowPointerDown($event, n)"
              @contextmenu.prevent="openMenu(n, $event)"
            >
              <span class="card-top">
                <span class="card-title">{{ n.title || 'New Note' }}</span>
                <Pin v-if="n.pinned" class="card-pin" :size="11" :stroke-width="1.75" aria-hidden="true" />
              </span>
              <span class="card-sub">{{ previewOf(n) || 'No additional text' }}</span>
              <span class="card-foot">
                <span class="card-date">{{ listDate(n.updated_at) }}</span>
                <span v-if="n.tags.length" class="card-dots">
                  <i
                    v-for="t in n.tags.slice(0, 4)"
                    :key="t"
                    class="tag-dot"
                    :style="{ background: tagColor(t) }"
                    :title="t"
                    aria-hidden="true"
                  ></i>
                </span>
              </span>
            </button>
            </div>
          </TransitionGroup>

          <div v-else-if="searching || activeTag || showArchived" class="list-empty gallery-empty">
            <p v-if="searching">No notes match “{{ query.trim() }}”</p>
            <p v-else-if="showArchived">No archived notes</p>
            <p v-else>No notes with this tag</p>
          </div>
          <div v-else class="list-empty gallery-empty">
            <StickyNote :size="26" :stroke-width="1.5" aria-hidden="true" />
            <p class="empty-title">No Notes</p>
            <p class="empty-sub">Keep commands, server details and anything worth remembering.</p>
            <button class="btn btn-primary" @click="createNote">
              <SquarePen :size="13" :stroke-width="2" aria-hidden="true" />
              New note
            </button>
          </div>
        </div>

        <div
          ref="archiveZone"
          class="archive-zone"
          :class="{ on: drag !== null, over: drag?.overArchive }"
          aria-hidden="true"
        >
          <Archive :size="14" :stroke-width="1.75" aria-hidden="true" />
          {{ drag?.overArchive ? 'Release to archive' : 'Drop here to archive' }}
        </div>

        <Transition name="page" :css="!instantClose">
          <div
            v-if="galleryNote"
            class="gallery-editor"
            :style="{ '--page-origin': pageOrigin }"
          >
            <NoteEditor
              ref="editorComp"
              :note="galleryNote"
              back
              @remove="removeNote(galleryNote.id)"
              @back="closeGallery()"
            />
          </div>
        </Transition>
      </div>
    </Transition>

    <ContextMenu v-if="menu" :x="menu.x" :y="menu.y" :items="menu.items" @close="menu = null" />
  </div>
</template>

<style scoped>
.notes {
  position: relative;
  display: flex;
  height: 100%;
  min-height: 0;
}

.split-root,
.gallery-root {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

/* Layout swap crossfade — quick, opacity only */
.swap-enter-active,
.swap-leave-active {
  transition: opacity 130ms ease;
}
.swap-enter-from,
.swap-leave-to {
  opacity: 0;
}

/* ==== shared chrome ==== */

.search-box {
  display: flex;
  align-items: center;
  gap: 6px;
  flex: 1;
  min-width: 0;
  background: var(--bg-input);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 5px 9px;
  color: var(--text-3);
  transition: border-color var(--dur-hover) ease, box-shadow var(--dur-hover) ease;
}

.search-box:hover {
  border-color: var(--border-strong);
}

.search-box:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-bg);
}

.search-box input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.search-box input::placeholder {
  color: var(--text-4);
}

.search-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: 999px;
  color: var(--text-4);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.search-clear:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.compose {
  flex-shrink: 0;
  color: var(--text-2);
}

.layout-toggle {
  display: flex;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  overflow: hidden;
  flex-shrink: 0;
}

.layout-toggle button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 27px;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.layout-toggle button:hover {
  color: var(--text-1);
  background: var(--bg-hover);
}

.layout-toggle button.on {
  background: var(--accent-bg);
  color: var(--accent);
}

.layout-toggle button + button {
  border-left: 1px solid var(--border);
}

.tag-dot {
  width: 6px;
  height: 6px;
  border-radius: 999px;
  flex-shrink: 0;
  display: inline-block;
}

.list-empty {
  padding: var(--space-6) var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-4);
}

.empty-title {
  font-size: var(--text-lg);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  color: var(--text-2);
}

.empty-sub {
  max-width: 36ch;
  font-size: var(--text-sm);
}

/* ==== Split mode ==== */

.notes-list {
  position: relative;
  display: flex;
  flex-direction: column;
  width: 264px;
  flex-shrink: 0;
  min-height: 0;
  border-right: 1px solid var(--border);
}

.list-toolbar {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-3) var(--space-2) var(--space-2);
  flex-shrink: 0;
}

.list-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 0 var(--space-2) var(--space-3);
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
}

.list-label {
  margin: var(--space-2) var(--space-1) var(--space-1);
}

.tag-row {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 100%;
  padding: 5px 9px;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-2);
  text-align: left;
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.tag-row:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.tag-row.on {
  background: var(--accent-bg);
  color: var(--accent);
}

.tag-count {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-4);
  flex-shrink: 0;
}

.note-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.list-item {
  position: relative;
}

/* Inset hairlines between rows, Apple Notes style — they step aside
   around the selected and hovered row so the highlight reads as one shape */
.list-item.is-note + .list-item.is-note::before {
  content: '';
  position: absolute;
  top: -1px;
  left: 11px;
  right: 11px;
  height: 1px;
  background: var(--border);
  pointer-events: none;
  transition: opacity var(--dur-hover) ease;
}

.list-item.is-selected::before,
.list-item.is-selected + .list-item::before {
  opacity: 0;
}

@media (hover: hover) and (pointer: fine) {
  .list-item.is-note:hover::before,
  .list-item.is-note:hover + .list-item::before {
    opacity: 0;
  }
}

/* Data-driven reorders glide (strong in-out: on-screen movement);
   new rows (create, undo) fade in where they land. Leaves are instant. */
.row-move {
  transition: transform 280ms var(--ease-move);
}

.row-enter-active {
  transition: opacity 200ms var(--ease-out);
}

.row-enter-from {
  opacity: 0;
}

.note-row {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
  width: 100%;
  padding: 9px 11px;
  border-radius: var(--radius-md);
  text-align: left;
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease;
}

.note-row:hover {
  background: var(--bg-hover);
}

/* Selection is neutral while focus is elsewhere (typing in the note) and
   takes the accent only while the list itself has focus — like macOS */
.note-row.selected {
  background: var(--bg-active);
}

.note-list:focus-within .note-row.selected {
  background: var(--accent-bg);
}

.row-title {
  display: block;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
}

.row-sub {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  font-size: var(--text-sm);
  line-height: 1.45;
  color: var(--text-3);
}

.row-date {
  margin-right: 4px;
  color: var(--text-2);
  font-variant-numeric: tabular-nums;
}

.notes-editor {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.editor-empty {
  flex: 1;
}

/* ==== Gallery mode ==== */

.gallery-root {
  position: relative;
  flex-direction: column;
}

.gallery-toolbar {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4) var(--space-2);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.gallery-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.gallery-row .search-box {
  max-width: 320px;
}

.gallery-spacer {
  flex: 1;
}

.gallery-tags {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 5px;
}

.gchip {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid transparent;
  font-size: var(--text-xs);
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease,
    border-color var(--dur-hover) ease;
}

.gchip:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.gchip.on {
  background: var(--accent-bg);
  border-color: color-mix(in oklch, var(--accent) 30%, transparent);
  color: var(--accent);
}

.gchip-count {
  font-size: 10px;
  opacity: 0.7;
}

.gallery-scroll {
  flex: 1;
  overflow-y: auto;
  padding: var(--space-4);
  min-height: 0;
}

.card-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(215px, 1fr));
  gap: 12px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  text-align: left;
  padding: 13px 14px 11px;
  min-height: 128px;
  border-radius: var(--radius-lg);
  border: 1px solid var(--border);
  background: var(--bg-surface);
  overflow: hidden;
  transition: transform var(--dur-press) var(--ease-out), border-color var(--dur-hover) ease,
    box-shadow var(--dur-hover) ease;
}

/* One-time entrance only (see galleryIntro) — never replays on filter */
.card-grid.intro .card {
  animation: card-in 280ms var(--ease-out) both;
  animation-delay: var(--d, 0ms);
}

/* Cells in a drag: the held one lifts under the pointer, the others slide
   to make room (`shift`). Settling on drop is handled by the script. */
.note-row,
.card {
  user-select: none;
}

.shift {
  transition: transform 200ms var(--ease-move);
}

.drag-src {
  z-index: 2;
}

.note-row.dragging,
.card.dragging {
  scale: 1.02;
  box-shadow: var(--shadow-3);
  cursor: grabbing;
  transition: scale 160ms var(--ease-out), box-shadow 160ms var(--ease-out);
}

/* Grid cell wrapper: the card fills it, so the cell is the moving unit */
.card-cell {
  position: relative;
  display: flex;
  min-width: 0;
}

.card-cell > .card {
  flex: 1;
  min-width: 0;
}

/* Drop zone for archiving: floats over the bottom of the list while a note is
   held, so showing it never shifts the rows (the snapshot depends on that) */
.archive-zone {
  position: absolute;
  left: 10px;
  right: 10px;
  bottom: 10px;
  z-index: 6;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  height: 52px;
  border-radius: var(--radius-lg);
  border: 1px dashed var(--border-strong);
  background: var(--bg-surface);
  box-shadow: var(--shadow-2);
  font-size: var(--text-sm);
  color: var(--text-2);
  opacity: 0;
  transform: translateY(12px);
  pointer-events: none;
  transition: opacity 160ms var(--ease-out), transform 200ms var(--ease-out),
    border-color var(--dur-hover) ease, background-color var(--dur-hover) ease,
    color var(--dur-hover) ease;
}

.archive-zone.on {
  opacity: 1;
  transform: none;
}

.archive-zone.over {
  border-style: solid;
  border-color: var(--accent);
  background: var(--accent-bg);
  color: var(--accent);
}

/* Gallery: the zone is centred under the grid instead of spanning a column */
.gallery-root > .archive-zone {
  left: 50%;
  right: auto;
  bottom: 16px;
  width: 320px;
  margin-left: -160px;
  transform: translateY(12px);
}

.gallery-root > .archive-zone.on {
  transform: none;
}

:global(body.notes-dragging),
:global(body.notes-dragging *) {
  cursor: grabbing !important;
  user-select: none !important;
}

/* Hover is hit constantly while scanning the grid: no lift, just the
   edge and shadow coming up */
@media (hover: hover) and (pointer: fine) {
  .card:hover {
    border-color: var(--border-strong);
    box-shadow: var(--shadow-2);
  }
}

.card:active {
  transform: scale(0.97);
}

@keyframes card-in {
  from {
    opacity: 0;
    transform: translateY(8px);
  }
}

.card-top {
  display: flex;
  align-items: flex-start;
  gap: 6px;
}

.card-title {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  overflow: hidden;
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-1);
  line-height: 1.35;
  flex: 1;
  min-width: 0;
}

.card-pin {
  color: var(--accent);
  flex-shrink: 0;
  margin-top: 2px;
}

.card-sub {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
  font-size: var(--text-sm);
  line-height: 1.5;
  color: var(--text-3);
  flex: 1;
}

.card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: auto;
}

.card-date {
  font-size: var(--text-xs);
  color: var(--text-4);
}

.card-dots {
  display: flex;
  gap: 4px;
}

.gallery-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-7) var(--space-4);
}

/* Full-page editor over the grid — grows out of the card that opened it
   (transform-origin = that card's center) and shrinks back into it */
.gallery-editor {
  position: absolute;
  inset: 0;
  z-index: 5;
  display: flex;
  flex-direction: column;
  background: var(--bg);
  transform-origin: var(--page-origin, 50% 30%);
}

.page-enter-active {
  transition: opacity 200ms var(--ease-out), transform 260ms var(--ease-out);
}

/* exit: same path back, softer and faster */
.page-leave-active {
  transition: opacity 160ms var(--ease-out), transform 160ms var(--ease-out);
}

.page-enter-from {
  opacity: 0;
  transform: scale(0.92);
}

.page-leave-to {
  opacity: 0;
  transform: scale(0.95);
}

@media (prefers-reduced-motion: reduce) {
  .card-grid.intro .card {
    animation: none;
  }

  .row-move,
  .shift {
    transition: none;
  }

  .page-enter-active,
  .page-leave-active,
  .swap-enter-active,
  .swap-leave-active {
    transition: opacity 130ms ease;
  }

  .page-enter-from,
  .page-leave-to {
    transform: none;
  }
}
</style>
