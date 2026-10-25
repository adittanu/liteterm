import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { api } from '../services/api'
import type { UpdateInfo } from '../types'
import { useUiStore } from './ui'

export type UpdateStatus =
  | 'idle' // nothing known — "Check for updates"
  | 'checking'
  | 'available' // a newer release is one click away
  | 'downloading'
  | 'ready' // installer on disk — waiting for the user to restart
  | 'uptodate' // transient confirmation after a check

export const useUpdaterStore = defineStore('updater', () => {
  const ui = useUiStore()

  const status = ref<UpdateStatus>('idle')
  const info = ref<UpdateInfo | null>(null)
  const downloaded = ref(0)
  const total = ref(0)

  let installerPath: string | null = null
  let restarting = false
  let revertTimer: ReturnType<typeof setTimeout> | null = null
  let progressUnlisten: UnlistenFn | null = null

  const percent = computed(() => {
    if (status.value !== 'downloading') return null
    if (total.value <= 0) return null
    return Math.min(100, Math.floor((downloaded.value / total.value) * 100))
  })

  function clearRevert() {
    if (revertTimer) {
      clearTimeout(revertTimer)
      revertTimer = null
    }
  }

  function revertToIdle(delay = 2600) {
    clearRevert()
    revertTimer = setTimeout(() => {
      status.value = 'idle'
    }, delay)
  }

  /** Wire the progress event and run one quiet check. Called once on mount. */
  async function init() {
    if (progressUnlisten) return
    progressUnlisten = await listen<{ downloaded: number; total: number }>(
      'update:progress',
      (e) => {
        downloaded.value = e.payload.downloaded
        total.value = e.payload.total
      },
    )
    void check(true)
  }

  async function check(silent = false): Promise<UpdateInfo | null> {
    if (status.value === 'checking' || status.value === 'downloading') return null
    clearRevert()
    status.value = 'checking'
    try {
      const result = await api.checkUpdate()
      info.value = result
      if (result.available && result.asset) {
        status.value = 'available'
        if (silent) {
          ui.toast('info', `LiteTerm ${result.latestVersion} is available`, {
            actionLabel: 'Download',
            action: () => void download(),
          })
        }
      } else {
        status.value = 'uptodate'
        revertToIdle()
      }
      return result
    } catch (e) {
      status.value = 'idle'
      if (!silent) ui.toast('error', String(e))
      return null
    }
  }

  async function download() {
    const asset = info.value?.asset
    if (!asset || status.value === 'downloading' || status.value === 'ready') return
    clearRevert()
    status.value = 'downloading'
    downloaded.value = 0
    total.value = asset.size
    try {
      // installUpdate needs the path; keep it module-local, not reactive state
      installerPath = await api.downloadUpdate(asset)
      status.value = 'ready'
    } catch (e) {
      status.value = 'idle'
      ui.toast('error', String(e))
    }
  }

  async function restart() {
    if (!installerPath || restarting) return
    restarting = true
    try {
      // The backend exits the app as part of installing — this call not
      // resolving is the normal path.
      await api.installUpdate(installerPath)
    } catch (e) {
      restarting = false
      ui.toast('error', String(e))
    }
  }

  /** One handler for the rail button: act on whatever state it shows. */
  async function run() {
    switch (status.value) {
      case 'checking':
      case 'downloading':
        break
      case 'ready':
        // The installer is on disk — this click runs it and relaunches
        await restart()
        break
      case 'available':
        await download()
        break
      case 'idle':
      case 'uptodate': {
        // A manual check that finds something flows straight into the
        // download — one click, no second confirmation.
        const result = await check()
        if (result?.available && result.asset) await download()
        break
      }
    }
  }

  return {
    status,
    info,
    downloaded,
    total,
    percent,
    init,
    check,
    download,
    restart,
    run,
  }
})
