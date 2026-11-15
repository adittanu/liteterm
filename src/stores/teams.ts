import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { useCloudStore } from './cloud'
import { errorText } from './sessions'
import type { TeamRole, TeamsView } from '../types'

/**
 * Team vaults. The backend owns the keys and syncs the teams' data with the
 * personal vault; this holds the teams list, the invites for this account and
 * its safety code. Loaded the first time something asks for it: it reads
 * invites from the server.
 */
export const useTeamsStore = defineStore('teams', () => {
  const view = ref<TeamsView | null>(null)
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function init() {
    // The backend says when the teams this computer knows changed
    await listen('cloud:teams', () => {
      if (loaded.value) void load().catch(() => {})
    })
    // Teams need the vault unlocked; signing out drops them on this computer
    watch(
      () => useCloudStore().status.state,
      (state) => {
        if (state !== 'ready') {
          view.value = null
          loaded.value = false
        }
      },
    )
  }

  async function load() {
    loading.value = true
    try {
      view.value = await api.cloudTeams()
      error.value = null
      loaded.value = true
    } catch (e) {
      error.value = errorText(e)
      throw e
    } finally {
      loading.value = false
    }
  }

  async function createTeam(name: string) {
    const team = await api.cloudCreateTeam(name)
    await load()
    return team
  }

  async function renameTeam(teamId: string, name: string) {
    await api.cloudRenameTeam(teamId, name)
    await load()
  }

  async function deleteTeam(teamId: string) {
    await api.cloudDeleteTeam(teamId)
    await load()
  }

  async function leaveTeam(teamId: string) {
    await api.cloudLeaveTeam(teamId)
    await load()
  }

  async function acceptInvite(inviteId: string) {
    await api.cloudAcceptInvite(inviteId)
    await load()
  }

  async function declineInvite(inviteId: string) {
    await api.cloudCancelInvite(inviteId)
    await load()
  }

  /** After an owner changed their own role: the local role comes from a sync */
  async function syncRoles() {
    await useCloudStore().syncNow()
    await load()
  }

  const roleOf = (teamId: string): TeamRole | null =>
    view.value?.teams.find((t) => t.id === teamId)?.role ?? null

  return {
    view,
    loaded,
    loading,
    error,
    init,
    load,
    createTeam,
    renameTeam,
    deleteTeam,
    leaveTeam,
    acceptInvite,
    declineInvite,
    syncRoles,
    roleOf,
  }
})
