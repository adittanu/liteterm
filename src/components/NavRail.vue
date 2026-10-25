<script setup lang="ts">
import {
  MonitorCog,
  KeyRound,
  ArrowLeftRight,
  ScrollText,
  StickyNote,
  ShieldCheck,
  Clock,
  Settings,
  type LucideIcon,
} from 'lucide-vue-next'
import { useUiStore, type ViewId } from '../stores/ui'
import { useDialogsStore } from '../stores/dialogs'
import { comboFor, viewShortcutId } from '../services/shortcuts'
import UpdateButton from './UpdateButton.vue'

const ui = useUiStore()
const dialogs = useDialogsStore()

const items: { id: ViewId; label: string; icon: LucideIcon }[] = [
  { id: 'hosts', label: 'Hosts', icon: MonitorCog },
  { id: 'keychain', label: 'Keychain', icon: KeyRound },
  { id: 'forwards', label: 'Port Forwarding', icon: ArrowLeftRight },
  { id: 'snippets', label: 'Snippets', icon: ScrollText },
  { id: 'notes', label: 'Notes', icon: StickyNote },
  { id: 'known', label: 'Known Hosts', icon: ShieldCheck },
  { id: 'logs', label: 'Logs', icon: Clock },
]

function isActive(id: ViewId): boolean {
  return ui.activeView === id
}

// Collapsed rail hides labels — the tooltip doubles as shortcut discovery
function railTitle(id: ViewId, label: string): string {
  return ui.railCollapsed ? `${label} (${comboFor(viewShortcutId(id), ui.settings)})` : label
}
</script>

<template>
  <nav class="rail" :class="{ collapsed: ui.railCollapsed }" aria-label="Main navigation">
    <button
      v-for="item in items"
      :key="item.id"
      class="rail-item"
      :class="{ active: isActive(item.id) }"
      :title="railTitle(item.id, item.label)"
      :aria-current="isActive(item.id) ? 'page' : undefined"
      @click="ui.openView(item.id)"
    >
      <component :is="item.icon" :size="17" :stroke-width="1.5" aria-hidden="true" />
      <span v-if="!ui.railCollapsed" class="rail-label">{{ item.label }}</span>
    </button>

    <span class="rail-spacer"></span>

    <img
      v-if="!ui.railCollapsed"
      class="rail-art"
      src="../assets/rail-art-crop.png"
      alt=""
      aria-hidden="true"
      draggable="false"
    />

    <div class="rail-bottom">
      <UpdateButton />

      <button
        class="rail-item"
        :title="ui.railCollapsed ? 'Settings' : undefined"
        aria-label="Settings"
        @click="dialogs.showSettings = true"
      >
        <Settings :size="17" :stroke-width="1.5" aria-hidden="true" />
        <span v-if="!ui.railCollapsed" class="rail-label">Settings</span>
      </button>
    </div>
  </nav>
</template>

<style scoped>
.rail {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: var(--rail-width);
  flex-shrink: 0;
  padding: var(--space-3) var(--space-2);
  overflow-y: auto;
  overflow-x: hidden;
  /* Defined sidebar surface — without a visible boundary the collapsed
     icon column reads as floating in the content's own whitespace */
  background: var(--material-bg);
  border-right: var(--material-edge);
  /* A nav rail never shows a scrollbar (wheel still scrolls) — a
     classic scrollbar would eat asymmetric space and shift icons */
  scrollbar-width: none;
  transition: width 180ms var(--ease-out), padding 180ms var(--ease-out);
}

.rail::-webkit-scrollbar {
  display: none;
}

@media (prefers-reduced-transparency: reduce) {
  .rail {
    background: var(--bg-surface);
  }
}

.rail.collapsed {
  width: var(--rail-width-collapsed);
  padding-inline: 6px;
}

.rail-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 36px;
  padding: 0 10px;
  border-radius: var(--radius-md);
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-2);
  flex-shrink: 0;
  transition: background-color var(--dur-hover) ease, color var(--dur-hover) ease;
}

.rail-item:hover {
  background: var(--bg-hover);
  color: var(--text-1);
}

.rail-item.active {
  background: var(--accent-bg);
  color: var(--text-1);
}

.rail-item.active svg {
  color: var(--accent);
}

.rail.collapsed .rail-item {
  justify-content: center;
  padding: 0;
  width: 36px;
  align-self: center;
}

.rail-spacer {
  flex: 1;
}

/* Bottom pair: Settings leads on the left, update control beside it on
   the right. row-reverse keeps DOM order update-then-settings, which is
   the stacking order the collapsed column needs — Settings stays the
   bottom-most item either way. */
.rail-bottom {
  display: flex;
  flex-direction: row-reverse;
  gap: 2px;
}

.rail-bottom > .rail-item {
  flex: 1;
  min-width: 0;
}

.rail.collapsed .rail-bottom {
  flex-direction: column;
}

.rail.collapsed .rail-bottom > .rail-item {
  flex: none;
}

.rail-art {
  display: block;
  flex-shrink: 0;
  /* Span the rail's full width (bleeding into its side padding) so the
     clouds hug the sidebar's left edge, then scale up a touch — the
     source's right side is empty, so the overflow (clipped by the
     rail) loses nothing */
  width: calc((100% + 2 * var(--space-2)) * 1.15);
  margin: var(--space-2) calc(var(--space-2) * -1) 0;
  pointer-events: none;
  user-select: none;
  opacity: 0.9;
  /* Dissolve both cut edges into the rail surface — the source art is
     trimmed by its original frame at top and bottom */
  -webkit-mask-image: linear-gradient(to bottom, transparent, black 26%, black 86%, transparent);
  mask-image: linear-gradient(to bottom, transparent, black 26%, black 86%, transparent);
}

.rail-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
