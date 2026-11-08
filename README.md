# LiteTerm

SSH client desktop ringan — Termius-lite dengan pendekatan PandaCode:
**Tauri 2** (Rust + WebView bawaan OS: WebView2 / WKWebView / WebKitGTK), SQLite WAL
untuk data, kredensial di keyring OS (Windows Credential Manager, macOS Keychain,
Secret Service di Linux), UI Vue 3 + xterm.js.

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

## Platform

Setiap rilis di GitHub Releases berisi:

- **Windows** — installer NSIS (`*-setup.exe`). Update langsung dari dalam aplikasi.
- **macOS 11+** — `.dmg` universal (Apple Silicon dan Intel). Build ini belum
  di-sign/notarize, jadi saat pertama dibuka macOS menolaknya: buka
  **System Settings → Privacy & Security → Open Anyway**, atau jalankan
  `xattr -dr com.apple.quarantine /Applications/LiteTerm.app`. (Kalau secret Apple
  Developer diisi di repo — daftarnya ada di komentar `release.yml` — build macOS
  otomatis di-sign dan di-notarize.) Tombol update membuka halaman release (belum
  update otomatis).
- **Linux** — `.AppImage`, `.deb`, `.rpm` (dibangun di Ubuntu 22.04, butuh
  WebKitGTK 4.1). Password tersimpan butuh layanan Secret Service yang berjalan dan
  terbuka (GNOME Keyring / KWallet); tanpa itu LiteTerm memberi tahu bahwa password
  tidak tersimpan. Tombol update membuka halaman release.

Di macOS shortcut memakai `Cmd` (`Cmd+K` command palette, `Cmd+T` terminal lokal,
`Cmd+W` tutup sesi, ...); di Linux salin/tempel di terminal dengan
`Ctrl+Shift+C` / `Ctrl+Shift+V`.

## Menjalankan

```bash
pnpm install
pnpm tauri dev
```

## Build rilis

```bash
pnpm tauri build        # Windows: src-tauri/target/release/bundle/nsis/
```

Build macOS dan Linux dibuat oleh GitHub Actions (`.github/workflows/release.yml`):
tag `v*` membuat release, atau jalankan workflow secara manual untuk mencoba
sebuah branch (installernya keluar sebagai artifact, tidak ada yang dirilis).
Di Linux butuh `libwebkit2gtk-4.1-dev libdbus-1-dev` dan kawan-kawannya
(daftar lengkap ada di workflow).

## Diagnosa masalah di installer

Build installer tidak punya konsol, jadi error tidak terlihat. LiteTerm menulis
panic dan kegagalan yang tadinya senyap (mis. output terminal yang gagal terkirim)
ke `%APPDATA%\com.liteterm.app\liteterm.log` (macOS:
`~/Library/Application Support/com.liteterm.app/liteterm.log`, Linux:
`~/.local/share/com.liteterm.app/liteterm.log`).

Untuk membuka DevTools (F12) di build rilis, bangun installer diagnostik:

```bash
pnpm tauri build --features diag
```

Untuk membandingkan dev dengan installer secara adil, jalankan hasil build rilis
(`pnpm tauri build --no-bundle`, lalu `src-tauri\target\release\liteterm.exe`)
dengan klik dua kali dari Explorer, bukan dari terminal: lingkungannya sama dengan
installer (variabel `TERM_PROGRAM`, `WT_SESSION`, dll. dari terminal tidak ikut).

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
