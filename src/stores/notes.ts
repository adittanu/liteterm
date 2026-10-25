import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import { useUiStore } from './ui'
import { errorText } from './sessions'
import { extractTags } from '../utils/tags'
import type { Note } from '../types'

export const useNotesStore = defineStore('notes', () => {
  const notes = ref<Note[]>([])
  const loaded = ref(false)

  // Debounced persistence: the list reflects edits instantly, the
  // database catches up once typing pauses.
  const saveTimers = new Map<number, ReturnType<typeof setTimeout>>()

  async function load() {
    notes.value = await api.listNotes()
    loaded.value = true
    // Tags derive from #hashtags in the body — backfill notes that were
    // saved before this derivation existed (or never edited since).
    for (const n of notes.value) {
      const tags = extractTags(n.body)
      if (tags.join('\u0000') !== n.tags.join('\u0000')) {
        n.tags = tags
        void api.setNoteTags(n.id, tags).catch(() => {})
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
    notes.value.unshift(note)
    return note
  }

  function edit(id: number, title: string, body: string) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    note.title = title
    note.body = body
    // Tags are whatever `#words` the body contains — keep the column
    // in sync so folders and card dots follow the text.
    const tags = extractTags(body)
    const changed = tags.join('\u0000') !== note.tags.join('\u0000')
    if (changed) note.tags = tags
    const prev = saveTimers.get(id)
    if (prev) clearTimeout(prev)
        saveTimers.set(
          id,
          setTimeout(() => {
            saveTimers.delete(id)
            void api
              .updateNote(id, title, body)
              .then((saved) => {
                const n = notes.value.find((x) => x.id === id)
                if (n) n.updated_at = saved.updated_at
              })
              .catch((e) => {
                if (notes.value.some((x) => x.id === id)) {
                  useUiStore().toast('error', `Could not save note: ${errorText(e)}`)
                }
              })
            if (changed) {
              void api.setNoteTags(id, tags).catch((e) => {
                useUiStore().toast('error', `Could not tag note: ${errorText(e)}`)
              })
            }
          }, 500),
        )
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
    const timer = saveTimers.get(id)
    if (timer) {
      clearTimeout(timer)
      saveTimers.delete(id)
    }
    notes.value = notes.value.filter((n) => n.id !== id)
    return () => {
      void api
        .createNote(note.title, note.body)
        .then(async (restored) => {
          if (note.tags.length) await api.setNoteTags(restored.id, note.tags)
          restored.tags = note.tags
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

  return {
    notes,
    loaded,
    load,
    create,
    edit,
    setPinned,
    setArchived,
    reorder,
    setHost,
    forgetHost,
    setTags,
    remove,
  }
})
