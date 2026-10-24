<script setup lang="ts">
import { reactive, ref, watch } from 'vue'
import { KeyRound } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useCredentialsStore } from '../stores/credentials'
import { useUiStore } from '../stores/ui'
import { api } from '../services/api'
import { errorText } from '../stores/sessions'
import type { Credential } from '../types'

const props = defineProps<{ credential: Credential | null }>()
const emit = defineEmits<{ close: [] }>()

const credentials = useCredentialsStore()
const ui = useUiStore()

const form = reactive({
  name: '',
  username: '',
  auth_method: 'password' as 'password' | 'key',
  password: '',
  save_password: true,
  key_path: '',
  key_passphrase: '',
})

const errors = reactive<{ name?: string; username?: string; key_path?: string }>({})
const saving = ref(false)

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
    errors.name = errors.username = errors.key_path = undefined
  },
  { immediate: true },
)

function validate(): boolean {
  let ok = true
  if (!form.name.trim()) { errors.name = 'Name this credential'; ok = false }
  if (!form.username.trim()) { errors.username = 'Username is required'; ok = false }
  if (form.auth_method === 'key' && !form.key_path.trim()) { errors.key_path = 'Choose a private key file'; ok = false }
  return ok
}

async function browseKey() {
  const path = await api.pickFile('Select private key', [
    { name: 'Private keys', extensions: ['pem', 'key', 'ppk', 'id_rsa', 'ed25519', 'ecdsa'] },
    { name: 'All files', extensions: ['*'] },
  ])
  if (path) form.key_path = path
}

async function save() {
  if (!validate()) return
  saving.value = true
  try {
    await credentials.save(props.credential?.id ?? null, {
      name: form.name.trim(),
      username: form.username.trim(),
      auth_method: form.auth_method,
      password: form.password ? form.password : null,
      save_password: form.save_password,
      key_path: form.auth_method === 'key' ? form.key_path.trim() : null,
      key_passphrase: form.key_passphrase ? form.key_passphrase : null,
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
          class="field-input"
          :class="{ invalid: errors.name }"
          type="text"
          placeholder="e.g. admindqli"
          spellcheck="false"
        />
        <span v-if="errors.name" class="field-error">{{ errors.name }}</span>
      </div>

      <div class="field">
        <label class="field-label" for="cf-user">Username</label>
        <input
          id="cf-user"
          v-model="form.username"
          class="field-input mono"
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
        </div>
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
              Remember password (Windows Credential Manager)
            </label>
          </span>
        </div>
      </template>

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
        </div>
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
