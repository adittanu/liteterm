// What the download buttons point at. Read from the latest GitHub release at
// build time, so a rebuild after a release (the deploy workflow does it) keeps
// every link and size current. If GitHub cannot be reached the page still
// builds, and the buttons fall back to the releases page.

export const REPO = 'adittanu/liteterm'
export const REPO_URL = `https://github.com/${REPO}`
export const RELEASES_URL = `${REPO_URL}/releases/latest`

export interface Asset {
  name: string
  url: string
  /** Size in MB, one decimal */
  size: string
}

export interface Release {
  version: string
  url: string
  windows: Asset | null
  macos: Asset | null
  appimage: Asset | null
  deb: Asset | null
  rpm: Asset | null
  /** False when the numbers are the built-in fallback, not a live release */
  live: boolean
}

interface GhAsset {
  name: string
  size: number
  browser_download_url: string
}

const FALLBACK: Release = {
  version: '1.9.0',
  url: RELEASES_URL,
  windows: null,
  macos: null,
  appimage: null,
  deb: null,
  rpm: null,
  live: false,
}

function asset(assets: GhAsset[], match: (name: string) => boolean): Asset | null {
  const found = assets.find((a) => match(a.name.toLowerCase()))
  if (!found) return null
  return {
    name: found.name,
    url: found.browser_download_url,
    size: (found.size / 1048576).toFixed(1),
  }
}

async function load(): Promise<Release> {
  try {
    const headers: Record<string, string> = {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'liteterm-site-build',
    }
    // CI has a token; a local build works without one
    if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      headers,
      signal: AbortSignal.timeout(10_000),
    })
    if (!res.ok) throw new Error(`GitHub answered ${res.status}`)
    const data = (await res.json()) as { tag_name: string; html_url: string; assets: GhAsset[] }
    const a = data.assets ?? []
    return {
      version: data.tag_name.replace(/^v/i, ''),
      url: data.html_url,
      windows: asset(a, (n) => n.endsWith('x64-setup.exe') || n.endsWith('-setup.exe')),
      macos: asset(a, (n) => n.endsWith('.dmg')),
      appimage: asset(a, (n) => n.endsWith('.appimage')),
      deb: asset(a, (n) => n.endsWith('.deb')),
      rpm: asset(a, (n) => n.endsWith('.rpm')),
      live: true,
    }
  } catch (e) {
    console.warn(`[release] using the built-in fallback: ${e instanceof Error ? e.message : e}`)
    return FALLBACK
  }
}

export const release = await load()

/** Where a download button for an OS goes: the asset, or the releases page. */
export const links = {
  windows: release.windows?.url ?? release.url,
  macos: release.macos?.url ?? release.url,
  linux: release.appimage?.url ?? release.url,
}
