# LiteTerm landing page

Static [Astro](https://astro.build) site for LiteTerm. It is its own project with its own lockfile, so it does not touch the app's build.

```bash
cd site
pnpm install
pnpm dev        # http://localhost:4321/liteterm/
pnpm build      # dist/
pnpm preview
```

## How it fits together

- `src/pages/index.astro` assembles the sections in `src/components/`.
- `src/data/release.ts` reads the latest GitHub release **at build time** for the version, the installer sizes and the download links. Without network (or past GitHub's rate limit) the page still builds and the buttons point at the releases page. Set `GITHUB_TOKEN` for a local build to avoid the limit.
- `src/components/DownloadButton.astro` is a plain link to the platform cards; a few lines of script point it straight at the visitor's installer.
- `src/components/hero/HeroDemo.tsx` is the animated product illustration (a React island). It is built with the Stageshow kit in `src/stageshow/kit/` (MIT, see `src/stageshow/LICENSE`; the kit's third-party brand artwork was left out). Its look maps the app's own tokens (`src/styles/tokens.css` in the app) onto the kit's. It has a Pause button (WCAG 2.2.2) and shows a still under reduced motion.
- Colors are the app's dark theme tokens in `src/styles/global.css`; text pairs were measured against WCAG AA.
- `public/og.png` is the 1200x630 share image.

## Deploy

`.github/workflows/site.yml` builds and publishes to GitHub Pages on pushes to `main` that touch `site/`, on every published release (so the download links follow it), and by hand.

One-time: **Settings > Pages > Build and deployment > Source: GitHub Actions**. The site is live at `https://liteterm.kreat.id/`: a CNAME record `liteterm.kreat.id → adittanu.github.io`, that domain under Settings > Pages, and the repository variables `SITE_URL=https://liteterm.kreat.id` and `BASE_PATH=/`. Without those variables the build targets `https://adittanu.github.io/liteterm/`.

## Copy rules

Every claim on the page is checked against the app's code or README: features, limits, where secrets live, how updates work. Keep it that way when the app changes, especially the **Limits** and **FAQ** sections.
