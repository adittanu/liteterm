import { defineStore } from 'pinia'
import { ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { useHostsStore } from './hosts'
import { useCredentialsStore } from './credentials'
import { useSnippetsStore, useForwardsStore } from './extras'
import { useNotesStore } from './notes'
import type { CloudStatus } from '../types'

/**
 * The optional account. The backend owns the session, the vault key and the
 * sync loop; this mirrors its status and reloads whatever a sync changed.
 */
export const useCloudStore = defineStore('cloud', () => {
  const status = ref<CloudStatus>({
    state: 'signedOut',
    email: null,
    syncing: false,
    lastSyncAt: null,
    error: null,
    pending: 0,
  })
  /** Bumped when synced data lands, for views that read on mount (known hosts) */
  const dataVersion = ref(0)

  async function init() {
    await listen<CloudStatus>('cloud:status', (e) => {
      status.value = e.payload
    })
    await listen<string[]>('cloud:changed', (e) => void reload(e.payload))
    status.value = await api.cloudStatus()
  }

  async function reload(kinds: string[]) {
    const has = (k: string) => kinds.includes(k)
    const jobs: Promise<unknown>[] = []
    // Hosts show their group and their credential's login
    if (has('host') || has('group') || has('credential')) jobs.push(useHostsStore().load())
    if (has('credential')) jobs.push(useCredentialsStore().load())
    if (has('snippet')) jobs.push(useSnippetsStore().load())
    if (has('port_forward') || has('host')) jobs.push(useForwardsStore().load())
    const notes = useNotesStore()
    if (notes.loaded && (has('note') || has('host'))) jobs.push(notes.reloadFromSync())
    dataVersion.value++
    await Promise.allSettled(jobs)
  }

  function set(next: CloudStatus) {
    status.value = next
    return next
  }

  const sendCode = (email: string) => api.cloudSendCode(email)
  const verifyCode = async (email: string, code: string) => set(await api.cloudVerifyCode(email, code))
  const unlock = async (password: string) => set(await api.cloudUnlock(password))
  const resetVault = async () => set(await api.cloudResetVault())
  const signOut = async () => set(await api.cloudSignOut())
  const syncNow = async () => set(await api.cloudSyncNow())
  const refresh = async () => set(await api.cloudStatus())

  /** Resolves to the recovery key, to show once */
  async function createVault(password: string): Promise<string> {
    const recoveryKey = await api.cloudCreateVault(password)
    await refresh()
    return recoveryKey
  }

  /** Resolves to the new recovery key, to show once */
  async function recover(recoveryKey: string, newPassword: string): Promise<string> {
    const next = await api.cloudRecover(recoveryKey, newPassword)
    await refresh()
    return next
  }

  const changePassword = (current: string, next: string) => api.cloudChangePassword(current, next)

  return {
    status,
    dataVersion,
    init,
    sendCode,
    verifyCode,
    createVault,
    unlock,
    recover,
    changePassword,
    resetVault,
    signOut,
    syncNow,
    refresh,
  }
})
