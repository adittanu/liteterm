import { watch } from 'vue'
import { useSessionsStore } from '../stores/sessions'

/**
 * Resolve with the session id once `hostId` has a connected SSH tab, or
 * null if the attempt fails, the tab disappears, or it stalls past
 * `timeoutMs`. Generous default: a password or host-key prompt may sit
 * in between and the user needs time to answer it.
 *
 * An attempt that fails before any session exists (missing key file, host
 * gone) never creates a tab, so it is detected through the store's failure
 * counter. `failuresBaseline` is that counter as it was *before* the connect
 * was started — pass it when the connect was awaited first, or a failure that
 * already happened would go unnoticed.
 */
export function waitForConnection(
  hostId: number,
  timeoutMs = 90_000,
  failuresBaseline?: number,
): Promise<number | null> {
  const sessions = useSessionsStore()
  const failures = () => sessions.connectFailures[hostId] ?? 0
  const baseline = failuresBaseline ?? failures()
  const current = () => sessions.tabs.find((t) => t.hostId === hostId && t.kind === 'ssh')
  const now = current()
  if (now?.status === 'connected') return Promise.resolve(now.id)
  if (failures() !== baseline) return Promise.resolve(null)

  return new Promise((resolve) => {
    // A tab left over from a failed attempt is about to be replaced — don't
    // read its removal as this attempt failing
    let sawTab = now !== undefined && now.status !== 'error' && now.status !== 'closed'
    const timer = setTimeout(() => finish(null), timeoutMs)
    const stop = watch(
      () => {
        const t = current()
        return `${t ? `${t.id}:${t.status}` : ''}|${failures()}`
      },
      () => {
        const t = current()
        if (t?.status === 'connected') return finish(t.id)
        if (failures() !== baseline) return finish(null)
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
