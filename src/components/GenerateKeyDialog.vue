<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { Copy } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { api } from '../services/api'
import { useCredentialsStore } from '../stores/credentials'
import { errorText } from '../stores/sessions'
import { copyText } from '../utils/pageHelpers'
import { secretStore } from '../utils/platform'
import type { GeneratedKey } from '../types'
import type { KeySource } from '../utils/installKey'

// ssh-keygen -t ed25519, then (optionally) a credential that uses the key.
// The second step shows the public key to copy or install on a host.

const emit = defineEmits<{ close: []; install: [source: KeySource, keyName: string] }>()

const credentials = useCredentialsStore()

const name = ref('')
const comment = ref('')
const dir = ref('~/.ssh')
const passphrase = ref('')
const confirm = ref('')
const asCredential = ref(true)
const credName = ref('')
const username = ref('')
const submitted = ref(false)
const busy = ref(false)
const error = ref('')
const nameEl = ref<HTMLInputElement | null>(null)

const made = ref<{ key: GeneratedKey; credentialId: number | null; passphrase: string } | null>(null)

const errors = computed(() => ({
  name: /^[A-Za-z0-9_-][A-Za-z0-9._-]*$/.test(name.value.trim()) ? '' : 'Letters, digits, dots, dashes or underscores',
  confirm: passphrase.value === confirm.value ? '' : 'The passphrases differ',
  username: !asCredential.value || username.value.trim() ? '' : 'The login this key is for',
}))
const valid = computed(() => !errors.value.name && !errors.value.confirm && !errors.value.username)

onMounted(async () => {
  try {
    const d = await api.keygenDefaults()
    name.value = d.name
    comment.value = d.comment
    dir.value = d.dir
  } catch {
    name.value = 'id_ed25519'
  }
  void nextTick(() => {
    nameEl.value?.focus()
    nameEl.value?.select()
  })
})

async function generate() {
  submitted.value = true
  if (!valid.value || busy.value) return
  busy.value = true
  error.value = ''
  try {
    const key = await api.keygen(name.value.trim(), comment.value.trim(), passphrase.value || null)
    let credentialId: number | null = null
    if (asCredential.value) {
      const cred = await credentials.save(null, {
        name: credName.value.trim() || name.value.trim(),
        username: username.value.trim(),
        auth_method: 'key',
        key_path: key.path,
        key_passphrase: passphrase.value || null,
      })
      credentialId = cred.id
    }
    made.value = { key, credentialId, passphrase: passphrase.value }
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}

function install() {
  const m = made.value
  if (!m) return
  emit(
    'install',
    { publicKey: m.key.publicKey, keyPath: m.key.path, passphrase: m.passphrase || null, credentialId: m.credentialId },
    m.key.path.split(/[\\/]/).pop() ?? 'key',
  )
}
</script>

<template>
  <BaseModal :title="made ? 'Key created' : 'Generate SSH key'" :width="500" @close="emit('close')">
    <form v-if="!made" class="gk-form" novalidate @submit.prevent="generate">
      <div class="field">
        <label class="field-label" for="gk-name">File name</label>
        <input
          id="gk-name"
          ref="nameEl"
          v-model="name"
          class="field-input mono"
          :class="{ invalid: submitted && errors.name }"
          type="text"
          spellcheck="false"
          autocapitalize="off"
        />
        <span v-if="submitted && errors.name" class="field-error">{{ errors.name }}</span>
        <span v-else class="field-hint mono">{{ dir }}/{{ name.trim() || '…' }} and .pub · Ed25519</span>
      </div>

      <div class="field">
        <label class="field-label" for="gk-comment">Comment</label>
        <input id="gk-comment" v-model="comment" class="field-input mono" type="text" spellcheck="false" />
        <span class="field-hint">Shown next to the key in authorized_keys, so you can tell it apart.</span>
      </div>

      <div class="gk-row">
        <div class="field">
          <label class="field-label" for="gk-pass">Passphrase</label>
          <input id="gk-pass" v-model="passphrase" class="field-input" type="password" autocomplete="new-password" placeholder="Optional" />
        </div>
        <div class="field">
          <label class="field-label" for="gk-confirm">Repeat passphrase</label>
          <input
            id="gk-confirm"
            v-model="confirm"
            class="field-input"
            :class="{ invalid: submitted && errors.confirm }"
            type="password"
            autocomplete="new-password"
            :disabled="!passphrase"
          />
          <span v-if="submitted && errors.confirm" class="field-error">{{ errors.confirm }}</span>
        </div>
      </div>

      <div class="divider"></div>

      <label class="check">
        <input v-model="asCredential" type="checkbox" />
        <span>Save as a credential, so hosts can pick it</span>
      </label>
      <div v-if="asCredential" class="gk-row">
        <div class="field">
          <label class="field-label" for="gk-user">Username</label>
          <input
            id="gk-user"
            v-model="username"
            class="field-input mono"
            :class="{ invalid: submitted && errors.username }"
            type="text"
            placeholder="root"
            spellcheck="false"
            autocapitalize="off"
          />
          <span v-if="submitted && errors.username" class="field-error">{{ errors.username }}</span>
        </div>
        <div class="field">
          <label class="field-label" for="gk-cred">Credential name</label>
          <input id="gk-cred" v-model="credName" class="field-input" type="text" :placeholder="name.trim()" spellcheck="false" />
        </div>
      </div>
      <span v-if="asCredential && passphrase" class="field-hint">The passphrase is kept in the {{ secretStore }}.</span>

      <p v-if="error" class="field-error" role="alert">{{ error }}</p>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="busy">{{ busy ? 'Generating…' : 'Generate key' }}</button>
      </footer>
    </form>

    <div v-else class="gk-form">
      <div class="field">
        <span class="field-label">Public key</span>
        <div class="gk-pub">
          <code class="mono">{{ made.key.publicKey }}</code>
          <button class="btn-icon" type="button" aria-label="Copy public key" title="Copy public key" @click="copyText(made.key.publicKey, 'Public key copied')">
            <Copy :size="14" :stroke-width="1.5" aria-hidden="true" />
          </button>
        </div>
        <span class="field-hint mono">{{ made.key.fingerprint }}</span>
      </div>
      <div class="field">
        <span class="field-label">Private key</span>
        <span class="mono gk-path">{{ made.key.path }}</span>
        <span class="field-hint">Never leaves this computer. Back it up if losing it would lock you out.</span>
      </div>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Done</button>
        <button type="button" class="btn btn-primary" @click="install">Install on a host…</button>
      </footer>
    </div>
  </BaseModal>
</template>

<style scoped>
.gk-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.gk-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-3);
}

.check {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-1);
}

.divider {
  height: 1px;
  background: var(--border);
}

.gk-pub {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--bg-surface-2);
}

.gk-pub code {
  flex: 1;
  min-width: 0;
  font-size: var(--text-xs);
  line-height: 1.5;
  color: var(--text-2);
  word-break: break-all;
}

.gk-path {
  font-size: var(--text-sm);
  color: var(--text-1);
  word-break: break-all;
}

.field-input.invalid {
  border-color: var(--danger);
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-1);
}
</style>
