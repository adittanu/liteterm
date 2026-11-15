import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '../services/api'
import { useCloudStore } from './cloud'
import { errorText } from './sessions'
import type { TeamRole, TeamSummary, TeamsView } from '../types'

/**
 * Team vaults. The backend owns the keys and syncs the teams' data with the
 * personal vault. This holds the teams this computer is in (read locally, for
 * the hosts view and the vault pickers) and, for Settings → Teams, the
 * invites for this account and its safety code (read from the server, so
 * loaded only when that page asks).
 */
export const useTeamsStore = defineStore('teams', () => {
  /** The teams this computer is in, as of its last sync */
  const list = ref<TeamSummary[]>([])
  const view = ref<TeamsView | null>(null)
  const loaded = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)

  async function init() {
    // The backend says when the teams this computer knows changed
    await listen('cloud:teams', () => {
      void loadList()
      if (loaded.value) void load().catch(() => {})
    })
    // Signing out drops the teams on this computer
    watch(
      () => useCloudStore().status.state,
      (state) => {
        void loadList()
        if (state !== 'ready') {
          view.value = null
          loaded.value = false
        }
      },
    )
    await loadList()
  }

  async function loadList() {
    try {
      list.value = await api.listTeams()
    } catch {
      /* the database is busy; the next event reads it again */
    }
  }

  async function load() {
    loading.value = true
    try {
      view.value = await api.cloudTeams()
      list.value = view.value.teams
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

  // ---- vaults: where an item is, and whether it can be changed here ----

  const roleOf = (teamId: string): TeamRole | null => list.value.find((t) => t.id === teamId)?.role ?? null

  /** The personal vault always; a team's unless this account only views it */
  function canEdit(teamId: string | null): boolean {
    if (teamId === null) return true
    const role = roleOf(teamId)
    return role !== null && role !== 'viewer'
  }

  function teamName(teamId: string | null): string {
    if (teamId === null) return 'Personal'
    return list.value.find((t) => t.id === teamId)?.name ?? 'A team'
  }

  /** Teams this account can add to and move into */
  const writable = computed(() => list.value.filter((t) => t.role !== 'viewer'))

  /** Vaults a new item can go in, for the pickers; one entry means no choice */
  const vaultOptions = computed(() => [
    { value: null as string | null, label: 'Personal', hint: 'only you' },
    ...writable.value.map((t) => ({ value: t.id as string | null, label: t.name, hint: 'team' })),
  ])

  return {
    list,
    view,
    loaded,
    loading,
    error,
    init,
    loadList,
    load,
    createTeam,
    renameTeam,
    deleteTeam,
    leaveTeam,
    acceptInvite,
    declineInvite,
    syncRoles,
    roleOf,
    canEdit,
    teamName,
    writable,
    vaultOptions,
  }
})
