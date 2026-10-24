# LiteTerm

SSH client desktop ringan — Termius-lite dengan pendekatan PandaCode:
**Tauri 2** (Rust + WebView2), SQLite WAL untuk data, kredensial di Windows
Credential Manager, UI Vue 3 + xterm.js.

## Fitur

- **Terminal SSH** — multi-tab, xterm.js, resize mengikuti jendela, keepalive 30s
- **Manajemen host** — grup, pencarian, auth password / private key
  (Ed25519, ECDSA, RSA), password tersimpan via Windows Credential Manager
- **TOFU host key** — fingerprint diverifikasi di koneksi pertama; perubahan
  key tersimpan = error keras (anti-MITM)
- **SFTP browser** — navigasi, mkdir, rename, delete, upload/download
  streaming dengan progress & cancel
- **Snippets** — simpan command, run sekali klik / Shift+click untuk insert saja
- **Port forwarding lokal** ( `-L` ) — start/stop dari UI, auto-start saat host
  tersambung
- **Command palette** — `Ctrl+K` untuk lompat ke host
- Light/dark theme (snap, tanpa smear), reduced-motion friendly

## Menjalankan

```bash
pnpm install
pnpm tauri dev
```

## Build rilis

```bash
pnpm tauri build        # output: src-tauri/target/release/bundle/nsis/
```

## Struktur

```
src/                 frontend Vue 3 (stores, components, design system CSS)
src/styles/          tokens.css (design tokens) · base.css · motion.css
src-tauri/src/store/ SQLite (migrasi + CRUD) + keyring helpers
src-tauri/src/ssh/   russh client, session pool, SFTP, port forwarding
src-tauri/src/commands/ Tauri commands (hosts, session, sftp, snippets, forwards, settings)
```

## Batasan v1

- SSH agent (Pageant / named pipe) belum didukung — password & key file dulu
- Transfer SFTP tanpa resume; tanpa sinkronisasi cloud
- Konfirmasi delete hanya untuk yang destruktif (host, file remote);
  sisanya via toast
