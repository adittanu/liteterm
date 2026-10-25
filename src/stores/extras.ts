import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import type { Snippet, PortForward } from '../types'

export const useSnippetsStore = defineStore('snippets', () => {
  const snippets = ref<Snippet[]>([])
  const loaded = ref(false)

  async function load() {
    snippets.value = await api.listSnippets()
    loaded.value = true
  }

  async function save(name: string, command: string, id: number | null = null) {
    const s = await api.upsertSnippet(id, name, command)
    const i = snippets.value.findIndex((x) => x.id === s.id)
    if (i === -1) snippets.value.push(s)
    else snippets.value[i] = s
  }

  async function remove(id: number) {
    await api.deleteSnippet(id)
    snippets.value = snippets.value.filter((s) => s.id !== id)
  }

  return { snippets, loaded, load, save, remove }
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

  // The backend starts auto-start forwards on connect and drops them when a
  // session ends — re-read which tunnels are really up.
  async function refreshActive() {
    activeIds.value = await api.listActiveForwards()
  }

  async function save(input: Omit<PortForward, 'id'> & { id?: number }) {
    const f = await api.upsertForward(
      input.id ?? null,
      input.host_id,
      input.name,
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

  return { forwards, activeIds, loaded, load, refreshActive, save, remove, start, stop }
})
