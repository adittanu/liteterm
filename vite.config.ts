import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [vue()],

  build: {
    // Vite's default target (es2020) makes esbuild 0.25 lower `x ||= {}`, and
    // with minify on it drops the variable's declaration but keeps the write.
    // In xterm's requestMode that became `(void 0||(s={}))`: a ReferenceError
    // on every DECRQM query (`CSI ? Ps $ p`), which halts the parser and
    // freezes the terminal — opencode sends one at startup. `tauri dev` does
    // not minify, so only the installer broke. Every WebView this app runs
    // in (WebView2, Safari 14+, WebKitGTK) supports ES2021 natively.
    target: 'es2021',
  },

  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
