// "Move to <team>" for hosts and groups: the menu entries, the confirmation
// that says what goes along and who can read it, and the toast after.
import { User, Users } from 'lucide-vue-next'
import type { MenuItem } from '../components/ContextMenu.vue'
import { useHostsStore } from '../stores/hosts'
import { useCredentialsStore } from '../stores/credentials'
import { useForwardsStore } from '../stores/extras'
import { useDialogsStore } from '../stores/dialogs'
import { useTeamsStore } from '../stores/teams'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import type { Host, HostGroup, MoveResult } from '../types'

/** One entry per vault the item can move to; none when it can't move */
export function moveItems(from: string | null, move: (to: string | null) => void): MenuItem[] {
  const teams = useTeamsStore()
  if (!teams.canEdit(from)) return []
  const items: MenuItem[] = teams.writable
    .filter((t) => t.id !== from)
    .map((t) => ({ label: `Move to ${t.name}`, icon: Users, action: () => move(t.id) }))
  if (from !== null) items.push({ label: 'Move to Personal', icon: User, action: () => move(null) })
  return items
}

export const hostMoveItems = (host: Host): MenuItem[] =>
  moveItems(host.team_id, (to) => void moveHosts([host], null, to))

export const groupMoveItems = (group: HostGroup): MenuItem[] =>
  moveItems(group.team_id, (to) => void moveHosts([], group, to))

const quote = (names: string[]) => names.map((n) => `“${n}”`).join(', ')

/** Confirm, move, and say how it went. Resolves to the result, or null. */
export async function moveHosts(list: Host[], group: HostGroup | null, to: string | null): Promise<MoveResult | null> {
  const hosts = useHostsStore()
  const credentials = useCredentialsStore()
  const teams = useTeamsStore()
  const dialogs = useDialogsStore()
  const ui = useUiStore()

  const moving = group ? hosts.hosts.filter((h) => h.group_id === group.id) : list
  const ids = new Set(moving.map((h) => h.id))
  const from = group ? group.team_id : (list[0]?.team_id ?? null)
  const toName = to === null ? 'your personal vault' : teams.teamName(to)
  const what = group ? `group “${group.name}”` : moving.length === 1 ? `“${moving[0].label}”` : `${moving.length} hosts`
  const one = moving.length === 1 && !group

  const lines: string[] = []
  if (to !== null) {
    lines.push(
      `Everyone in ${toName} can connect to ${one ? 'it' : 'them'} and read the passwords saved for ${one ? 'it' : 'them'}.`,
    )
  }
  if (from !== null) lines.push(`${one ? 'It leaves' : 'They leave'} ${teams.teamName(from)} for every member.`)
  const creds = [...new Set(moving.map((h) => h.credential_id).filter((c): c is number => c !== null))]
    .map((c) => credentials.byId(c)?.name)
    .filter((n): n is string => !!n)
  if (creds.length) lines.push(`${creds.length === 1 ? 'Credential' : 'Credentials'} ${quote(creds)} go${creds.length === 1 ? 'es' : ''} along.`)
  const forwardCount = useForwardsStore().forwards.filter((f) => ids.has(f.host_id)).length
  if (forwardCount) lines.push(`${forwardCount === 1 ? 'Its port forward goes' : `${forwardCount} port forwards go`} along, and stop if running.`)
  const strandedJumps = moving.filter((h) => h.jump_host_id !== null && !ids.has(h.jump_host_id))
  if (strandedJumps.length) {
    lines.push(`${quote(strandedJumps.map((h) => h.label))} will connect directly: ${strandedJumps.length === 1 ? 'its jump host stays' : 'their jump hosts stay'} behind.`)
  }
  lines.push(`Notes linked to ${one ? 'it' : 'them'} stay where they are, unlinked.`)

  const ok = await dialogs.confirm({
    title: `Move ${what} to ${to === null ? 'Personal' : toName}?`,
    message: lines.join(' '),
    confirmLabel: 'Move',
  })
  if (!ok) return null
  try {
    const result = await hosts.moveHosts(group ? [] : moving.map((h) => h.id), group?.id ?? null, to)
    const extra = result.credentialsCopied.length
      ? ` ${quote(result.credentialsCopied)} stayed too, for the hosts that use ${result.credentialsCopied.length === 1 ? 'it' : 'them'} here.`
      : ''
    ui.toast('success', `Moved ${what} to ${to === null ? 'Personal' : toName}.${extra}`)
    return result
  } catch (e) {
    ui.toast('error', `Could not move: ${errorText(e)}`)
    return null
  }
}
