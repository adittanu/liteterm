<script setup lang="ts">
import { computed } from 'vue'
import SearchSelect from './SearchSelect.vue'
import { useTeamsStore } from '../stores/teams'

// Where a new item goes: the personal vault or a team's. Shows only when
// there is a team to choose.
const model = defineModel<string | null>({ required: true })
defineProps<{ id: string }>()

const teams = useTeamsStore()
const shown = computed(() => teams.writable.length > 0)
</script>

<template>
  <div v-if="shown" class="field">
    <label class="field-label" :for="id">Vault</label>
    <SearchSelect :id="id" v-model="model" :options="teams.vaultOptions" aria-label="Vault" />
    <span v-if="model" class="field-hint">
      Everyone in {{ teams.teamName(model) }} gets it, saved passwords included.
    </span>
  </div>
</template>
