import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import { useUiStore } from './ui'
import { errorText } from './sessions'
import { extractTags } from '../utils/tags'
import type { Note } from '../types'

const tagsKey = (tags: string[]) => tags.join('\u0000')

// An edit waiting out the debounce. The payload is kept so it can be written
// at once when the window closes instead of being lost with its timer.
interface PendingSave {
  timer: ReturnType<typeof setTimeout>
  title: string
  body: string
  tags: string[]
}

export const useNotesStore = defineStore('notes', () => {
  const notes = ref<Note[]>([])
  const loaded = ref(false)

  // Debounced persistence: the list reflects edits instantly, the
  // database catches up once typing pauses.
  const pending = new Map<number, PendingSave>()

  // The tags the database holds per note. Comparing against this — not
  // against "did *this* keystroke change them" — means a tag change can't be
  // lost when a later edit lands inside the debounce window and replaces the
  // earlier timer.
  const persistedTags = new Map<number, string>()

  async function load() {
    notes.value = await api.listNotes()
    loaded.value = true
    // Tags derive from #hashtags in the body — backfill notes that were
    // saved before this derivation existed (or never edited since).
    for (const n of notes.value) {
      persistedTags.set(n.id, tagsKey(n.tags))
      const tags = extractTags(n.body)
      if (tagsKey(tags) !== tagsKey(n.tags)) {
        n.tags = tags
        void api
          .setNoteTags(n.id, tags)
          .then(() => persistedTags.set(n.id, tagsKey(tags)))
          .catch(() => {})
      }
    }
  }

  async function create(title = '', body = ''): Promise<Note> {
    let note = await api.createNote(title, body)
    const tags = extractTags(body)
    if (tags.length) {
      try {
        note = await api.setNoteTags(note.id, tags)
      } catch {
        /* the note exists; the tags sync again on first edit */
      }
    }
    persistedTags.set(note.id, tagsKey(note.tags))
    notes.value.unshift(note)
    return note
  }

  // Write one note to the database: its text, and its tags when they differ
  // from what the database holds
  async function persist(id: number, title: string, body: string, tags: string[]) {
    const key = tagsKey(tags)
    const jobs: Promise<unknown>[] = [
      api
        .updateNote(id, title, body)
        .then((saved) => {
          const n = notes.value.find((x) => x.id === id)
          if (n) n.updated_at = saved.updated_at
        })
        .catch((e) => {
          if (notes.value.some((x) => x.id === id)) {
            useUiStore().toast('error', `Could not save note: ${errorText(e)}`)
          }
        }),
    ]
    if (persistedTags.get(id) !== key) {
      jobs.push(
        api
          .setNoteTags(id, tags)
          .then(() => {
            persistedTags.set(id, key)
          })
          .catch((e) => {
            useUiStore().toast('error', `Could not tag note: ${errorText(e)}`)
          }),
      )
    }
    await Promise.all(jobs)
  }

  function edit(id: number, title: string, body: string) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    note.title = title
    note.body = body
    // Tags are whatever `#words` the body contains — keep the column
    // in sync so folders and card dots follow the text.
    const tags = extractTags(body)
    if (tagsKey(tags) !== tagsKey(note.tags)) note.tags = tags
    const prev = pending.get(id)
    if (prev) clearTimeout(prev.timer)
    const timer = setTimeout(() => {
      pending.delete(id)
      void persist(id, title, body, tags)
    }, 500)
    pending.set(id, { timer, title, body, tags })
  }

  /** Write every edit that is still waiting out its debounce. Called before
   *  the window closes or the app restarts to update — otherwise the last
   *  half second of typing is lost with the timer. */
  async function flushPending() {
    const waiting = [...pending.entries()]
    pending.clear()
    for (const [, p] of waiting) clearTimeout(p.timer)
    await Promise.all(waiting.map(([id, p]) => persist(id, p.title, p.body, p.tags)))
  }

  async function setPinned(id: number, pinned: boolean) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    note.pinned = pinned
    try {
      await api.setNotePinned(id, pinned)
    } catch (e) {
      note.pinned = !pinned
      throw e
    }
  }

  // Manual order: `ids` is every note, top to bottom. Applied at once and
  // rolled back if saving fails.
  async function reorder(ids: number[]) {
    const prev = new Map<number, number>()
    ids.forEach((id, i) => {
      const note = notes.value.find((n) => n.id === id)
      if (!note) return
      prev.set(id, note.sort)
      note.sort = i
    })
    try {
      await api.reorderNotes(ids)
    } catch (e) {
      for (const [id, sort] of prev) {
        const note = notes.value.find((n) => n.id === id)
        if (note) note.sort = sort
      }
      throw e
    }
  }

  // Archived notes leave All Notes and show under Archive; no timestamp change
  async function setArchived(id: number, archived: boolean) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    note.archived = archived
    try {
      await api.setNoteArchived(id, archived)
    } catch (e) {
      note.archived = !archived
      throw e
    }
  }

  // Link (or unlink) a note to a host — shown in that host's details panel
  async function setHost(id: number, hostId: number | null) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    const prev = note.host_id
    note.host_id = hostId
    try {
      await api.setNoteHost(id, hostId)
    } catch (e) {
      note.host_id = prev
      throw e
    }
  }

  // A host was deleted: the database already nulled the links (ON DELETE
  // SET NULL); mirror that here without a reload
  function forgetHost(hostId: number) {
    for (const n of notes.value) if (n.host_id === hostId) n.host_id = null
  }

  async function setTags(id: number, tags: string[]) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    const prev = note.tags
    note.tags = tags
    try {
      await api.setNoteTags(id, tags)
      persistedTags.set(id, tagsKey(tags))
    } catch (e) {
      note.tags = prev
      throw e
    }
  }

  // Delete is immediate; the returned closure recreates the note if the
  // user hits Undo within the toast window.
  async function remove(id: number): Promise<() => void> {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return () => {}
    await api.deleteNote(id)
    // A pending autosave for a deleted note would fail on a missing row
    const waiting = pending.get(id)
    if (waiting) {
      clearTimeout(waiting.timer)
      pending.delete(id)
    }
    persistedTags.delete(id)
    notes.value = notes.value.filter((n) => n.id !== id)
    // The toast stays clickable while it fades out; a second click must not
    // restore the note twice
    let restoring = false
    return () => {
      if (restoring) return
      restoring = true
      void api
        .createNote(note.title, note.body)
        .then(async (restored) => {
          if (note.tags.length) await api.setNoteTags(restored.id, note.tags)
          restored.tags = note.tags
          persistedTags.set(restored.id, tagsKey(note.tags))
          // Pinned / archived are separate columns the new row starts without
          if (note.pinned) {
            await api
              .setNotePinned(restored.id, true)
              .then(() => (restored.pinned = true))
              .catch(() => {})
          }
          if (note.archived) {
            await api
              .setNoteArchived(restored.id, true)
              .then(() => (restored.archived = true))
              .catch(() => {})
          }
          // The host may be gone by now — keep the link only if it saved
          if (note.host_id !== null) {
            await api
              .setNoteHost(restored.id, note.host_id)
              .then(() => (restored.host_id = note.host_id))
              .catch(() => {})
          }
          notes.value.unshift(restored)
        })
        .catch((e) => useUiStore().toast('error', `Could not restore note: ${errorText(e)}`))
    }
  }

  /** A sync brought edits from another device. Whatever is still waiting
   *  out its debounce is saved first, so the reload can't show it reverted. */
  async function reloadFromSync() {
    await flushPending()
    await load()
  }

  return {
    notes,
    loaded,
    load,
    reloadFromSync,
    create,
    edit,
    flushPending,
    setPinned,
    setArchived,
    reorder,
    setHost,
    forgetHost,
    setTags,
    remove,
  }
})
