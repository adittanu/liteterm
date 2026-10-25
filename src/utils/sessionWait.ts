import { watch } from 'vue'
import { useSessionsStore } from '../stores/sessions'

/**
 * Resolve with the session id once `hostId` has a connected SSH tab, or
 * null if the attempt fails, the tab disappears, or it stalls past
 * `timeoutMs`. Generous default: a password or host-key prompt may sit
 * in between and the user needs time to answer it.
 */
export function waitForConnection(hostId: number, timeoutMs = 90_000): Promise<number | null> {
  const sessions = useSessionsStore()
  const current = () => sessions.tabs.find((t) => t.hostId === hostId && t.kind === 'ssh')
  const now = current()
  if (now?.status === 'connected') return Promise.resolve(now.id)

  return new Promise((resolve) => {
    // A tab left over from a failed attempt is about to be replaced — don't
    // read its removal as this attempt failing
    let sawTab = now !== undefined && now.status !== 'error' && now.status !== 'closed'
    const timer = setTimeout(() => finish(null), timeoutMs)
    const stop = watch(
      () => {
        const t = current()
        return t ? `${t.id}:${t.status}` : ''
      },
      () => {
        const t = current()
        if (t?.status === 'connected') return finish(t.id)
        if (t && (t.status === 'error' || t.status === 'closed')) return finish(null)
        if (!t && sawTab) return finish(null)
        if (t) sawTab = true
      },
    )
    function finish(id: number | null) {
      clearTimeout(timer)
      stop()
      resolve(id)
    }
  })
}
