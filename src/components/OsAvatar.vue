<script setup lang="ts">
import { computed } from 'vue'
import {
  siUbuntu,
  siDebian,
  siFedora,
  siCentos,
  siAlpinelinux,
  siArchlinux,
  siRedhat,
  siOpensuse,
  siApple,
  siLinux,
} from 'simple-icons'

const props = withDefaults(
  defineProps<{
    label: string
    os?: string | null
    size?: number
  }>(),
  { size: 40 },
)

const ICONS: Record<string, { path: string }> = {
  ubuntu: siUbuntu,
  debian: siDebian,
  fedora: siFedora,
  centos: siCentos,
  alpine: siAlpinelinux,
  arch: siArchlinux,
  redhat: siRedhat,
  rhel: siRedhat,
  rocky: siRedhat,
  opensuse: siOpensuse,
  suse: siOpensuse,
  macos: siApple,
  darwin: siApple,
  linux: siLinux,
  debian_linux: siDebian,
}

const AVATAR_HUES = 8

const icon = computed(() => {
  if (!props.os) return null
  const key = props.os.toLowerCase().replace(/\s+/g, '')
  return ICONS[key] ?? (props.os === 'unknown' ? null : siLinux)
})

function initials(label: string): string {
  const words = label.trim().split(/[\s._-]+/).filter(Boolean)
  if (!words.length) return '?'
  if (words.length === 1) return words[0].slice(0, 2).toUpperCase()
  return (words[0][0] + words[1][0]).toUpperCase()
}

function avatarClass(label: string): string {
  let hash = 0
  for (let i = 0; i < label.length; i++) hash = (hash * 31 + label.charCodeAt(i)) >>> 0
  return `avatar-h${hash % AVATAR_HUES}`
}
</script>

<template>
  <span
    v-if="icon"
    class="os-avatar"
    :style="{ width: `${size}px`, height: `${size}px` }"
    :title="os || undefined"
  >
    <svg viewBox="0 0 24 24" :width="Math.round(size * 0.58)" :height="Math.round(size * 0.58)" aria-hidden="true">
      <path :d="icon.path" fill="currentColor" />
    </svg>
  </span>
  <span
    v-else
    class="os-avatar initials"
    :class="avatarClass(label)"
    :style="{ width: `${size}px`, height: `${size}px`, fontSize: `${Math.round(size * 0.34)}px` }"
    :title="os ?? label"
  >
    {{ initials(label) }}
  </span>
</template>

<style scoped>
.os-avatar {
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  flex-shrink: 0;
}

.os-avatar svg {
  color: var(--text-2);
}

.os-avatar.initials {
  font-weight: 700;
  color: white;
  letter-spacing: 0.02em;
}

.avatar-h0 { background: #4a7ae8; }
.avatar-h1 { background: #2a9d6e; }
.avatar-h2 { background: #c2703d; }
.avatar-h3 { background: #b0568f; }
.avatar-h4 { background: #5c8ecf; }
.avatar-h5 { background: #8a63c9; }
.avatar-h6 { background: #3f97a8; }
.avatar-h7 { background: #a85e56; }
</style>
