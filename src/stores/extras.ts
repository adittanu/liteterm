import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { useUiStore } from './ui'
import type { Snippet, PortForward } from '../types'

export const useSnippetsStore = defineStore('snippets', () => {
  const snippets = ref<Snippet[]>([])
  const loaded = ref(false)

  async function load() {
    snippets.value = await api.listSnippets()
    loaded.value = true
  }

  /** `teamId` is the vault a new snippet goes in (null: personal) */
  async function save(name: string, command: string, id: number | null = null, teamId: string | null = null) {
    const s = await api.upsertSnippet(id, name, command, teamId)
    const i = snippets.value.findIndex((x) => x.id === s.id)
    if (i === -1) snippets.value.push(s)
    else snippets.value[i] = s
  }

  async function remove(id: number) {
    await api.deleteSnippet(id)
    snippets.value = snippets.value.filter((s) => s.id !== id)
  }

  /** To another vault (null: personal); it comes back as a new row */
  async function move(id: number, teamId: string | null) {
    const moved = await api.moveSnippet(id, teamId)
    snippets.value = [...snippets.value.filter((s) => s.id !== id), moved]
    return moved
  }

  return { snippets, loaded, load, save, remove, move }
})

export const useForwardsStore = defineStore('forwards', () => {
  const forwards = ref<PortForward[]>([])
  const activeIds = ref<number[]>([])
  const loaded = ref(false)

  async function load() {
    const [f, a] = await Promise.all([api.listForwards(), api.listActiveForwards()])
    forwards.value = f
    activeIds.value = a
    loaded.value = true
  }

  // The backend starts auto-start forwards on connect and stops them when
  // their session ends — re-read which tunnels are really up.
  async function refreshActive() {
    activeIds.value = await api.listActiveForwards()
  }

  // …and it says so as it happens, so follow those events instead of waiting
  // for the next poll. A tunnel can end on its own (its SSH session closed).
  async function initEvents() {
    await listen<{ id: number; active: boolean }>('forward:started', (e) => {
      if (!activeIds.value.includes(e.payload.id)) activeIds.value.push(e.payload.id)
    })
    await listen<{ id: number; active: boolean; error?: string }>('forward:stopped', (e) => {
      activeIds.value = activeIds.value.filter((x) => x !== e.payload.id)
      // The app stopped it by itself (a remote forward the server refused
      // again after a reconnect): say so, or it just goes quiet
      if (e.payload.error) {
        const name = forwards.value.find((f) => f.id === e.payload.id)?.name ?? 'A tunnel'
        useUiStore().toast('error', `“${name}”: ${e.payload.error}`)
      }
    })
  }

  /** A forward is in its host's vault, so there is no vault to pass */
  async function save(input: Omit<PortForward, 'id' | 'team_id'> & { id?: number }) {
    // A running tunnel keeps listening on its old port and target, whatever the
    // saved row now says. Stop it, so the row cannot claim "Listening" on an
    // address nothing serves any more.
    if (input.id !== undefined && activeIds.value.includes(input.id)) {
      await stop(input.id)
      useUiStore().toast('info', `“${input.name}” was stopped to apply the changes — start it again`)
    }
    const f = await api.upsertForward(
      input.id ?? null,
      input.host_id,
      input.name,
      input.kind,
      input.local_port,
      input.remote_host,
      input.remote_port,
      input.auto_start,
    )
    const i = forwards.value.findIndex((x) => x.id === f.id)
    if (i === -1) forwards.value.push(f)
    else forwards.value[i] = f
  }

  async function remove(id: number) {
    if (activeIds.value.includes(id)) await api.forwardStop(id)
    await api.deleteForward(id)
    forwards.value = forwards.value.filter((f) => f.id !== id)
    activeIds.value = activeIds.value.filter((x) => x !== id)
  }

  async function start(id: number) {
    await api.forwardStart(id)
    if (!activeIds.value.includes(id)) activeIds.value.push(id)
  }

  async function stop(id: number) {
    await api.forwardStop(id)
    activeIds.value = activeIds.value.filter((x) => x !== id)
  }

  return { forwards, activeIds, loaded, load, refreshActive, initEvents, save, remove, start, stop }
})
