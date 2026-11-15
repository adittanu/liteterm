<script setup lang="ts">
import { computed } from 'vue'
import { Users } from 'lucide-vue-next'
import { useTeamsStore } from '../stores/teams'

// Marks an item that lives in a team vault; personal items show nothing
const props = defineProps<{ teamId: string | null }>()

const teams = useTeamsStore()
const readOnly = computed(() => props.teamId !== null && !teams.canEdit(props.teamId))
const title = computed(() =>
  readOnly.value ? `In ${teams.teamName(props.teamId)}. You can view it, not change it.` : `In ${teams.teamName(props.teamId)}`,
)
</script>

<template>
  <span v-if="teamId" class="vault-chip" :title="title">
    <Users :size="11" :stroke-width="1.75" aria-hidden="true" />
    <span class="vault-name">{{ teams.teamName(teamId) }}</span>
    <span v-if="readOnly" class="vault-ro">view only</span>
  </span>
</template>

<style scoped>
.vault-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  max-width: 100%;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--accent-bg);
  color: var(--text-2);
  font-size: var(--text-xs);
  font-weight: 500;
  line-height: 1.5;
  white-space: nowrap;
  vertical-align: middle;
}

.vault-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.vault-ro {
  color: var(--text-3);
  font-weight: 400;
}

.vault-ro::before {
  content: '· ';
}
</style>
