import type { ForwardKind, PortForward } from '../types'

export const FORWARD_KINDS: { kind: ForwardKind; label: string; flag: string; hint: string }[] = [
  { kind: 'local', label: 'Local', flag: '-L', hint: 'Open a service on the server from this computer' },
  { kind: 'remote', label: 'Remote', flag: '-R', hint: 'Let the server reach a service on this computer' },
  { kind: 'dynamic', label: 'SOCKS', flag: '-D', hint: 'Send apps through the server with a SOCKS proxy' },
]

/** A SOCKS proxy left without a port listens where most apps expect one */
export const DEFAULT_SOCKS_PORT = 1080

/**
 * The address worth copying, and where it lives: what an app on this
 * computer connects to (local, SOCKS), or what a program on the server
 * connects to (remote).
 */
export function forwardAddress(f: Pick<PortForward, 'kind' | 'local_port' | 'remote_port'>): {
  address: string
  where: 'here' | 'server'
} {
  if (f.kind === 'remote') return { address: `localhost:${f.remote_port}`, where: 'server' }
  return { address: `localhost:${f.local_port}`, where: 'here' }
}

/** One line for the toast after a forward starts */
export function startedText(f: PortForward, hostLabel: string): string {
  if (f.kind === 'remote') return `${hostLabel} now listens on localhost:${f.remote_port}`
  if (f.kind === 'dynamic') return `SOCKS proxy on localhost:${f.local_port}`
  return `Listening on localhost:${f.local_port}`
}
