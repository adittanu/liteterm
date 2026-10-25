import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import type { Host, HostGroup, HostInput } from '../types'

export const useHostsStore = defineStore('hosts', () => {
  const hosts = ref<Host[]>([])
  const groups = ref<HostGroup[]>([])
  const loaded = ref(false)

  async function load() {
    const [h, g] = await Promise.all([api.listHosts(), api.listGroups()])
    hosts.value = h
    groups.value = g
    loaded.value = true
  }

  // Live OS detection results from the backend
  async function initEvents() {
    await listen<{ hostId: number; os: string }>('host:os', (e) => {
      const host = hosts.value.find((h) => h.id === e.payload.hostId)
      if (host) host.os = e.payload.os
    })
  }

  async function saveHost(input: HostInput): Promise<Host> {
    const host = await api.upsertHost(input)
    const i = hosts.value.findIndex((x) => x.id === host.id)
    if (i === -1) hosts.value.push(host)
    else hosts.value[i] = host
    return host
  }

  async function removeHost(id: number) {
    await api.deleteHost(id)
    hosts.value = hosts.value.filter((h) => h.id !== id)
  }

  async function saveGroup(id: number | null, name: string): Promise<HostGroup> {
    const group = await api.upsertGroup(id, name)
    const i = groups.value.findIndex((g) => g.id === group.id)
    if (i === -1) groups.value.push(group)
    else groups.value[i] = group
    return group
  }

  async function removeGroup(id: number) {
    await api.deleteGroup(id)
    groups.value = groups.value.filter((g) => g.id !== id)
    hosts.value = hosts.value.map((h) =>
      h.group_id === id ? { ...h, group_id: null } : h,
    )
  }

  function hostById(id: number | null): Host | undefined {
    if (id === null) return undefined
    return hosts.value.find((h) => h.id === id)
  }

  // Local mirror of the backend connect-time bump so the list re-sorts
  // immediately without a refetch
  function touchHost(id: number) {
    const host = hosts.value.find((h) => h.id === id)
    if (host) host.last_used_at = Date.now()
  }

  return {
    hosts,
    groups,
    loaded,
    load,
    initEvents,
    saveHost,
    removeHost,
    saveGroup,
    removeGroup,
    hostById,
    touchHost,
  }
})
