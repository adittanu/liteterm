import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import type { Credential, CredentialInput } from '../types'

export const useCredentialsStore = defineStore('credentials', () => {
  const credentials = ref<Credential[]>([])
  const loaded = ref(false)

  async function load() {
    credentials.value = await api.listCredentials()
    loaded.value = true
  }

  async function save(id: number | null, input: CredentialInput): Promise<Credential> {
    const cred = await api.upsertCredential(id, input)
    const i = credentials.value.findIndex((c) => c.id === cred.id)
    if (i === -1) credentials.value.push(cred)
    else credentials.value[i] = cred
    return cred
  }

  async function remove(id: number) {
    await api.deleteCredential(id)
    credentials.value = credentials.value.filter((c) => c.id !== id)
  }

  function byId(id: number | null): Credential | undefined {
    if (id === null) return undefined
    return credentials.value.find((c) => c.id === id)
  }

  return { credentials, loaded, load, save, remove, byId }
})
