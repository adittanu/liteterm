import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import { useUiStore } from './ui'
import { errorText } from './sessions'
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
  }

  async function create(title = '', body = ''): Promise<Note> {
    const note = await api.createNote(title, body)
    notes.value.unshift(note)
    return note
  }

  function edit(id: number, title: string, body: string) {
    const note = notes.value.find((n) => n.id === id)
    if (!note) return
    note.title = title
    note.body = body
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
        .then((restored) => {
          notes.value.unshift(restored)
        })
        .catch((e) => useUiStore().toast('error', `Could not restore note: ${errorText(e)}`))
    }
  }

  return { notes, loaded, load, create, edit, setPinned, remove }
})
