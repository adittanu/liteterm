<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import OsAvatar from './OsAvatar.vue'
import type { Host } from '../types'

const props = withDefaults(
  defineProps<{
    host: Host
    size?: number
  }>(),
  { size: 40 },
)

// Seed from the host id: each host keeps its own planet across launches
// instead of re-rolling on every render
const seed = computed(() => String(props.host.id))
const failed = ref(false)

watch(
  () => props.host.id,
  () => {
    failed.value = false
  },
)
</script>

<template>
  <span
    v-if="!failed"
    class="planet-avatar"
    :style="{ width: `${size}px`, height: `${size}px` }"
  >
    <img
      :src="`https://api.dicebear.com/10.x/planets/svg?tags=animation&animationVariant=fastest:1&seed=${seed}`"
      alt=""
      loading="lazy"
      draggable="false"
      @error="failed = true"
    />
  </span>
  <OsAvatar v-else :label="host.label" :os="host.os" :size="size" />
</template>

<style scoped>
.planet-avatar {
  display: flex;
  border-radius: var(--radius-md);
  overflow: hidden;
  background: var(--bg-surface-2);
  border: 1px solid var(--border);
  flex-shrink: 0;
}

.planet-avatar img {
  width: 100%;
  height: 100%;
  display: block;
  object-fit: cover;
}
</style>
