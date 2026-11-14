<script setup lang="ts">
// Text that may hold host names, addresses, logins or group names. With
// privacy mode on, those parts render as stand-ins (dummy style) or blurred
// in place (blur style); everything else in the text stays as it is.
import { computed } from 'vue'
import { usePrivacyStore } from '../stores/privacy'

const props = defineProps<{ text: string | null | undefined }>()
const pv = usePrivacyStore()

const parts = computed(() => pv.segments(props.text))
</script>

<template><template v-for="(p, i) in parts" :key="i"><span v-if="p.secret && pv.blur" class="pv">{{ p.text }}</span><template v-else>{{ p.text }}</template></template></template>
