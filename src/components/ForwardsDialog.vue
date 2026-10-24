<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { Plus, Play, Square, Pencil, Trash2, ArrowLeftRight } from 'lucide-vue-next'
import BaseModal from './BaseModal.vue'
import { useDialogsStore } from '../stores/dialogs'
import { useForwardsStore } from '../stores/extras'
import { useHostsStore } from '../stores/hosts'
import { useUiStore } from '../stores/ui'
import { errorText } from '../stores/sessions'

const dialogs = useDialogsStore()
const forwards = useForwardsStore()
const hosts = useHostsStore()
const ui = useUiStore()

const editingId = ref<number | null>(null)
const form = ref({
  name: '',
  host_id: 0,
  local_port: '',
  remote_host: '',
  remote_port: '',
  auto_start: false,
})

function startCreate() {
  editingId.value = null
  form.value = {
    name: '',
    host_id: hosts.hosts[0]?.id ?? 0,
    local_port: '',
    remote_host: 'localhost',
    remote_port: '',
    auto_start: false,
  }
}

function startEdit(id: number) {
  const f = forwards.forwards.find((x) => x.id === id)
  if (!f) return
  editingId.value = id
  form.value = {
    name: f.name,
    host_id: f.host_id,
    local_port: String(f.local_port),
    remote_host: f.remote_host,
    remote_port: String(f.remote_port),
    auto_start: f.auto_start,
  }
}

function valid(): boolean {
  const lp = Number(form.value.local_port)
  const rp = Number(form.value.remote_port)
  return (
    form.value.name.trim().length > 0 &&
    form.value.host_id > 0 &&
    Number.isInteger(lp) && lp >= 1 && lp <= 65535 &&
    form.value.remote_host.trim().length > 0 &&
    Number.isInteger(rp) && rp >= 1 && rp <= 65535
  )
}

async function save() {
  if (!valid()) return
  try {
    await forwards.save({
      id: editingId.value ?? undefined,
      host_id: form.value.host_id,
      name: form.value.name.trim(),
      local_port: Number(form.value.local_port),
      remote_host: form.value.remote_host.trim(),
      remote_port: Number(form.value.remote_port),
      auto_start: form.value.auto_start,
    })
    startCreate()
  } catch (e) {
    ui.toast('error', `Save failed: ${errorText(e)}`)
  }
}

async function remove(id: number) {
  const f = forwards.forwards.find((x) => x.id === id)
  const ok = await dialogs.confirm({
    title: `Delete forward “${f?.name ?? id}”?`,
    message: 'This cannot be undone.',
    confirmLabel: 'Delete',
    danger: true,
  })
  if (!ok) return
  await forwards.remove(id)
}

function hostLabel(id: number): string {
  return hosts.hostById(id)?.label ?? `host #${id}`
}

function onToggle(id: number, active: boolean) {
  void (active ? forwards.start(id) : forwards.stop(id)).catch((e) =>
    ui.toast('error', errorText(e)),
  )
}

watch(
  () => dialogs.showForwards,
  (o) => {
    if (o) startCreate()
  },
)

onMounted(startCreate)
</script>

<template>
  <BaseModal
    v-if="dialogs.showForwards"
    title="Port Forwards"
    :width="600"
    @close="dialogs.showForwards = false"
  >
    <div class="fwd">
      <template v-if="hosts.hosts.length">
        <!-- Editor -->
        <form class="fwd-editor" @submit.prevent="save">
          <div class="fwd-grid">
            <div class="field">
              <label class="field-label" for="fw-name">Name</label>
              <input id="fw-name" v-model="form.name" class="field-input" type="text" placeholder="db tunnel" spellcheck="false" />
            </div>
            <div class="field">
              <label class="field-label" for="fw-host">Via host</label>
              <select id="fw-host" v-model="form.host_id" class="field-input">
                <option v-for="h in hosts.hosts" :key="h.id" :value="h.id">{{ h.label }}</option>
              </select>
            </div>
          </div>
          <div class="fwd-grid3">
            <div class="field">
              <label class="field-label" for="fw-lport">Local port</label>
              <input id="fw-lport" v-model="form.local_port" class="field-input mono" type="text" inputmode="numeric" placeholder="5432" />
            </div>
            <div class="field">
              <label class="field-label" for="fw-rhost">Remote host</label>
              <input id="fw-rhost" v-model="form.remote_host" class="field-input mono" type="text" placeholder="localhost" spellcheck="false" />
            </div>
            <div class="field">
              <label class="field-label" for="fw-rport">Remote port</label>
              <input id="fw-rport" v-model="form.remote_port" class="field-input mono" type="text" inputmode="numeric" placeholder="5432" />
            </div>
          </div>
          <div class="fwd-editor-foot">
            <label class="check">
              <input v-model="form.auto_start" type="checkbox" />
              Start automatically when the host connects
            </label>
            <button type="submit" class="btn btn-primary" :disabled="!valid()">
              <Plus :size="14" :stroke-width="2" aria-hidden="true" />
              {{ editingId !== null ? 'Update' : 'Add forward' }}
            </button>
          </div>
        </form>

        <div class="divider"></div>

        <!-- List -->
        <div v-if="!forwards.forwards.length" class="fwd-empty">
          <ArrowLeftRight :size="20" :stroke-width="1.5" aria-hidden="true" />
          <p>No forwards yet. Add one above to tunnel a local port through SSH.</p>
        </div>

        <ul v-else class="fwd-list">
          <li v-for="f in forwards.forwards" :key="f.id" class="fwd-item">
            <span
              class="status-dot"
              :class="forwards.activeIds.includes(f.id) ? 'connected' : 'idle'"
              :title="forwards.activeIds.includes(f.id) ? 'Active' : 'Stopped'"
            ></span>
            <div class="fwd-info">
              <span class="fwd-name">{{ f.name }}</span>
              <code class="fwd-detail mono">
                localhost:{{ f.local_port }} → {{ hostLabel(f.host_id) }} → {{ f.remote_host }}:{{ f.remote_port }}
              </code>
            </div>
            <div class="fwd-actions">
              <button
                class="btn-icon"
                :aria-label="forwards.activeIds.includes(f.id) ? `Stop ${f.name}` : `Start ${f.name}`"
                :title="forwards.activeIds.includes(f.id) ? 'Stop' : 'Start'"
                @click="onToggle(f.id, !forwards.activeIds.includes(f.id))"
              >
                <Square v-if="forwards.activeIds.includes(f.id)" :size="14" :stroke-width="1.5" aria-hidden="true" />
                <Play v-else :size="14" :stroke-width="1.5" aria-hidden="true" />
              </button>
              <button class="btn-icon" :aria-label="`Edit ${f.name}`" title="Edit" @click="startEdit(f.id)">
                <Pencil :size="13" :stroke-width="1.5" aria-hidden="true" />
              </button>
              <button class="btn-icon fwd-delete" :aria-label="`Delete ${f.name}`" title="Delete" @click="remove(f.id)">
                <Trash2 :size="13" :stroke-width="1.5" aria-hidden="true" />
              </button>
            </div>
          </li>
        </ul>

        <p class="fwd-hint">
          A forward needs an SSH connection to its host while active.
          Connect once from the sidebar, then start the tunnel here.
        </p>
      </template>

      <div v-else class="fwd-empty">
        <ArrowLeftRight :size="20" :stroke-width="1.5" aria-hidden="true" />
        <p>Add a host first — forwards tunnel through an SSH connection.</p>
      </div>
    </div>
  </BaseModal>
</template>

<style scoped>
.fwd {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.fwd-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-3);
}

.fwd-grid3 {
  display: grid;
  grid-template-columns: 100px 1fr 100px;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.fwd-editor-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-3);
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

.fwd-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-5);
  color: var(--text-4);
  font-size: var(--text-sm);
  text-align: center;
}

.fwd-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 240px;
  overflow-y: auto;
}

.fwd-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-md);
  transition: background-color var(--dur-hover) ease;
}

.fwd-item:hover {
  background: var(--bg-hover);
}

.fwd-info {
  display: flex;
  flex-direction: column;
  gap: 1px;
  flex: 1;
  min-width: 0;
}

.fwd-name {
  font-size: var(--text-base);
  font-weight: 500;
  color: var(--text-1);
}

.fwd-detail {
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  color: var(--text-3);
}

.fwd-actions {
  display: flex;
  gap: 2px;
  opacity: 0;
  transition: opacity var(--dur-hover) ease;
}

.fwd-item:hover .fwd-actions,
.fwd-item:focus-within .fwd-actions {
  opacity: 1;
}

.fwd-delete:hover {
  color: var(--danger);
}

.fwd-hint {
  font-size: var(--text-xs);
  color: var(--text-4);
  line-height: var(--leading-normal);
}
</style>
