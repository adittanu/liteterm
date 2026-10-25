# LiteTerm

SSH client desktop ringan — Termius-lite dengan pendekatan PandaCode:
**Tauri 2** (Rust + WebView2), SQLite WAL untuk data, kredensial di Windows
Credential Manager, UI Vue 3 + xterm.js.

## Fitur

- **Terminal SSH** — multi-tab, xterm.js, resize mengikuti jendela, keepalive 30s
- **Auto-reconnect** — sesi yang putus (ganti Wi-Fi, laptop sleep) disambung ulang
  di tab yang sama dengan backoff (±3½ menit), scrollback tetap; bisa dimatikan
  di Settings
- **Find di terminal** — `Ctrl+Shift+F`, match case / regex, Enter / Shift+Enter
- **Manajemen host** — grup, pencarian, auth password / private key
  (Ed25519, ECDSA, RSA) / **SSH agent** (OpenSSH Authentication Agent, 1Password,
  Bitwarden, KeePassXC), password tersimpan via Windows Credential Manager
- **Jump host (ProxyJump)** — tunnel lewat host bastion tersimpan, bisa berantai;
  `ProxyJump` di `~/.ssh/config` ikut ter-import
- **TOFU host key** — fingerprint diverifikasi di koneksi pertama (per hop); perubahan
  key tersimpan = error keras (anti-MITM)
- **SFTP browser** — navigasi, mkdir, rename, delete, upload/download
  streaming dengan progress & cancel
- **Snippets** — simpan command, run sekali klik / Shift+click untuk insert saja
- **Notes** — ala Apple Notes, #tag, bisa ditautkan ke host (tampil di panel host);
  code block punya tombol **Run** ke terminal host itu
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

## Batasan

- SSH agent: Pageant belum didukung (pakai agent OpenSSH / 1Password / Bitwarden /
  KeePassXC), agent forwarding belum ada
- Jump host butuh login yang tersimpan (password + Remember, key, atau agent)
- Transfer SFTP tanpa resume; tanpa sinkronisasi cloud
- Konfirmasi delete hanya untuk yang destruktif (host, file remote);
  sisanya via toast
