// What a host is reached over. SSH hosts have everything; Telnet and serial
// hosts are a plain terminal stream: no SFTP, tunnels, keys or jump hosts.
import type { Host, HostProtocol } from '../types'

export const PROTOCOLS: { value: HostProtocol; label: string }[] = [
  { value: 'ssh', label: 'SSH' },
  { value: 'telnet', label: 'Telnet' },
  { value: 'serial', label: 'Serial' },
]

export const DEFAULT_PORT: Record<Exclude<HostProtocol, 'serial'>, number> = { ssh: 22, telnet: 23 }

// The speeds consoles use; 9600 and 115200 cover most switches and boards
export const BAUD_RATES = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600]
export const DEFAULT_BAUD = 115200

/** A host from an older build has no protocol field: it is SSH */
export function protocolOf(h: Pick<Host, 'protocol'>): HostProtocol {
  return h.protocol ?? 'ssh'
}

export function isSsh(h: Pick<Host, 'protocol'>): boolean {
  return protocolOf(h) === 'ssh'
}

/** The line under a host's name, and the tab's address */
export function hostTarget(h: Pick<Host, 'protocol' | 'username' | 'address' | 'port' | 'baud_rate'>): string {
  switch (protocolOf(h)) {
    case 'telnet':
      return h.port === DEFAULT_PORT.telnet ? h.address : `${h.address}:${h.port}`
    case 'serial':
      return `${h.address} · ${h.baud_rate ?? DEFAULT_BAUD} baud`
    default:
      return `${h.username}@${h.address}`
  }
}
