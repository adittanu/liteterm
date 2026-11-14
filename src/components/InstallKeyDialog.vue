<script setup lang="ts">
import { computed, ref } from 'vue'
import BaseModal from './BaseModal.vue'
import SearchSelect, { type ComboOption } from './SearchSelect.vue'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { installKeyOn, switchHostToKey, type KeySource } from '../utils/installKey'

// ssh-copy-id from the Keychain: pick a saved host, LiteTerm logs in the way
// it does today and appends the public key to ~/.ssh/authorized_keys.

const props = defineProps<{ source: KeySource; keyName: string }>()
const emit = defineEmits<{ close: [] }>()

const hosts = useHostsStore()
const ui = useUiStore()

const hostId = ref<number | null>(null)
const switchLogin = ref(true)
const busy = ref(false)

const hostOptions = computed<ComboOption[]>(() =>
  [...hosts.hosts]
    .sort((a, b) => (b.last_used_at ?? 0) - (a.last_used_at ?? 0) || a.label.localeCompare(b.label))
    .map((h) => ({ value: h.id, label: h.label, hint: `${h.username}@${h.address}` })),
)
const host = computed(() => (hostId.value === null ? undefined : hosts.hostById(hostId.value)))
// Already logging in with this very key: nothing to switch
const alreadyUses = computed(
  () =>
    !!host.value &&
    host.value.auth_method === 'key' &&
    (host.value.key_path === props.source.keyPath ||
      (!!props.source.credentialId && host.value.credential_id === props.source.credentialId)),
)

async function install() {
  const h = host.value
  if (!h || busy.value) return
  busy.value = true
  const source = props.source
  const switchTo = switchLogin.value && !alreadyUses.value
  // Close first: the login may stop for a password or host key prompt, and
  // that dialog must not sit underneath this one
  emit('close')
  const pending = ui.toast('info', `Installing the key on ${h.label}…`, { duration: 0 })
  try {
    const result = await installKeyOn(h.id, source.publicKey)
    if (result === null) {
      ui.toast('error', `Could not connect to ${h.label}, so the key was not installed`)
      return
    }
    if (switchTo) await switchHostToKey(h.id, source)
    ui.toast(
      'success',
      result === 'added'
        ? `Key added to ${h.label}${switchTo ? ', which now logs in with it' : ''}`
        : `${h.label} already had this key${switchTo ? '; it now logs in with it' : ''}`,
    )
  } catch (e) {
    ui.toast('error', `Could not install the key on ${h.label}: ${errorText(e)}`)
  } finally {
    ui.dismissToast(pending)
  }
}
</script>

<template>
  <BaseModal :title="`Install ${keyName} on a host`" :width="460" @close="emit('close')">
    <form class="ik-form" novalidate @submit.prevent="install">
      <p class="ik-intro">
        LiteTerm logs in the way it does today and adds the public key to the server's
        <span class="mono">~/.ssh/authorized_keys</span>.
      </p>

      <div class="field">
        <label class="field-label" for="ik-host">Host</label>
        <SearchSelect id="ik-host" v-model="hostId" :options="hostOptions" aria-label="Host" />
      </div>

      <label v-if="host && !alreadyUses" class="check">
        <input v-model="switchLogin" type="checkbox" />
        <span>Log in to this host with the key from now on</span>
      </label>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="!host || busy">
          {{ busy ? 'Installing…' : 'Install key' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.ik-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.ik-intro {
  font-size: var(--text-sm);
  color: var(--text-2);
  line-height: 1.5;
}

.check {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-1);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
