<script setup vapor lang="ts">
import { computed } from 'vue'
import type { WorkItem } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import { kindLabel, effortLabel } from '@/composables/useDisplay'
import StatusPill from '@/components/StatusPill.vue'

const props = defineProps<{ node: WorkItem; childCount: number }>()

const { setFocus } = useHierarchy()

const effortBadge = computed(() => effortLabel(props.node.effort))
</script>

<template>
  <button
    type="button"
    @click="setFocus(node.id)"
    class="text-left bg-[var(--surface)] border border-[var(--border)] rounded-lg p-4 cursor-pointer flex flex-col gap-3 transition-all duration-150 hover:border-[var(--border-strong)] hover:bg-[var(--surface-2)] hover:-translate-y-px"
  >
    <!-- top row: kindLabel · id -->
    <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">
      {{ kindLabel(node.kind) }} · {{ node.id }}
    </div>
    <!-- title -->
    <h3 class="text-[15.5px] font-medium text-[var(--ink-2)] leading-[1.3]">
      {{ node.title }}
    </h3>
    <!-- summary (clamped to 2 lines) -->
    <p
      v-if="node.body"
      class="text-[13px] text-[var(--muted)] leading-[1.5] line-clamp-2"
    >
      {{ node.body }}
    </p>
    <!-- bottom row: StatusPill + (task) effort + (non-task) child count -->
    <div class="flex items-center justify-between gap-3 mt-2">
      <StatusPill v-if="node.status" :status="node.status" />
      <div class="flex items-center gap-2">
        <span
          v-if="node.kind === 'task' && effortBadge"
          class="font-mono text-[10.5px] px-2 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface-2)] text-[var(--ink-2)]"
        >
          {{ effortBadge }}
        </span>
        <span
          v-if="node.kind !== 'task' && childCount > 0"
          class="font-mono text-[10.5px] px-2 py-0.5 border border-[var(--border)] rounded-md bg-[var(--surface-2)] text-[var(--faint)]"
        >
          {{ childCount }}
        </span>
      </div>
    </div>
  </button>
</template>
