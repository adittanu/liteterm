<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { KeyRound, Fingerprint } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import VaultField from './VaultField.vue'
import { useCredentialsStore } from '../stores/credentials'
import { useUiStore } from '../stores/ui'
import { usePrivacyStore } from '../stores/privacy'
import { api } from '../services/api'
import { errorText } from '../stores/sessions'
import { secretStore, sshAgentName } from '../utils/platform'
import type { AuthMethod, Credential } from '../types'

const props = defineProps<{ credential: Credential | null }>()
const emit = defineEmits<{ close: [] }>()

const credentials = useCredentialsStore()
const ui = useUiStore()
const pv = usePrivacyStore()

const form = reactive({
  name: '',
  username: '',
  auth_method: 'password' as AuthMethod,
  password: '',
  save_password: true,
  key_path: '',
  key_passphrase: '',
})

const errors = reactive<{ name?: string; username?: string; key_path?: string }>({})
const saving = ref(false)
// The vault a new credential goes in; an existing one stays where it is
const teamId = ref<string | null>(null)
const inTeam = computed(() => (props.credential ? props.credential.team_id : teamId.value) !== null)

// "Store the key in LiteTerm": always on in a team, which has no use for a
// file on this computer
const storeKey = ref(false)
// Editing one whose key is stored already: kept unless replaced
const keepStored = ref(false)

watch(
  () => props.credential,
  (c) => {
    form.name = c?.name ?? ''
    form.username = c?.username ?? ''
    form.auth_method = c?.auth_method ?? 'password'
    form.password = ''
    form.save_password = c?.password_saved ?? true
    form.key_path = c?.key_path ?? ''
    form.key_passphrase = ''
    storeKey.value = c?.key_stored ?? false
    keepStored.value = c?.key_stored ?? false
    errors.name = errors.username = errors.key_path = undefined
  },
  { immediate: true },
)

watch(
  inTeam,
  (team) => {
    if (team) storeKey.value = true
  },
  { immediate: true },
)

function validate(): boolean {
  let ok = true
  if (!form.name.trim()) { errors.name = 'Name this credential'; ok = false }
  if (!form.username.trim()) { errors.username = 'Username is required'; ok = false }
  if (form.auth_method === 'key' && !keepStored.value && !form.key_path.trim()) {
    errors.key_path = storeKey.value ? 'Choose the private key file to store' : 'Choose a private key file'
    ok = false
  }
  return ok
}

async function browseKey() {
  const path = await api.pickFile('Select private key', [
    { name: 'Private keys', extensions: ['pem', 'key', 'ppk', 'id_rsa', 'ed25519', 'ecdsa'] },
    { name: 'All files', extensions: ['*'] },
  ])
  if (path) form.key_path = path
}

/** Store another key file in place of the stored one */
async function replaceStored() {
  await browseKey()
  if (!form.key_path.trim()) return
  keepStored.value = false
  storeKey.value = true
}

function useFile() {
  keepStored.value = false
  storeKey.value = false
  form.key_path = ''
}

async function save() {
  if (!validate()) return
  saving.value = true
  try {
    const key = form.auth_method === 'key'
    await credentials.save(props.credential?.id ?? null, {
      name: form.name.trim(),
      username: form.username.trim(),
      auth_method: form.auth_method,
      password: form.password ? form.password : null,
      // Agent auth stores no secret — dropping the flag clears an old password
      save_password: form.auth_method === 'agent' ? false : form.save_password,
      key_path: key && !keepStored.value ? form.key_path.trim() : null,
      key_passphrase: key && !keepStored.value && form.key_passphrase ? form.key_passphrase : null,
      // Left out while the stored key stays as it is
      store_key: key && !keepStored.value ? storeKey.value : null,
      team_id: teamId.value,
    })
    emit('close')
  } catch (e) {
    ui.toast('error', `Could not save credential: ${errorText(e)}`)
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <BaseModal
    :title="credential ? 'Edit Credential' : 'New Credential'"
    :width="460"
    @close="emit('close')"
  >
    <form class="cred-form" @submit.prevent="save">
      <div class="field">
        <label class="field-label" for="cf-name">Name</label>
        <input
          id="cf-name"
          v-model="form.name"
          class="field-input pv-field"
          :class="{ invalid: errors.name }"
          type="text"
          :placeholder="pv.t('e.g. admindqli')"
          spellcheck="false"
        />
        <span v-if="errors.name" class="field-error">{{ errors.name }}</span>
      </div>

      <VaultField v-if="!credential" id="cf-vault" v-model="teamId" />

      <div class="field">
        <label class="field-label" for="cf-user">Username</label>
        <input
          id="cf-user"
          v-model="form.username"
          class="field-input mono pv-field"
          :class="{ invalid: errors.username }"
          type="text"
          placeholder="root"
          spellcheck="false"
          autocapitalize="off"
        />
        <span v-if="errors.username" class="field-error">{{ errors.username }}</span>
      </div>

      <div class="field">
        <span class="field-label">Authentication</span>
        <div class="auth-toggle" role="radiogroup" aria-label="Authentication method">
          <button
            type="button"
            class="auth-option"
            :class="{ on: form.auth_method === 'password' }"
            role="radio"
            :aria-checked="form.auth_method === 'password'"
            @click="form.auth_method = 'password'"
          >
            Password
          </button>
          <button
            type="button"
            class="auth-option"
            :class="{ on: form.auth_method === 'key' }"
            role="radio"
            :aria-checked="form.auth_method === 'key'"
            @click="form.auth_method = 'key'"
          >
            <KeyRound :size="13" :stroke-width="1.5" aria-hidden="true" />
            Private key
          </button>
          <button
            type="button"
            class="auth-option"
            :class="{ on: form.auth_method === 'agent' }"
            role="radio"
            :aria-checked="form.auth_method === 'agent'"
            @click="form.auth_method = 'agent'"
          >
            <Fingerprint :size="13" :stroke-width="1.5" aria-hidden="true" />
            SSH agent
          </button>
        </div>
        <span v-if="form.auth_method === 'agent'" class="field-hint">
          Keys come from the running SSH agent ({{ sshAgentName }}, 1Password,
          Bitwarden, KeePassXC). Nothing secret is stored.
        </span>
      </div>

      <template v-if="form.auth_method === 'password'">
        <div class="field">
          <label class="field-label" for="cf-pass">Password</label>
          <input
            id="cf-pass"
            v-model="form.password"
            class="field-input"
            type="password"
            :placeholder="credential?.has_password ? '•••••••• (saved)' : 'Leave empty to type on connect'"
            autocomplete="new-password"
          />
          <span class="field-hint">
            <label class="check">
              <input v-model="form.save_password" type="checkbox" />
              Remember password ({{ secretStore }})
            </label>
          </span>
        </div>
      </template>

      <template v-else-if="form.auth_method === 'key'">
        <!-- Stored in LiteTerm already: kept unless replaced -->
        <div v-if="keepStored" class="field">
          <span class="field-label">Private key</span>
          <div class="stored-key">
            <KeyRound :size="14" :stroke-width="1.5" class="stored-icon" aria-hidden="true" />
            <span class="stored-text">Stored in LiteTerm</span>
            <button type="button" class="btn btn-secondary" @click="replaceStored">Replace…</button>
            <button v-if="!inTeam" type="button" class="btn btn-ghost" @click="useFile">Use a key file</button>
          </div>
          <span class="field-hint">
            It syncs with the credential, encrypted, so it works on every computer{{ inTeam ? ' and for everyone in the team' : '' }}.
          </span>
        </div>

        <template v-else>
          <div class="field">
            <label class="field-label" for="cf-key">Private key file</label>
            <div class="key-row">
              <input
                id="cf-key"
                v-model="form.key_path"
                class="field-input mono"
                :class="{ invalid: errors.key_path }"
                type="text"
                placeholder="C:\Users\you\.ssh\id_ed25519"
                spellcheck="false"
              />
              <button type="button" class="btn btn-secondary" @click="browseKey">Browse…</button>
            </div>
            <span v-if="errors.key_path" class="field-error">{{ errors.key_path }}</span>
            <span v-else-if="!storeKey" class="field-hint">A certificate next to it (<span class="mono">id_ed25519-cert.pub</span>) is offered first.</span>
          </div>
          <div class="field">
            <label class="check">
              <input v-model="storeKey" type="checkbox" :disabled="inTeam" />
              Store the key in LiteTerm
            </label>
            <span class="field-hint">
              <template v-if="inTeam">A team credential carries its key, so everyone in the team can sign in with it.</template>
              <template v-else>The key syncs with the credential, encrypted, so it works on your other computers too.</template>
            </span>
          </div>
          <div class="field">
            <label class="field-label" for="cf-keypass">Key passphrase</label>
            <input
              id="cf-keypass"
              v-model="form.key_passphrase"
              class="field-input"
              type="password"
              placeholder="Leave empty if the key has none"
              autocomplete="new-password"
            />
            <span v-if="storeKey" class="field-hint">Unlocks the key once, before it's stored. It isn't kept.</span>
          </div>
        </template>
      </template>

      <footer class="form-actions">
        <button type="button" class="btn btn-ghost" @click="emit('close')">Cancel</button>
        <button type="submit" class="btn btn-primary" :disabled="saving">
          {{ saving ? 'Saving…' : 'Save credential' }}
        </button>
      </footer>
    </form>
  </BaseModal>
</template>

<style scoped>
.cred-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.field-input.invalid {
  border-color: var(--danger);
}

.key-row {
  display: flex;
  gap: var(--space-2);
}

.stored-key {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 6px 6px 6px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-input);
}

.stored-icon {
  flex-shrink: 0;
  color: var(--text-3);
}

.stored-text {
  flex: 1;
  min-width: 0;
  font-size: var(--text-sm);
  color: var(--text-1);
}

.key-row .field-input {
  flex: 1;
  min-width: 0;
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

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: var(--space-2);
  padding-top: var(--space-2);
}
</style>
