<script setup lang="ts">
import {
  MonitorCog,
  KeyRound,
  ArrowLeftRight,
  ScrollText,
  ShieldCheck,
  Clock,
  Settings,
  type LucideIcon,
} from 'lucide-vue-next'
import { useUiStore, type ViewId } from '../stores/ui'
import { useDialogsStore } from '../stores/dialogs'

const ui = useUiStore()
const dialogs = useDialogsStore()

const items: { id: ViewId; label: string; icon: LucideIcon }[] = [
  { id: 'hosts', label: 'Hosts', icon: MonitorCog },
  { id: 'keychain', label: 'Keychain', icon: KeyRound },
  { id: 'forwards', label: 'Port Forwarding', icon: ArrowLeftRight },
  { id: 'snippets', label: 'Snippets', icon: ScrollText },
  { id: 'known', label: 'Known Hosts', icon: ShieldCheck },
  { id: 'logs', label: 'Logs', icon: Clock },
]

function isActive(id: ViewId): boolean {
  return ui.activeView === id
}
</script>

<template>
  <nav class="rail" :class="{ collapsed: ui.railCollapsed }" aria-label="Main navigation">
    <button
      v-for="item in items"
      :key="item.id"
      class="rail-item"
      :class="{ active: isActive(item.id) }"
      :title="ui.railCollapsed ? item.label : undefined"
      :aria-current="isActive(item.id) ? 'page' : undefined"
      @click="ui.openView(item.id)"
    >
      <component :is="item.icon" :size="17" :stroke-width="1.5" aria-hidden="true" />
      <span v-if="!ui.railCollapsed" class="rail-label">{{ item.label }}</span>
    </button>

    <span class="rail-spacer"></span>

    <button
      class="rail-item"
      :title="ui.railCollapsed ? 'Settings' : undefined"
      aria-label="Settings"
      @click="dialogs.showSettings = true"
    >
      <Settings :size="17" :stroke-width="1.5" aria-hidden="true" />
      <span v-if="!ui.railCollapsed" class="rail-label">Settings</span>
    </button>
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
  border-right: 1px solid var(--border);
  overflow-y: auto;
  transition: width 180ms var(--ease-out);
}

.rail.collapsed {
  width: var(--rail-width-collapsed);
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
  width: 40px;
  align-self: center;
}

.rail-spacer {
  flex: 1;
}

.rail-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
