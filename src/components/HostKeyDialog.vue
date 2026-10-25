<script setup lang="ts">
import { computed, ref } from 'vue'
import { ShieldAlert } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useSessionsStore } from '../stores/sessions'
import type { SessionTab } from '../types'

const sessions = useSessionsStore()

const pending = computed(() =>
  sessions.tabs.find((t) => t.status === 'hostkey') as SessionTab | undefined,
)
const remember = ref(true)

async function answer(accept: boolean) {
  const tab = pending.value
  if (!tab) return
  const keep = remember.value
  remember.value = true // the next prompt starts from the safe default
  tab.hostKeyFingerprint = undefined
  tab.hostKeyAddress = undefined
  // Leave 'hostkey' before the backend resumes: its 'connected' event — or
  // a jump chain's prompt for the next hop — can land before this returns
  tab.status = accept ? 'connecting' : 'closed'
  if (!accept) tab.error = 'Host key rejected'
  await sessions.confirmHostKey(tab.id, accept, keep)
}
</script>

<template>
  <BaseModal
    v-if="pending"
    title="Verify Host Key"
    :width="460"
    :close-on-scrim="false"
  >
    <div class="hostkey">
      <div class="hostkey-icon" aria-hidden="true">
        <ShieldAlert :size="26" :stroke-width="1.5" />
      </div>
      <p class="hostkey-lead">
        First connection to <strong class="mono">{{ pending.hostKeyAddress || pending.address }}</strong>.
        Verify the server's key fingerprint before trusting it.
      </p>
      <code class="fingerprint">{{ pending.hostKeyFingerprint }}</code>
      <label class="check">
        <input v-model="remember" type="checkbox" />
        Remember this host
      </label>
      <footer class="hostkey-actions">
        <button class="btn btn-ghost" @click="answer(false)">Reject</button>
        <button class="btn btn-primary" @click="answer(true)">Trust &amp; connect</button>
      </footer>
    </div>
  </BaseModal>
</template>

<style scoped>
.hostkey {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-3);
  text-align: center;
}

.hostkey-icon {
  color: var(--warning);
}

.hostkey-lead {
  font-size: var(--text-base);
  color: var(--text-2);
  line-height: var(--leading-normal);
  max-width: 40ch;
}

.hostkey-lead strong {
  color: var(--text-1);
  font-weight: 500;
}

.fingerprint {
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  background: var(--bg-surface-2);
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  padding: 10px 14px;
  user-select: text;
  word-break: break-all;
  max-width: 100%;
  color: var(--text-1);
}

.check {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
  width: 14px;
  height: 14px;
}

.hostkey-actions {
  display: flex;
  gap: var(--space-2);
  margin-top: var(--space-2);
}
</style>
