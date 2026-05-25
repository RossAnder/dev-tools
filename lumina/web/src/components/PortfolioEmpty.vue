<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import type { WorkItem, WorkItemNode } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import ChildGrid from '@/components/ChildGrid.vue'

const { tree, descendantCounts, treeStatus, error } = useHierarchy()

/**
 * Walk every node in the tree (roots + all descendants) and count matches.
 * Used by the KPI sub-text lines that need portfolio-wide rollups not
 * exposed by `descendantCounts` (which only counts by kind).
 */
function countWhere(predicate: (node: WorkItem) => boolean): number {
  let n = 0
  const visit = (node: WorkItemNode): void => {
    if (predicate(node)) n += 1
    for (const child of node.children) visit(child)
  }
  for (const root of tree.value) visit(root)
  return n
}

const rootsInFlight: ComputedRef<number> = computed(
  () => tree.value.filter((n) => n.status === 'in_progress').length,
)

const blockedStories: ComputedRef<number> = computed(() =>
  countWhere((n) => n.kind === 'story' && n.status === 'blocked'),
)

const executingTasks: ComputedRef<number> = computed(() =>
  countWhere((n) => n.kind === 'task' && n.status === 'in_progress'),
)
</script>

<template>
  <div v-if="treeStatus === 'loading'" class="mx-4 my-3">
    <div class="h-64 bg-[var(--surface-2)] rounded-xl animate-pulse-dot"></div>
  </div>

  <div
    v-else-if="treeStatus === 'error'"
    class="mx-4 my-3 p-4 border border-[var(--border)] rounded-xl text-blocked text-[13px] font-mono"
  >
    {{ error }}
  </div>

  <div
    v-else-if="treeStatus === 'empty'"
    class="mx-4 my-3 p-8 border border-[var(--border)] rounded-xl text-[var(--faint)] text-[14px] font-mono italic"
  >
    No work items yet — create your first epic via the MCP <code class="text-accent">create_work_item</code> tool.
  </div>

  <template v-else>
    <article
      class="relative bg-[var(--surface)] border border-[var(--border)] rounded-xl p-8 mx-4 my-3"
    >
      <span aria-hidden="true" class="absolute top-2 left-2 w-4 h-4 border-t-2 border-l-2 border-[var(--accent)]"></span>
      <span aria-hidden="true" class="absolute top-2 right-2 w-4 h-4 border-t-2 border-r-2 border-[var(--accent)]"></span>
      <span aria-hidden="true" class="absolute bottom-2 left-2 w-4 h-4 border-b-2 border-l-2 border-[var(--accent)]"></span>
      <span aria-hidden="true" class="absolute bottom-2 right-2 w-4 h-4 border-b-2 border-r-2 border-[var(--accent)]"></span>

      <header class="mb-6">
        <div class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase mb-3">
          PORTFOLIO · LUMINA / ALL
        </div>
        <h1 class="font-display italic text-[46px] leading-[1.05] text-[var(--ink)] mb-3">
          Plan. Dispatch. Observe.
        </h1>
        <p class="text-[var(--ink-2)] text-[14px] leading-[1.55] max-w-prose">
          This is the control surface for the agentic harness. Build out epics and features as the durable structure; let sprints and tasks come and go through them. Drill into any node on the left to focus the lens.
        </p>
      </header>

      <div class="grid grid-cols-4 gap-6 py-5 border-y border-[var(--border)]">
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ tree.length }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Epics</div>
          <div class="font-mono text-[10px] text-[var(--muted)]">{{ rootsInFlight }} IN FLIGHT</div>
        </div>
        <div class="flex flex-col gap-1">
          <div class="font-mono text-[16px] text-[var(--ink)]">{{ descendantCounts.features }}</div>
          <div class="font-mono text-[10.5px] tracking-[0.16em] text-[var(--faint)] uppercase">Features</div>
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

    <ChildGrid :children="tree" child-kind-label="EPICS" />
  </template>
</template>
