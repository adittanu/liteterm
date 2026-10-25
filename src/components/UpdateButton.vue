<script setup lang="ts">
import { computed } from 'vue'
import {
  Check,
  Download,
  LoaderCircle,
  RefreshCw,
  RotateCcw,
} from 'lucide-vue-next'
import { useUpdaterStore } from '../stores/updater'
import { useUiStore } from '../stores/ui'

const updater = useUpdaterStore()
const ui = useUiStore()

function fmtMB(bytes: number): string {
  return (bytes / 1048576).toFixed(1)
}

const tooltip = computed(() => {
  const current = updater.info?.currentVersion
  const latest = updater.info?.latestVersion
  switch (updater.status) {
    case 'checking':
      return 'Checking for updates…'
    case 'available':
      return `Download LiteTerm v${latest} (installed v${current})`
    case 'downloading':
      return `Downloading v${latest} — ${fmtMB(updater.downloaded)} of ${fmtMB(updater.total)} MB`
    case 'ready':
      return `Restart LiteTerm to finish updating to v${latest}`
    case 'uptodate':
      return `LiteTerm is up to date${current ? ` (v${current})` : ''}`
    default:
      return current ? `Check for updates — LiteTerm v${current}` : 'Check for updates'
  }
})

const busy = computed(
  () => updater.status === 'checking' || updater.status === 'downloading',
)
</script>

<template>
  <button
    class="upd-btn"
    :class="[updater.status, { collapsed: ui.railCollapsed }]"
    :title="tooltip"
    :aria-label="tooltip"
    :aria-busy="updater.status === 'downloading'"
    :disabled="busy"
    @click="updater.run()"
  >
    <span class="upd-icon" aria-hidden="true">
      <RefreshCw :size="17" :stroke-width="1.5" :class="{ on: updater.status === 'idle' }" />
      <LoaderCircle
        :size="17"
        :stroke-width="1.5"
        class="spin"
        :class="{ on: busy }"
      />
      <Download :size="17" :stroke-width="1.5" :class="{ on: updater.status === 'available' }" />
      <RotateCcw :size="17" :stroke-width="1.5" :class="{ on: updater.status === 'ready' }" />
      <Check :size="17" :stroke-width="1.5" :class="{ on: updater.status === 'uptodate' }" />
      <span v-if="updater.status === 'available'" class="upd-dot"></span>
    </span>

    <span
      v-if="updater.status === 'downloading'"
      class="upd-progress"
      :class="{ indet: updater.percent === null }"
      :style="updater.percent !== null ? { width: `${updater.percent}%` } : undefined"
    ></span>
  </button>
</template>

<style scoped>
/* Icon-only companion to the Settings button: a 36px square that sits
 * beside it while the rail is expanded and stacks above it when collapsed
 * (the parent .rail-bottom in NavRail owns that placement). */
.upd-btn {
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  border-radius: var(--radius-md);
  color: var(--text-2);
  overflow: hidden;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.upd-btn.collapsed {
  align-self: center;
}

.upd-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  color: var(--text-1);
}

.upd-btn:disabled {
  cursor: default;
}

/* State tints — every state pairs its motion with a static cue */
.upd-btn.available .upd-icon {
  color: var(--accent);
}

.upd-btn.ready {
  background: var(--accent-bg);
  color: var(--accent);
}

.upd-btn.ready:hover {
  background: var(--accent-bg-hover);
  color: var(--accent);
}

.upd-btn.uptodate .upd-icon {
  color: var(--success);
}

/* Icon morph: all icons stay in the DOM; the active one resolves from
   scale(0.25)/blur(4px), per the icon-transition recipe */
.upd-icon {
  position: relative;
  width: 17px;
  height: 17px;
  flex-shrink: 0;
}

.upd-icon > svg {
  position: absolute;
  inset: 0;
  margin: auto;
  opacity: 0;
  transform: scale(0.25);
  filter: blur(4px);
  transition:
    opacity 200ms cubic-bezier(0.2, 0, 0, 1),
    transform 200ms cubic-bezier(0.2, 0, 0, 1),
    filter 200ms cubic-bezier(0.2, 0, 0, 1);
}

.upd-icon > svg.on {
  opacity: 1;
  transform: scale(1);
  filter: blur(0);
}

.spin {
  animation: spin 0.7s linear infinite;
}

/* Attention dot — same shape language as the tab status dots */
.upd-dot {
  position: absolute;
  top: -3px;
  right: -4px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 1.5px var(--material-bg);
}

/* Download progress: a hairline strip along the button's bottom edge */
.upd-progress {
  position: absolute;
  left: 0;
  bottom: 0;
  height: 2px;
  background: var(--accent);
  transition: width 120ms linear;
}

.upd-progress.indet {
  width: 30%;
  animation: upd-slide 1.1s ease-in-out infinite;
}

@keyframes upd-slide {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(433%);
  }
}
</style>
