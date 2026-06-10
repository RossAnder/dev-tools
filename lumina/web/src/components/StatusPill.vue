<script setup vapor lang="ts">
import { computed } from 'vue'
import { statusLabel, statusToken } from '@/composables/useDisplay'

const props = defineProps<{ status: string }>()

const label = computed(() => statusLabel(props.status))
const tokenClass = computed(() => statusToken(props.status))
</script>

<template>
  <span
    :class="['inline-flex items-center px-2 py-0.5 rounded-md border border-[var(--border)] bg-[var(--surface-2)] font-mono text-[10.5px] tracking-wider', tokenClass]"
  >
    {{ label }}
  </span>
</template>
