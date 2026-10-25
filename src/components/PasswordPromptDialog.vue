<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { KeyRound } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useSessionsStore } from '../stores/sessions'
import { useHostsStore } from '../stores/hosts'
import { useCredentialsStore } from '../stores/credentials'
import { useUiStore } from '../stores/ui'

const dialogs = useDialogsStore()
const sessions = useSessionsStore()
const hosts = useHostsStore()
const credentials = useCredentialsStore()
const ui = useUiStore()

const value = ref('')
const remember = ref(true)
const submitting = ref(false)

const prompt = computed(() => dialogs.passwordPrompt)

watch(prompt, (p) => {
  if (p) {
    value.value = ''
    remember.value = true
  }
})

// Store the typed secret so the next connect skips this prompt. Hosts with a
// linked credential keep it on the credential, inline hosts on themselves —
// matching where the backend looks it up on connect.
async function rememberSecret(hostId: number, kind: 'password' | 'passphrase', secret: string) {
  const host = hosts.hostById(hostId)
  if (!host) return
  if (host.credential_id !== null) {
    const cred = credentials.byId(host.credential_id)
    if (!cred) return
    await credentials.save(cred.id, {
      name: cred.name,
      username: cred.username,
      auth_method: cred.auth_method,
      password: kind === 'password' ? secret : null,
      save_password: kind === 'password' ? true : cred.password_saved,
      key_path: cred.key_path ?? null,
      key_passphrase: kind === 'passphrase' ? secret : null,
    })
  } else {
    await hosts.saveHost({
      id: host.id,
      group_id: host.group_id,
      label: host.label,
      address: host.address,
      port: host.port,
      username: host.username,
      auth_method: host.auth_method,
      password: kind === 'password' ? secret : null,
      save_password: kind === 'password' ? true : host.password_saved,
      key_path: host.key_path ?? null,
      key_passphrase: kind === 'passphrase' ? secret : null,
      credential_id: null,
    })
  }
}

async function submit() {
  if (!prompt.value || !value.value) return
  const { hostId, kind } = prompt.value
  const secret = value.value
  const shouldRemember = remember.value
  submitting.value = true
  try {
    dialogs.closePasswordPrompt()
    if (shouldRemember) {
      try {
        await rememberSecret(hostId, kind, secret)
      } catch (e) {
        ui.toast('error', `Could not save ${kind === 'password' ? 'password' : 'passphrase'}: ${String(e)}`)
      }
    }
    // The typed secret drives this connection even if the keyring write failed
    await sessions.connect(hostId, secret)
  } finally {
    submitting.value = false
    value.value = ''
  }
}

function cancel() {
  dialogs.closePasswordPrompt()
}
</script>

<template>
  <BaseModal
    v-if="prompt"
    :title="prompt.kind === 'password' ? 'Password' : 'Key passphrase'"
    :width="380"
    :close-on-scrim="true"
    @close="cancel"
  >
    <form class="prompt" @submit.prevent="submit">
      <p class="prompt-lead">
        <KeyRound :size="14" :stroke-width="1.5" aria-hidden="true" />
        {{ prompt.kind === 'password' ? 'Enter the password for' : 'Enter the passphrase for' }}
        <strong>{{ prompt.label }}</strong>
      </p>
      <input
        v-model="value"
        class="field-input"
        type="password"
        :placeholder="prompt.kind === 'password' ? 'Password' : 'Passphrase'"
        autocomplete="new-password"
        autofocus
      />
      <label class="check">
        <input v-model="remember" type="checkbox" />
        {{ prompt.kind === 'password' ? 'Remember password' : 'Remember passphrase' }}
      </label>
      <footer class="prompt-actions">
        <button type="button" class="btn btn-ghost" @click="cancel">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="!value || submitting">
          {{ submitting ? 'Connecting…' : 'Connect' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.prompt {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.prompt-lead {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: var(--text-base);
  color: var(--text-2);
}

.prompt-lead strong {
  color: var(--text-1);
  font-weight: 500;
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

.prompt-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
}
</style>
