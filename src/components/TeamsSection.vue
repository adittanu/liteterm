<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { ChevronRight, Copy, Plus, RefreshCw } from 'lucide-vue-next'
import { useCloudStore } from '../stores/cloud'
import { useTeamsStore } from '../stores/teams'
import { useDialogsStore } from '../stores/dialogs'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'
import { api } from '../services/api'
import { copyText, plural } from '../utils/pageHelpers'
import type { MemberView, PendingView, TeamDetail, TeamRole, TeamSummary } from '../types'

const emit = defineEmits<{ (e: 'go-account'): void }>()

const cloud = useCloudStore()
const teams = useTeamsStore()
const dialogs = useDialogsStore()
const ui = useUiStore()

const ready = computed(() => cloud.status.state === 'ready')

const ROLES: { id: TeamRole; label: string; desc: string }[] = [
  { id: 'viewer', label: 'Viewer', desc: 'Connects with what’s in the team, changes nothing' },
  { id: 'editor', label: 'Editor', desc: 'Also adds, changes and deletes items' },
  { id: 'admin', label: 'Admin', desc: 'Also invites and removes editors and viewers' },
  { id: 'owner', label: 'Owner', desc: 'Also manages admins, renames and deletes the team' },
]
const roleLabel = (r: TeamRole) => ROLES.find((x) => x.id === r)?.label ?? r
const canManage = (t: TeamSummary) => t.role === 'owner' || t.role === 'admin'

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

function load() {
  if (!ready.value) return
  void teams.load().catch(() => {})
}
onMounted(load)
watch(ready, load)

const dateText = (iso: string) => new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short' })

// ---- invites for this account ----
const acceptInvite = (id: string) => run(() => teams.acceptInvite(id))
const declineInvite = (id: string) => run(() => teams.declineInvite(id))

// ---- new team ----
const creating = ref(false)
const newName = ref('')

function createTeam() {
  return run(async () => {
    const team = await teams.createTeam(newName.value)
    newName.value = ''
    creating.value = false
    await open(team.id)
  })
}

// ---- one team, open ----
const openId = ref<string | null>(null)
const detail = ref<TeamDetail | null>(null)
const detailError = ref('')
const detailLoading = ref(false)
const openTeam = computed(() => teams.view?.teams.find((t) => t.id === openId.value) ?? null)

// A team that was left or deleted, here or elsewhere, closes
watch(openTeam, (t) => {
  if (!t && openId.value && teams.view) close()
})

async function loadDetail() {
  const id = openId.value
  if (!id) return
  detailLoading.value = true
  try {
    const next = await api.cloudTeamDetail(id)
    if (openId.value === id) {
      detail.value = next
      detailError.value = ''
    }
  } catch (e) {
    if (openId.value === id) detailError.value = errorText(e)
  } finally {
    detailLoading.value = false
  }
}

function close() {
  openId.value = null
  detail.value = null
  detailError.value = ''
  exposed.value = null
  renaming.value = false
  inviteEmail.value = ''
  for (const k of Object.keys(codeChecked)) delete codeChecked[k]
}

async function open(id: string) {
  close()
  openId.value = id
  await loadDetail()
}

const toggle = (id: string) => (openId.value === id ? close() : void open(id))

// ---- members ----
function canRemove(team: TeamSummary, m: MemberView) {
  if (m.isMe) return false
  if (team.role === 'owner') return true
  return team.role === 'admin' && (m.role === 'editor' || m.role === 'viewer')
}

function setRole(team: TeamSummary, m: MemberView, role: TeamRole) {
  return run(async () => {
    try {
      await api.cloudSetMemberRole(team.id, m.userId, role)
      if (m.isMe) await teams.syncRoles()
    } finally {
      // Shows the role the server kept, also when it refused
      await loadDetail()
    }
  })
}

// The saved passwords someone who just left could read
const exposed = ref<{ email: string; labels: string[] } | null>(null)

async function removeMember(team: TeamSummary, m: MemberView) {
  const ok = await dialogs.confirm({
    title: `Remove ${m.email}?`,
    message:
      'They lose the team’s items on their next sync, and the team key changes. Passwords saved in the team were readable to them, so change those on the servers.',
    confirmLabel: 'Remove',
    danger: true,
  })
  if (!ok) return
  await run(async () => {
    const labels = await api.cloudRemoveMember(team.id, m.userId)
    exposed.value = labels.length ? { email: m.email, labels } : null
    ui.toast('success', `Removed ${m.email}`)
    await loadDetail()
  })
}

// ---- invites to the open team ----
const inviteEmail = ref('')
const inviteRole = ref<TeamRole>('editor')
const inviteRoles = computed(() =>
  ROLES.filter((r) => r.id !== 'owner' && (r.id !== 'admin' || openTeam.value?.role === 'owner')),
)
const inviteEmailValid = computed(() => /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(inviteEmail.value.trim()))

function invite(team: TeamSummary) {
  return run(async () => {
    const email = inviteEmail.value.trim()
    await api.cloudInvite(team.id, email, inviteRole.value)
    inviteEmail.value = ''
    ui.toast('success', `Invited ${email}`)
    await loadDetail()
  })
}

function withdraw(p: PendingView) {
  return run(async () => {
    await api.cloudCancelInvite(p.id)
    await loadDetail()
  })
}

// Admitting needs the codes compared out loud first, per invite
const codeChecked = reactive<Record<string, boolean>>({})

function admit(p: PendingView) {
  if (!p.safetyCode) return
  const code = p.safetyCode
  return run(async () => {
    await api.cloudAdmit(p.id, code)
    delete codeChecked[p.id]
    ui.toast('success', `${p.email} is in the team`)
    await loadDetail()
  })
}

// ---- rename, leave, delete ----
const renaming = ref(false)
const renameDraft = ref('')

function startRename(team: TeamSummary) {
  renameDraft.value = team.name
  renaming.value = true
}

function rename(team: TeamSummary) {
  return run(async () => {
    await teams.renameTeam(team.id, renameDraft.value)
    renaming.value = false
  })
}

async function leave(team: TeamSummary) {
  const ok = await dialogs.confirm({
    title: `Leave “${team.name}”?`,
    message: 'Its items leave this computer, and you’ll need a new invite to come back.',
    confirmLabel: 'Leave team',
    danger: true,
  })
  if (ok) await run(() => teams.leaveTeam(team.id))
}

async function remove(team: TeamSummary) {
  const ok = await dialogs.confirm({
    title: `Delete “${team.name}”?`,
    message: 'Everything in it is deleted for every member. This can’t be undone.',
    confirmLabel: 'Delete team',
    danger: true,
  })
  if (ok) await run(() => teams.deleteTeam(team.id))
}

// A new state is a new form
watch(ready, () => {
  error.value = ''
  creating.value = false
  close()
})
</script>

<template>
  <div class="teams">
    <!-- ============ Needs sync first ============ -->
    <template v-if="!ready">
      <div class="intro">
        <span class="row-label">Share hosts with a team</span>
        <p class="desc">
          A team vault holds hosts, groups, credentials, snippets, notes and port forwards that
          every member gets. It’s encrypted on each member’s computer, so the server can’t read it.
        </p>
      </div>
      <p class="notice" role="status">
        Teams need sync with a master password. Set it up under Account &amp; sync first.
      </p>
      <div class="actions">
        <button class="btn btn-secondary" type="button" @click="emit('go-account')">Open Account &amp; sync</button>
      </div>
    </template>

    <p v-else-if="!teams.view && !teams.error" class="desc">Loading teams…</p>

    <template v-else-if="!teams.view">
      <p class="field-error" role="alert">{{ teams.error }}</p>
      <div class="actions">
        <button class="btn btn-secondary" type="button" :disabled="teams.loading" @click="load">Try again</button>
      </div>
    </template>

    <template v-else>
      <!-- ============ This account ============ -->
      <div class="row">
        <div class="row-text">
          <span class="row-label">Your safety code</span>
          <span class="row-desc">
            Read it out to whoever admits you to a team, so they know the invite reached you.
          </span>
          <code class="code">{{ teams.view.safetyCode }}</code>
        </div>
        <button class="btn btn-secondary" type="button" @click="copyText(teams.view.safetyCode, 'Safety code copied')">
          <Copy :size="14" :stroke-width="1.5" aria-hidden="true" />
          Copy
        </button>
      </div>

      <!-- ============ Invites for this account ============ -->
      <template v-if="teams.view.invites.length">
        <span class="group-label">Invites</span>
        <div v-for="i in teams.view.invites" :key="i.id" class="row">
          <div class="row-text">
            <span class="row-label">{{ i.teamName }}</span>
            <span v-if="i.accepted" class="row-desc">
              Accepted. Waiting for {{ i.invitedBy }} to admit you: read them your safety code.
            </span>
            <span v-else class="row-desc">
              From {{ i.invitedBy }}, as {{ roleLabel(i.role).toLowerCase() }} · until {{ dateText(i.expiresAt) }}
            </span>
          </div>
          <div class="row-actions">
            <button v-if="!i.accepted" class="btn btn-primary" type="button" :disabled="busy" @click="acceptInvite(i.id)">
              Accept
            </button>
            <button class="btn btn-ghost" type="button" :disabled="busy" @click="declineInvite(i.id)">
              {{ i.accepted ? 'Withdraw' : 'Decline' }}
            </button>
          </div>
        </div>
      </template>

      <!-- ============ Teams ============ -->
      <div class="group-head">
        <span class="group-label">Teams</span>
        <button v-if="!creating" class="btn btn-ghost" type="button" @click="creating = true">
          <Plus :size="14" :stroke-width="1.5" aria-hidden="true" />
          New team
        </button>
      </div>

      <form v-if="creating" class="form" @submit.prevent="createTeam">
        <label class="field">
          <span class="field-label">Team name</span>
          <input v-model="newName" class="field-input" maxlength="80" spellcheck="false" placeholder="Ops" />
        </label>
        <div class="actions">
          <button class="btn btn-primary" type="submit" :disabled="busy || !newName.trim()">
            {{ busy ? 'Creating…' : 'Create team' }}
          </button>
          <button class="btn btn-ghost" type="button" :disabled="busy" @click="creating = false; newName = ''">
            Cancel
          </button>
        </div>
      </form>

      <p v-if="!teams.view.teams.length && !creating" class="desc">
        No teams yet. Create one, or ask a teammate to invite {{ cloud.status.email }}.
      </p>

      <div v-for="t in teams.view.teams" :key="t.id" class="team" :class="{ open: openId === t.id }">
        <button class="team-head" type="button" :aria-expanded="openId === t.id" @click="toggle(t.id)">
          <ChevronRight :size="14" :stroke-width="1.5" class="chevron" aria-hidden="true" />
          <span class="team-name">{{ t.name }}</span>
          <span class="team-role">{{ roleLabel(t.role) }}</span>
        </button>

        <div v-if="openId === t.id" class="team-body">
          <p v-if="t.role === 'viewer'" class="desc">
            You can connect with what’s in this team, but not change it.
          </p>

          <p v-if="exposed" class="notice" role="status">
            {{ exposed.email }} could read the passwords saved on
            {{ exposed.labels.join(', ') }}. Change them on the servers.
          </p>

          <p v-if="detailError" class="field-error" role="alert">{{ detailError }}</p>
          <p v-else-if="!detail" class="desc">Loading members…</p>

          <template v-if="detail">
            <!-- Members -->
            <span class="sub-label">{{ plural(detail.members.length, 'member') }}</span>
            <ul class="list">
              <li v-for="m in detail.members" :key="m.userId" class="item">
                <span class="item-text">
                  {{ m.email }}<span v-if="m.isMe" class="me"> (you)</span>
                </span>
                <select
                  v-if="t.role === 'owner'"
                  class="role-select"
                  :value="m.role"
                  :disabled="busy"
                  :aria-label="`Role of ${m.email}`"
                  @change="setRole(t, m, ($event.target as HTMLSelectElement).value as TeamRole)"
                >
                  <option v-for="r in ROLES" :key="r.id" :value="r.id">{{ r.label }}</option>
                </select>
                <span v-else class="item-role">{{ roleLabel(m.role) }}</span>
                <button
                  v-if="canRemove(t, m)"
                  class="btn btn-ghost btn-small"
                  type="button"
                  :disabled="busy"
                  @click="removeMember(t, m)"
                >
                  Remove
                </button>
              </li>
            </ul>

            <!-- Invites to this team -->
            <template v-if="canManage(t) && detail.pending.length">
              <span class="sub-label">Invited</span>
              <ul class="list">
                <li v-for="p in detail.pending" :key="p.id" class="item pending">
                  <div class="item-line">
                    <span class="item-text">{{ p.email }}</span>
                    <span class="item-role">
                      {{ roleLabel(p.role) }} ·
                      {{ p.accepted ? 'accepted' : `until ${dateText(p.expiresAt)}` }}
                    </span>
                    <button class="btn btn-ghost btn-small" type="button" :disabled="busy" @click="withdraw(p)">
                      Withdraw
                    </button>
                  </div>
                  <div v-if="p.accepted && p.safetyCode" class="admit">
                    <p class="desc">
                      Ask {{ p.email }} to read you their safety code, from Settings → Teams. Admit
                      them only if it matches this one:
                    </p>
                    <code class="code">{{ p.safetyCode }}</code>
                    <label class="check">
                      <input v-model="codeChecked[p.id]" type="checkbox" />
                      {{ p.email }} read me the same code
                    </label>
                    <div class="actions">
                      <button
                        class="btn btn-primary"
                        type="button"
                        :disabled="busy || !codeChecked[p.id]"
                        @click="admit(p)"
                      >
                        Admit
                      </button>
                    </div>
                  </div>
                </li>
              </ul>
            </template>

            <!-- Invite someone -->
            <form v-if="canManage(t)" class="form" @submit.prevent="invite(t)">
              <label class="field">
                <span class="field-label">Invite by email</span>
                <input
                  v-model="inviteEmail"
                  class="field-input"
                  type="email"
                  spellcheck="false"
                  placeholder="teammate@example.com"
                />
                <span class="field-hint">
                  No email is sent. They sign in to LiteTerm with this address and accept under
                  Settings → Teams, then you admit them here.
                </span>
              </label>
              <div class="auth-toggle" role="radiogroup" aria-label="Role">
                <button
                  v-for="r in inviteRoles"
                  :key="r.id"
                  type="button"
                  class="auth-option"
                  :class="{ on: inviteRole === r.id }"
                  role="radio"
                  :aria-checked="inviteRole === r.id"
                  :title="r.desc"
                  @click="inviteRole = r.id"
                >
                  {{ r.label }}
                </button>
              </div>
              <span class="field-hint">{{ ROLES.find((r) => r.id === inviteRole)?.desc }}</span>
              <div class="actions">
                <button class="btn btn-primary" type="submit" :disabled="busy || !inviteEmailValid">
                  {{ busy ? 'Inviting…' : 'Invite' }}
                </button>
              </div>
            </form>
          </template>

          <!-- Rename -->
          <form v-if="renaming" class="form" @submit.prevent="rename(t)">
            <label class="field">
              <span class="field-label">Team name</span>
              <input v-model="renameDraft" class="field-input" maxlength="80" spellcheck="false" />
            </label>
            <div class="actions">
              <button class="btn btn-primary" type="submit" :disabled="busy || !renameDraft.trim()">Save</button>
              <button class="btn btn-ghost" type="button" :disabled="busy" @click="renaming = false">Cancel</button>
            </div>
          </form>

          <div class="actions team-actions">
            <button class="btn btn-ghost" type="button" :disabled="detailLoading" @click="loadDetail">
              <RefreshCw :size="14" :stroke-width="1.5" :class="{ spin: detailLoading }" aria-hidden="true" />
              Refresh
            </button>
            <button v-if="canManage(t) && !renaming" class="btn btn-ghost" type="button" :disabled="busy" @click="startRename(t)">
              Rename
            </button>
            <button class="btn btn-ghost" type="button" :disabled="busy" @click="leave(t)">Leave team</button>
            <button v-if="t.role === 'owner'" class="btn btn-danger" type="button" :disabled="busy" @click="remove(t)">
              Delete team
            </button>
          </div>
        </div>
      </div>
    </template>

    <p v-if="error" class="field-error form-error" role="alert">{{ error }}</p>
  </div>
</template>

<style scoped>
.teams {
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

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
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

.code {
  align-self: flex-start;
  margin-top: 4px;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
  background: var(--bg-input);
  font-family: var(--font-mono);
  font-size: var(--text-md);
  color: var(--text-1);
  letter-spacing: 0.04em;
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

.row + .row {
  border-top: 1px solid var(--border);
}

.row > .btn,
.row-actions {
  margin-left: auto;
  flex-shrink: 0;
}

.row-actions {
  display: flex;
  gap: var(--space-2);
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

.group-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding-top: var(--space-2);
  border-top: 1px solid var(--border);
}

.group-label {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-2);
}

/* ---- teams ---- */
.team {
  border: 1px solid var(--border);
  border-radius: var(--radius-md);
}

.team-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--radius-md);
  text-align: left;
  color: var(--text-1);
  transition: background-color var(--dur-hover) ease;
}

.team-head:hover {
  background: var(--bg-hover);
}

.chevron {
  flex-shrink: 0;
  color: var(--text-3);
  transition: transform var(--dur-hover) ease;
}

.team.open .chevron {
  transform: rotate(90deg);
}

.team-name {
  font-size: var(--text-sm);
  font-weight: 500;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.team-role {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.team-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: 4px 12px 12px;
}

.sub-label {
  font-size: var(--text-xs);
  font-weight: 500;
  color: var(--text-3);
}

.list {
  display: flex;
  flex-direction: column;
  margin: 0;
  padding: 0;
  list-style: none;
}

.item,
.item-line {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  min-height: 32px;
}

.item.pending {
  flex-direction: column;
  align-items: stretch;
  gap: var(--space-2);
}

.item + .item {
  border-top: 1px solid var(--border);
}

.item-text {
  min-width: 0;
  font-size: var(--text-sm);
  color: var(--text-1);
  overflow-wrap: anywhere;
}

.me {
  color: var(--text-3);
}

.item-role {
  margin-left: auto;
  flex-shrink: 0;
  font-size: var(--text-xs);
  color: var(--text-3);
}

.item-line .btn,
.item > .btn {
  flex-shrink: 0;
}

.role-select {
  margin-left: auto;
  flex-shrink: 0;
  padding: 3px 6px;
  border: 1px solid var(--border);
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  color: var(--text-1);
  font-size: var(--text-xs);
}

.role-select:focus-visible {
  outline: none;
  border-color: var(--accent);
}

.btn-small {
  padding: 4px 8px;
  font-size: var(--text-xs);
}

.admit {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: 10px;
  margin-bottom: var(--space-2);
  border-radius: var(--radius-md);
  background: var(--bg-surface-2);
}

.team-actions {
  padding-top: var(--space-2);
  border-top: 1px solid var(--border);
}

.form-error {
  margin: 0;
}

.spin {
  animation: spin 1s linear infinite;
}

@media (prefers-reduced-motion: reduce) {
  .spin {
    animation: none;
  }

  .chevron {
    transition: none;
  }
}
</style>
