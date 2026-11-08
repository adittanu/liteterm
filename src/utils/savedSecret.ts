import { useUiStore } from '../stores/ui'
import { isMac, isWindows, secretStore } from './platform'

/**
 * The backend keeps passwords in the OS keyring and only logs a failure (a
 * Linux desktop without a running Secret Service, a locked Keychain). The saved
 * record's `has_password` is read back from the keyring, so a password that
 * was asked to be kept but is not there did not stick: say so, instead of
 * leaving the user to find out at the next connect.
 */
export function warnIfPasswordNotKept(
  asked: { password?: string | null; save_password?: boolean },
  kept: boolean,
) {
  if (!asked.save_password || !asked.password || kept) return
  const hint =
    isWindows || isMac ? '' : ' Is a keyring service (GNOME Keyring or KWallet) running and unlocked?'
  useUiStore().toast(
    'error',
    `Couldn’t save the password to the ${secretStore}.${hint} It will be asked for on connect.`,
  )
}
