<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { Copy, RefreshCw } from 'lucide-vue-next'
import { useCloudStore } from '../stores/cloud'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { copyText, plural, relativeAge } from '../utils/pageHelpers'

// Matches the backend (cloud/vault.rs): counted after NFC normalization
const MIN_PASSWORD = 10

const cloud = useCloudStore()
const ui = useUiStore()
const status = computed(() => cloud.status)

const busy = ref(false)
const error = ref('')

async function run(action: () => Promise<unknown>) {
  if (busy.value) return
  busy.value = true
  error.value = ''
  try {
    await action()
  } catch (e) {
    error.value = errorText(e)
  } finally {
    busy.value = false
  }
}

// "Synced 3 min ago" has to age while the pane is open
const now = ref(new Date())
let clock: ReturnType<typeof setInterval> | null = null
onMounted(() => {
  void cloud.refresh().catch(() => {})
  clock = setInterval(() => (now.value = new Date()), 30_000)
})

// ---- sign in ----
const email = ref(cloud.status.email ?? '')
const codeSentTo = ref<string | null>(null)
const code = ref('')
const resendIn = ref(0)
let resendTimer: ReturnType<typeof setInterval> | null = null

onUnmounted(() => {
  if (clock) clearInterval(clock)
  if (resendTimer) clearInterval(resendTimer)
})

// The status may arrive after this mounts, or after a session ends
watch(
  () => status.value.email,
  (known) => {
    if (known && !email.value) email.value = known
  },
)

const emailValid = computed(() => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.value.trim()))
const codeDigits = computed(() => code.value.replace(/\D/g, ''))

function startCooldown() {
  // The server sends at most one email a minute per address
  resendIn.value = 60
  if (resendTimer) clearInterval(resendTimer)
  resendTimer = setInterval(() => {
    resendIn.value -= 1
    if (resendIn.value <= 0 && resendTimer) {
      clearInterval(resendTimer)
      resendTimer = null
    }
  }, 1000)
}

function sendCode() {
  return run(async () => {
    const to = email.value.trim()
    await cloud.sendCode(to)
    codeSentTo.value = to
    code.value = ''
    startCooldown()
  })
}

function verify() {
  return run(async () => {
    await cloud.verifyCode(codeSentTo.value ?? email.value.trim(), codeDigits.value)
    codeSentTo.value = null
    code.value = ''
  })
}

function useAnotherEmail() {
  codeSentTo.value = null
  code.value = ''
  error.value = ''
}

// ---- master password ----
const password = ref('')
const confirmPassword = ref('')
const currentPassword = ref('')
const recoveryInput = ref('')

const charCount = (s: string) => [...s.normalize('NFC')].length
const tooShort = computed(() => password.value.length > 0 && charCount(password.value) < MIN_PASSWORD)
const mismatch = computed(() => confirmPassword.value.length > 0 && confirmPassword.value !== password.value)
const newPasswordOk = computed(
  () => charCount(password.value) >= MIN_PASSWORD && password.value === confirmPassword.value,
)

function clearForms() {
  password.value = ''
  confirmPassword.value = ''
  currentPassword.value = ''
  recoveryInput.value = ''
}

// Shown once, right after it is made
const recoveryKey = ref<string | null>(null)
const recoverySaved = ref(false)

function showRecoveryKey(key: string) {
  recoveryKey.value = key
  recoverySaved.value = false
}

function createVault() {
  return run(async () => {
    showRecoveryKey(await cloud.createVault(password.value))
    clearForms()
  })
}

// ---- locked ----
const lockedMode = ref<'password' | 'recovery' | 'reset'>('password')

function unlock() {
  return run(async () => {
    await cloud.unlock(currentPassword.value)
    clearForms()
  })
}

function recover() {
  return run(async () => {
    showRecoveryKey(await cloud.recover(recoveryInput.value, password.value))
    clearForms()
    lockedMode.value = 'password'
  })
}

function resetVault() {
  return run(async () => {
    await cloud.resetVault()
    lockedMode.value = 'password'
  })
}

// ---- ready ----
const changing = ref(false)

function changePassword() {
  return run(async () => {
    await cloud.changePassword(currentPassword.value, password.value)
    clearForms()
    changing.value = false
    ui.toast('success', 'Master password changed')
  })
}

function cancelChange() {
  changing.value = false
  clearForms()
  error.value = ''
}

const syncNow = () => run(() => cloud.syncNow())

function signOut() {
  return run(async () => {
    await cloud.signOut()
    email.value = cloud.status.email ?? email.value
  })
}

const syncText = computed(() => {
  const s = status.value
  if (s.syncing) return 'Syncing…'
  if (s.error) return s.error
  if (s.lastSyncAt) return `Synced ${relativeAge(new Date(s.lastSyncAt), now.value)}`
  return 'Not synced yet'
})

// A new state is a new form: no stale error or half-typed password
watch(
  () => status.value.state,
  () => {
    error.value = ''
    clearForms()
    changing.value = false
    lockedMode.value = 'password'
  },
)
</script>

<template>
  <div class="account">
    <!-- Why this device left "ready" (session ended, vault reset elsewhere) -->
    <p v-if="status.error && status.state !== 'ready' && !recoveryKey" class="notice" role="status">
      {{ status.error }}
    </p>

    <!-- ============ Recovery key, shown once ============ -->
    <section v-if="recoveryKey" class="block">
      <div class="intro">
        <span class="row-label">Save your recovery key</span>
        <p class="desc">
          If you forget the master password, this key is the only way back into your vault.
          Keep it somewhere safe, like a password manager or on paper. It won’t be shown again.
        </p>
      </div>
      <div class="key-box">
        <code class="key">{{ recoveryKey }}</code>
        <button class="btn btn-secondary" type="button" @click="copyText(recoveryKey, 'Recovery key copied')">
          <Copy :size="14" :stroke-width="1.5" aria-hidden="true" />
          Copy
        </button>
      </div>
      <label class="check">
        <input v-model="recoverySaved" type="checkbox" />
        I’ve saved my recovery key
      </label>
      <div class="actions">
        <button class="btn btn-primary" type="button" :disabled="!recoverySaved" @click="recoveryKey = null">
          Done
        </button>
      </div>
    </section>

    <!-- ============ Signed out ============ -->
    <template v-else-if="status.state === 'signedOut'">
      <div class="intro">
        <span class="row-label">Sync across your computers</span>
        <p class="desc">
          Sign in to keep hosts, credentials, snippets, notes and port forwards the same on every
          computer. Everything is encrypted here with your master password before it’s uploaded,
          so the server can’t read it. Without an account, LiteTerm keeps working as it does now.
        </p>
      </div>

      <form v-if="!codeSentTo" class="form" @submit.prevent="sendCode">
        <label class="field">
          <span class="field-label">Email</span>
          <input
            v-model="email"
            class="field-input"
            type="email"
            autocomplete="email"
            spellcheck="false"
            placeholder="you@example.com"
          />
        </label>
        <div class="actions">
          <button class="btn btn-primary" type="submit" :disabled="busy || !emailValid">
            {{ busy ? 'Sending…' : 'Email me a sign-in code' }}
          </button>
        </div>
      </form>

      <form v-else class="form" @submit.prevent="verify">
        <label class="field">
          <span class="field-label">Sign-in code</span>
          <input
            v-model="code"
            class="field-input code-input"
            inputmode="numeric"
            autocomplete="one-time-code"
            maxlength="12"
            spellcheck="false"
          />
          <span class="field-hint">Sent to {{ codeSentTo }}. It works once, for an hour.</span>
        </label>
        <div class="actions">
          <button class="btn btn-primary" type="submit" :disabled="busy || codeDigits.length < 6">
            {{ busy ? 'Signing in…' : 'Sign in' }}
          </button>
          <button class="btn btn-ghost" type="button" :disabled="busy || resendIn > 0" @click="sendCode">
            {{ resendIn > 0 ? `Send a new code in ${resendIn}s` : 'Send a new code' }}
          </button>
          <button class="btn btn-ghost" type="button" :disabled="busy" @click="useAnotherEmail">
            Use another email
          </button>
        </div>
      </form>
    </template>

    <!-- ============ Signed in, no vault yet ============ -->
    <template v-else-if="status.state === 'noVault'">
      <div class="intro">
        <span class="row-label">Set a master password</span>
        <p class="desc">
          It encrypts your data before it leaves this computer. It’s never sent anywhere, so
          nobody can reset it for you; you’ll get a recovery key in case you forget it.
        </p>
      </div>
      <form class="form" @submit.prevent="createVault">
        <label class="field">
          <span class="field-label">Master password</span>
          <input v-model="password" class="field-input" type="password" autocomplete="new-password" />
          <span v-if="tooShort" class="field-error">Use at least {{ MIN_PASSWORD }} characters</span>
        </label>
        <label class="field">
          <span class="field-label">Confirm master password</span>
          <input v-model="confirmPassword" class="field-input" type="password" autocomplete="new-password" />
          <span v-if="mismatch" class="field-error">The passwords don’t match</span>
        </label>
        <div class="actions">
          <button class="btn btn-primary" type="submit" :disabled="busy || !newPasswordOk">
            {{ busy ? 'Creating…' : 'Create vault' }}
          </button>
        </div>
      </form>
    </template>

    <!-- ============ Signed in, vault locked on this computer ============ -->
    <template v-else-if="status.state === 'locked'">
      <template v-if="lockedMode === 'password'">
        <div class="intro">
          <span class="row-label">Unlock your vault</span>
          <p class="desc">Enter the master password you set when you created your vault.</p>
        </div>
        <form class="form" @submit.prevent="unlock">
          <label class="field">
            <span class="field-label">Master password</span>
            <input v-model="currentPassword" class="field-input" type="password" autocomplete="current-password" />
          </label>
          <div class="actions">
            <button class="btn btn-primary" type="submit" :disabled="busy || !currentPassword">
              {{ busy ? 'Unlocking…' : 'Unlock' }}
            </button>
            <button class="btn btn-ghost" type="button" :disabled="busy" @click="lockedMode = 'recovery'">
              Forgot it? Use your recovery key
            </button>
          </div>
        </form>
      </template>

      <template v-else-if="lockedMode === 'recovery'">
        <div class="intro">
          <span class="row-label">Use your recovery key</span>
          <p class="desc">
            Enter the recovery key you saved, then choose a new master password. You’ll get a new
            recovery key; the old one stops working.
          </p>
        </div>
        <form class="form" @submit.prevent="recover">
          <label class="field">
            <span class="field-label">Recovery key</span>
            <input
              v-model="recoveryInput"
              class="field-input key-input"
              spellcheck="false"
              autocomplete="off"
              placeholder="XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX-XXXX"
            />
          </label>
          <label class="field">
            <span class="field-label">New master password</span>
            <input v-model="password" class="field-input" type="password" autocomplete="new-password" />
            <span v-if="tooShort" class="field-error">Use at least {{ MIN_PASSWORD }} characters</span>
          </label>
          <label class="field">
            <span class="field-label">Confirm new master password</span>
            <input v-model="confirmPassword" class="field-input" type="password" autocomplete="new-password" />
            <span v-if="mismatch" class="field-error">The passwords don’t match</span>
          </label>
          <div class="actions">
            <button class="btn btn-primary" type="submit" :disabled="busy || !recoveryInput.trim() || !newPasswordOk">
              {{ busy ? 'Recovering…' : 'Recover vault' }}
            </button>
            <button class="btn btn-ghost" type="button" :disabled="busy" @click="lockedMode = 'password'">Back</button>
            <button class="btn btn-ghost" type="button" :disabled="busy" @click="lockedMode = 'reset'">
              Lost both? Reset the vault
            </button>
          </div>
        </form>
      </template>

      <template v-else>
        <div class="intro">
          <span class="row-label">Reset the cloud vault?</span>
          <p class="desc">
            This deletes everything stored in your cloud vault. What’s on this computer stays, and is
            uploaded again once you set a new master password. Your other computers will have to
            unlock with the new one.
          </p>
        </div>
        <div class="actions">
          <button class="btn btn-danger" type="button" :disabled="busy" @click="resetVault">
            {{ busy ? 'Deleting…' : 'Delete cloud data' }}
          </button>
          <button class="btn btn-ghost" type="button" :disabled="busy" @click="lockedMode = 'recovery'">Cancel</button>
        </div>
      </template>
    </template>

    <!-- ============ Ready ============ -->
    <template v-else>
      <div class="row">
        <div class="row-text">
          <span class="row-label">Sync</span>
          <span class="row-desc" :class="{ bad: status.error && !status.syncing }">
            {{ syncText }}<template v-if="status.pending && !status.syncing">
              · {{ plural(status.pending, 'change') }} waiting</template>
          </span>
        </div>
        <button class="btn btn-secondary" type="button" :disabled="busy || status.syncing" @click="syncNow">
          <RefreshCw :size="14" :stroke-width="1.5" :class="{ spin: status.syncing }" aria-hidden="true" />
          Sync now
        </button>
      </div>

      <div class="row">
        <div class="row-text">
          <span class="row-label">Master password</span>
          <span class="row-desc">Encrypts your data on this computer. Changing it keeps your recovery key.</span>
        </div>
        <button v-if="!changing" class="btn btn-secondary" type="button" @click="changing = true">Change…</button>
      </div>
      <form v-if="changing" class="form nested" @submit.prevent="changePassword">
        <label class="field">
          <span class="field-label">Current master password</span>
          <input v-model="currentPassword" class="field-input" type="password" autocomplete="current-password" />
        </label>
        <label class="field">
          <span class="field-label">New master password</span>
          <input v-model="password" class="field-input" type="password" autocomplete="new-password" />
          <span v-if="tooShort" class="field-error">Use at least {{ MIN_PASSWORD }} characters</span>
        </label>
        <label class="field">
          <span class="field-label">Confirm new master password</span>
          <input v-model="confirmPassword" class="field-input" type="password" autocomplete="new-password" />
          <span v-if="mismatch" class="field-error">The passwords don’t match</span>
        </label>
        <div class="actions">
          <button class="btn btn-primary" type="submit" :disabled="busy || !currentPassword || !newPasswordOk">
            {{ busy ? 'Saving…' : 'Change password' }}
          </button>
          <button class="btn btn-ghost" type="button" :disabled="busy" @click="cancelChange">Cancel</button>
        </div>
      </form>

      <div class="row">
        <div class="row-text">
          <span class="row-label">What syncs</span>
          <span class="row-desc">
            Hosts, groups, credentials with their saved passwords, snippets, notes, port forwards
            and known hosts. Private key files stay on each computer: only their path syncs, and
            images in notes don’t sync yet.
          </span>
        </div>
      </div>
    </template>

    <!-- Who is signed in, under every signed-in step -->
    <div v-if="!recoveryKey && status.state !== 'signedOut'" class="row account-row">
      <div class="row-text">
        <span class="row-label">Account</span>
        <span class="row-desc">{{ status.email }}</span>
      </div>
      <button class="btn btn-ghost" type="button" :disabled="busy" @click="signOut">Sign out</button>
    </div>

    <p v-if="error" class="field-error form-error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.account {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.block {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.intro {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.desc {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: 1.5;
  max-width: 60ch;
}

.form {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  max-width: 360px;
}

.form.nested {
  padding-bottom: var(--space-2);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.code-input {
  font-family: var(--font-mono);
  font-size: var(--text-lg);
  letter-spacing: 0.2em;
}

.key-input {
  font-family: var(--font-mono);
}

.key-box {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.key {
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: var(--text-md);
  color: var(--text-1);
  user-select: all;
}

.check {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-2);
  cursor: pointer;
}

.check input {
  accent-color: var(--accent);
}

/* Same rows as the other settings sections */
.row {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: 12px 0;
}

.row + .row,
.form.nested + .row {
  border-top: 1px solid var(--border);
}

.account-row {
  border-top: 1px solid var(--border);
}

.row > .btn {
  margin-left: auto;
  flex-shrink: 0;
}

.row-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.row-label {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-1);
}

.row-desc {
  font-size: var(--text-xs);
  color: var(--text-3);
  line-height: 1.45;
  overflow-wrap: anywhere;
}

.row-desc.bad {
  color: var(--danger);
}

.form-error {
  margin: 0;
}

.notice {
  margin: 0;
  padding: 8px 10px;
  border-radius: var(--radius-md);
  background: var(--warning-bg);
  color: var(--text-1);
  font-size: var(--text-xs);
  line-height: 1.45;
}

.spin {
  animation: spin 1s linear infinite;
}

@media (prefers-reduced-motion: reduce) {
  .spin {
    animation: none;
  }
}
</style>
