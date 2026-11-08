import { defineConfig } from 'astro/config'
import react from '@astrojs/react'
import sitemap from '@astrojs/sitemap'

// GitHub Pages serves a project site from /<repo>/. A custom domain sets
// SITE_URL and BASE_PATH=/ (repository variables) for the deploy workflow.
const site = process.env.SITE_URL || 'https://adittanu.github.io'
const base = process.env.BASE_PATH || '/liteterm'

export default defineConfig({
  site,
  base,
  trailingSlash: 'ignore',
  integrations: [react(), sitemap()],
})
