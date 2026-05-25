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
    @click="setFocus(node.id)"
  >
    <!-- vertical gradient spine line (mirrors styles.css .spine-rail) -->
    <span
      aria-hidden="true"
      class="absolute left-3 top-0 bottom-0 w-px"
      style="background: linear-gradient(to bottom, transparent 0%, var(--border-strong) 20%, var(--border-strong) 80%, transparent 100%);"
    ></span>
    <!-- diamond marker -->
    <span
      aria-hidden="true"
      class="absolute left-[9px] top-1/2 -translate-y-1/2 w-[6px] h-[6px] rotate-45 border z-10"
      :class="
        isFocused
          ? 'border-[var(--accent)] bg-[var(--accent)]'
          : isAncestor
            ? 'border-[var(--color-accent-deep)] bg-[var(--bg)]'
            : 'border-[var(--border-strong)] bg-[var(--bg)]'
      "
      :style="isFocused ? 'box-shadow: 0 0 0 2px var(--bg), 0 0 0 3px var(--accent), 0 0 10px var(--color-accent-glow);' : ''"
    ></span>
    <span class="flex flex-col flex-1 min-w-0 gap-[2px]">
      <span
        class="font-mono text-[9.5px] tracking-[0.18em] uppercase"
        :class="isFocused ? 'text-accent' : 'text-[var(--faint)]'"
      >
        {{ kindLabel(node.kind) }} · {{ node.id }}
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
