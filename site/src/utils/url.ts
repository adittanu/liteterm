/** A path under the site's base ("/liteterm/" on GitHub Pages, "/" on a custom domain). */
export function withBase(path = ''): string {
  const base = import.meta.env.BASE_URL.replace(/\/$/, '')
  return `${base}/${path.replace(/^\//, '')}`
}
