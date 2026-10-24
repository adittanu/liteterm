<script setup lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window'
import { TerminalSquare } from 'lucide-vue-next'

const appWindow = getCurrentWindow()
</script>

<template>
  <header class="titlebar material" data-tauri-drag-region>
    <div class="titlebar-brand" data-tauri-drag-region>
      <TerminalSquare :size="15" :stroke-width="1.5" aria-hidden="true" />
      <span class="titlebar-name">LiteTerm</span>
    </div>

    <div class="titlebar-spacer" data-tauri-drag-region></div>

    <div class="window-controls">
      <button
        class="win-btn"
        aria-label="Minimize"
        @click="appWindow.minimize()"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 5h10" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button
        class="win-btn"
        aria-label="Maximize"
        @click="appWindow.toggleMaximize()"
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <rect x="0.5" y="0.5" width="9" height="9" rx="1" fill="none" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
      <button class="win-btn win-close" aria-label="Close" @click="appWindow.close()">
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
          <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1" />
        </svg>
      </button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  display: flex;
  align-items: center;
  height: var(--titlebar-height);
  flex-shrink: 0;
  border-bottom: 1px solid var(--border);
  /* top edge catches light — the material reads as glass */
  box-shadow: inset 0 1px 0 oklch(1 0 0 / 0.04);
}

.titlebar-brand {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 14px;
  color: var(--text-2);
  height: 100%;
}

.titlebar-name {
  font-size: var(--text-sm);
  font-weight: 600;
  letter-spacing: var(--tracking-tight);
  color: var(--text-2);
}

.titlebar-spacer {
  flex: 1;
  height: 100%;
}

.window-controls {
  display: flex;
  height: 100%;
}

.win-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: 100%;
  color: var(--text-3);
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.win-btn:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.win-close:hover {
  background: oklch(0.62 0.22 25);
  color: white;
}
</style>
