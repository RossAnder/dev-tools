<script setup vapor lang="ts">
import type { WorkItem } from '@/api'
import { kindLabel } from '@/composables/useDisplay'
import { useHierarchy } from '@/composables/useHierarchy'
import StatusPill from '@/components/StatusPill.vue'

defineProps<{
  node: WorkItem
  isFocused: boolean
  isAncestor: boolean
}>()

const { setFocus } = useHierarchy()
</script>

<template>
  <button
    type="button"
    class="relative flex items-center gap-3 w-full text-left bg-transparent border-none pl-8 pr-2 py-2 cursor-pointer group transition-opacity duration-150"
    :class="{
      'text-[var(--ink)] font-semibold': isFocused,
      'text-[var(--ink-2)]': isAncestor && !isFocused,
      'text-[var(--muted)] opacity-60 hover:opacity-95': !isFocused && !isAncestor,
    }"
    :aria-label="node.title"
    data-testid="spine-node"
    @click="setFocus(node.id)"
  >
    <!-- diamond marker -->
    <span
      aria-hidden="true"
      class="absolute left-3 top-1/2 w-[6px] h-[6px] border z-10"
      :class="
        isFocused
          ? 'border-[var(--accent)] bg-[var(--accent)]'
          : isAncestor
            ? 'border-[var(--color-accent-deep)] bg-[var(--bg)]'
            : 'border-[var(--border-strong)] bg-[var(--bg)]'
      "
      :style="
        (isFocused ? 'box-shadow: 0 0 0 2px var(--bg), 0 0 0 3px var(--accent), 0 0 10px var(--color-accent-glow); ' : '')
        + 'transform: translate(-50%, -50%) rotate(45deg);'
      "
    ></span>
    <span class="flex flex-col flex-1 min-w-0 gap-[2px]">
      <span
        class="font-mono text-[9.5px] tracking-[0.18em] uppercase"
        :class="isFocused ? 'text-accent' : 'text-[var(--faint)]'"
      >
        {{ kindLabel(node.kind) }}
      </span>
      <span
        class="font-sans text-[13px] leading-[1.3] truncate"
        :class="{
          'text-[var(--ink)] font-semibold text-[13.5px]': isFocused,
          'text-[var(--ink-2)] font-medium': !isFocused,
        }"
      >
        {{ node.title }}
      </span>
    </span>
    <StatusPill v-if="node.status" :status="node.status" />
  </button>
</template>

