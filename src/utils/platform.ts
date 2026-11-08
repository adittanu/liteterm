// Which OS this webview runs on. The user agent is enough to tell: WebView2
// says "Windows NT", WKWebView "Macintosh", WebKitGTK "X11; Linux".
export type OsName = 'windows' | 'macos' | 'linux'

export function osFromUserAgent(ua: string): OsName {
  if (/Windows/i.test(ua)) return 'windows'
  if (/Mac(intosh| OS X)/i.test(ua)) return 'macos'
  return 'linux'
}

export const os: OsName = osFromUserAgent(typeof navigator === 'undefined' ? '' : navigator.userAgent)

/** Local paths use `\` and drive letters here, `/` everywhere else. */
export const isWindows = os === 'windows'
export const isMac = os === 'macos'

// ---- the OS's own words for what the UI names ----

/** What this computer is called. */
export const thisComputer = isWindows ? 'This PC' : isMac ? 'This Mac' : 'This computer'
export const thisComputerLower = thisComputer.toLowerCase()

/** Where "Remember password" keeps secrets. */
export const secretStore = isWindows ? 'Windows Credential Manager' : isMac ? 'macOS Keychain' : 'system keyring'

/** The agent that serves SSH keys, as a noun phrase ("… , 1Password or KeePassXC"). */
export const sshAgentName = isWindows ? 'the Windows “OpenSSH Authentication Agent” service' : 'ssh-agent'

/** The shell a new local terminal opens. */
export const localShellName = isWindows ? 'PowerShell' : 'Shell'

/** The primary modifier as a tooltip prefix: "Ctrl+" or "⌘" ("Bold (Ctrl+B)" / "Bold (⌘B)"). */
export const modKey = isMac ? '⌘' : 'Ctrl+'
export const modShiftKey = isMac ? '⌘⇧' : 'Ctrl+Shift+'

/** Does this key event carry the primary modifier (Cmd on a Mac, Ctrl elsewhere)? */
export function hasMod(e: { ctrlKey: boolean; metaKey: boolean }): boolean {
  return isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey
}

/** Copy / paste in a terminal pane, as the tooltip and context menu show them. */
export const terminalClipboardKeys = isMac
  ? { copy: '⌘C', paste: '⌘V' }
  : isWindows
    ? { copy: 'Ctrl+C', paste: 'Ctrl+V' }
    : { copy: 'Ctrl+Shift+C', paste: 'Ctrl+Shift+V' }
