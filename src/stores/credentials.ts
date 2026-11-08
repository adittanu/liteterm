import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api } from '../services/api'
import { warnIfPasswordNotKept } from '../utils/savedSecret'
import { useHostsStore } from './hosts'
import type { Credential, CredentialInput } from '../types'

export const useCredentialsStore = defineStore('credentials', () => {
  const credentials = ref<Credential[]>([])
  const loaded = ref(false)

  async function load() {
    credentials.value = await api.listCredentials()
    loaded.value = true
  }

  // Hosts that use a credential show its username and auth method, and the
  // backend resolves them through it; after the credential changes (or goes)
  // the host list has to be read again or it keeps showing the old login.
  function refreshHosts() {
    void useHostsStore()
      .load()
      .catch(() => {})
  }

  async function save(id: number | null, input: CredentialInput): Promise<Credential> {
    const cred = await api.upsertCredential(id, input)
    const i = credentials.value.findIndex((c) => c.id === cred.id)
    if (i === -1) credentials.value.push(cred)
    else credentials.value[i] = cred
    warnIfPasswordNotKept(input, cred.has_password)
    if (id !== null) refreshHosts()
    return cred
  }

  async function remove(id: number) {
    await api.deleteCredential(id)
    credentials.value = credentials.value.filter((c) => c.id !== id)
    // The database detached the hosts that used it (ON DELETE SET NULL)
    refreshHosts()
  }

  function byId(id: number | null): Credential | undefined {
    if (id === null) return undefined
    return credentials.value.find((c) => c.id === id)
  }

  return { credentials, loaded, load, save, remove, byId }
})
