<script setup lang="ts">
import { computed } from 'vue'
import { Plus, Command, MousePointerClick } from 'lucide-vue-next'
import { useDialogsStore } from '../stores/dialogs'
import { useHostsStore } from '../stores/hosts'

const dialogs = useDialogsStore()
const hosts = useHostsStore()

const hasHosts = computed(() => hosts.hosts.length > 0)
</script>

<template>
  <div class="welcome empty-state">
    <div class="welcome-mark" aria-hidden="true">
      <svg width="56" height="56" viewBox="0 0 56 56" fill="none">
        <rect x="4" y="8" width="48" height="40" rx="10" class="mark-frame" stroke-width="2" />
        <path d="M16 24l8 6-8 6" class="mark-caret" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" />
        <path d="M28 37h12" class="mark-caret" stroke-width="2.5" stroke-linecap="round" />
      </svg>
    </div>
    <h2 class="welcome-title">{{ hasHosts ? 'Ready to connect' : 'Welcome to LiteTerm' }}</h2>
    <p v-if="hasHosts" class="welcome-sub">
      Pick a host from the sidebar to open a session — or open a local terminal
      with the terminal button up top.
    </p>
    <p v-else class="welcome-sub">A fast, lightweight SSH client. Add your first host to get started.</p>
    <button v-if="!hasHosts" class="btn btn-primary welcome-cta" @click="dialogs.openHostDialog(null, null)">
      <Plus :size="15" :stroke-width="2" aria-hidden="true" />
      Add your first host
    </button>
    <p v-else class="welcome-hint">
      <MousePointerClick :size="12" :stroke-width="1.5" aria-hidden="true" />
      or press <kbd>Ctrl</kbd>+<kbd>K</kbd> to search
    </p>
    <p class="welcome-hint">
      <Command :size="12" :stroke-width="1.5" aria-hidden="true" />
      Press <kbd>Ctrl</kbd>+<kbd>K</kbd> anytime to jump to a host
    </p>
  </div>
</template>

<style scoped>
.welcome {
  flex: 1;
  gap: var(--space-4);
  padding: var(--space-6);
}

.welcome-mark {
  color: var(--accent);
  opacity: 0.9;
}

.mark-frame {
  stroke: currentColor;
  fill: none;
  opacity: 0.55;
}

.mark-caret {
  stroke: currentColor;
  fill: none;
}

.welcome-title {
  font-size: var(--text-xl);
  font-weight: 600;
  color: var(--text-1);
}

.welcome-sub {
  max-width: 34ch;
  color: var(--text-3);
  font-size: var(--text-md);
  line-height: var(--leading-normal);
}

.welcome-cta {
  margin-top: var(--space-2);
}

.welcome-hint {
  display: flex;
  align-items: center;
  gap: 5px;
  margin-top: var(--space-4);
  font-size: var(--text-sm);
  color: var(--text-4);
}

kbd {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  background: var(--bg-surface-2);
  border: 1px solid var(--border);
  border-bottom-width: 2px;
  border-radius: 4px;
  padding: 1px 5px;
  color: var(--text-2);
}
</style>
