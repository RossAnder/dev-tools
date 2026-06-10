<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import ChildGrid from '@/components/ChildGrid.vue'

const { tree, descendantCounts, treeStatus, error, countMatching } = useHierarchy()

const epicCount: ComputedRef<number> = computed(
  () => tree.value.filter((n) => n.kind === 'epic').length,
)

const rootsInFlight: ComputedRef<number> = computed(
  () => tree.value.filter((n) => n.status === 'in_progress').length,
)

const blockedStories: ComputedRef<number> = computed(() =>
  countMatching((n) => n.kind === 'story' && n.status === 'blocked'),
)

const executingTasks: ComputedRef<number> = computed(() =>
  countMatching((n) => n.kind === 'task' && n.status === 'in_progress'),
)
</script>

<template>
  <div v-if="treeStatus === 'loading'" class="mx-4 my-3">
    <div class="h-64 bg-[var(--surface-2)] animate-pulse-dot"></div>
  </div>

  <div
    v-else-if="treeStatus === 'error'"
    class="mx-4 my-3 p-4 border border-[var(--border)] text-blocked text-[13px] font-mono"
  >
    {{ error }}
  </div>

  <div
    v-else-if="treeStatus === 'empty'"
    class="mx-4 my-3 p-8 border border-[var(--border)] text-[var(--faint)] text-[14px] font-mono italic"
  >
    No work items yet — create your first epic via the MCP <code class="text-accent">create_work_item</code> tool.
  </div>

  <template v-else>
    <article
      class="relative bg-[var(--surface)] border border-[var(--border)] p-8 mx-4 my-3"
    >
      <span aria-hidden="true" class="absolute -top-px -left-px w-4 h-4 border-t border-l border-[var(--accent)]"></span>
      <span aria-hidden="true" class="absolute -bottom-px -right-px w-4 h-4 border-b border-r border-[var(--accent)]"></span>

      <header class="mb-6">
        <div class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3">
          PORTFOLIO · LUMINA / ALL
        </div>
        <h1 class="font-display italic text-[46px] leading-[1.05] text-[var(--ink)] mb-3">
          Plan. Dispatch. Observe.
        </h1>
        <p class="text-[var(--ink-2)] text-[14px] leading-[1.55] max-w-prose">
          This is the control surface for the agentic harness. Build out epics and focuses as the durable structure; let sprints and tasks come and go through them. Drill into any node on the left to focus the lens.
        </p>
      </header>

      <div class="grid grid-cols-4 gap-6 py-5 border-y border-[var(--border)]">
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ epicCount }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Projects</div>
          <div class="font-mono text-[10px] text-[var(--muted)]">{{ rootsInFlight }} IN FLIGHT</div>
        </div>
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ descendantCounts.focuses }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Focuses</div>
          <div class="font-mono text-[10px] text-[var(--muted)]">ACROSS PORTFOLIO</div>
        </div>
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ descendantCounts.stories }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Stories</div>
          <div class="font-mono text-[10px] text-[var(--muted)]">{{ blockedStories }} BLOCKED</div>
        </div>
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ descendantCounts.tasks }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Tasks</div>
          <div class="font-mono text-[10px] text-[var(--muted)]">{{ executingTasks }} EXECUTING</div>
        </div>
      </div>
    </article>

    <ChildGrid :children="tree" />
  </template>
</template>
