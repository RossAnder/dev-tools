<script setup vapor lang="ts">
import { computed } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel } from '@/composables/useDisplay'

// Shared breadcrumb chain used by both the centre-column <Breadcrumbs> nav
// and the footer's left-column status strip. Both consumers iterate the same
// focusPath, call setFocus(node.id), and render kindLabel chips separated by
// '/'. Only the visual tone differs, so we expose a single `tone` prop and
// keep DOM structure identical across consumers.
//
// The outer wrapper (e.g. <nav aria-label="Breadcrumbs"> for the centre
// column, or the footer's grid-cell <div>) stays with the consumer — this
// component renders only the chip sequence so it composes cleanly into
// either context without an unnecessary nested nav.

const props = withDefaults(
  defineProps<{
    tone?: 'muted' | 'faint'
  }>(),
  { tone: 'muted' },
)

const { focusPath, setFocus } = useHierarchy()

// Tone drives colour, casing, and separator styling. The 'muted' tone is the
// uppercase tracked treatment used by the prominent centre-column nav; the
// 'faint' tone is the lower-contrast lowercase strip used in the footer.
const buttonClass = computed(() =>
  props.tone === 'faint'
    ? 'bg-transparent border-none p-0 cursor-pointer text-[var(--faint)] hover:text-[var(--ink-2)]'
    : 'bg-transparent border-none p-0 cursor-pointer text-[var(--muted)] hover:text-[var(--ink-2)] uppercase',
)

const separatorClass = computed(() =>
  props.tone === 'faint' ? 'text-[var(--faint)]' : 'text-[var(--ghost)]',
)
</script>

<template>
  <template v-for="(node, idx) in focusPath" :key="node.id">
    <button
      type="button"
      :class="buttonClass"
      :aria-label="`${kindLabel(node.kind)}: ${node.title}`"
      :aria-current="idx === focusPath.length - 1 ? 'page' : undefined"
      @click="setFocus(node.id)"
    >
      {{ kindLabel(node.kind) }}
    </button>
    <span
      v-if="idx < focusPath.length - 1"
      :class="separatorClass"
      aria-hidden="true"
    >
      /
    </span>
  </template>
</template>
