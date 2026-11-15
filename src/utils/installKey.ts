// Put a public key on a saved host (ssh-copy-id), and optionally switch the
// host to log in with it.
import { api } from '../services/api'
import { useCredentialsStore } from '../stores/credentials'
import { useHostsStore } from '../stores/hosts'
import { useSessionsStore } from '../stores/sessions'
import { waitForConnection } from './sessionWait'
import type { HostInput } from '../types'

export interface KeySource {
  publicKey: string
  keyPath: string
  /** Typed when the key was made; saved for the host when it switches */
  passphrase?: string | null
  /** The credential that holds this key, if any */
  credentialId?: number | null
}

/**
 * Add the key to the host's authorized_keys over a live session, connecting
 * first (the current login, password or not) when there is none. A session
 * opened only for this is closed again. 'present' = the key was already
 * there; null = the connection never came up.
 */
export async function installKeyOn(hostId: number, publicKey: string): Promise<'added' | 'present' | null> {
  const sessions = useSessionsStore()
  const live = sessions.tabs.find((t) => t.hostId === hostId && t.kind === 'ssh' && t.status === 'connected')
  let sessionId = live?.id ?? null
  let opened = false
  if (sessionId === null) {
    const baseline = sessions.connectFailures[hostId] ?? 0
    await sessions.connect(hostId, undefined, { silent: true })
    sessionId = await waitForConnection(hostId, 90_000, baseline)
    if (sessionId === null) return null
    opened = true
  }
  try {
    return (await api.installPublicKey(sessionId, publicKey)) ? 'added' : 'present'
  } finally {
    if (opened) void sessions.disconnect(sessionId)
  }
}

/** From now on the host logs in with this key, as the same user. */
export async function switchHostToKey(hostId: number, key: KeySource) {
  const hosts = useHostsStore()
  const credentials = useCredentialsStore()
  const host = hosts.hostById(hostId)
  if (!host) return
  // The key's credential fits only when it logs in as the host's user, from
  // the host's own vault
  const cred = credentials.byId(key.credentialId ?? null)
  const viaCredential = cred && cred.username === host.username && cred.team_id === host.team_id ? cred : undefined
  const input: HostInput = {
    id: host.id,
    group_id: host.group_id,
    label: host.label,
    address: host.address,
    port: host.port,
    username: host.username,
    auth_method: 'key',
    key_path: viaCredential ? null : key.keyPath,
    key_passphrase: viaCredential ? null : key.passphrase || null,
    // Keep a saved password: it is the way back in if the key is refused
    save_password: host.password_saved,
    credential_id: viaCredential?.id ?? null,
    jump_host_id: host.jump_host_id,
    startup_command: host.startup_command,
  }
  await hosts.saveHost(input)
}
