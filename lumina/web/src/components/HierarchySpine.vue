<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import type { WorkItem } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import SpineNode from './SpineNode.vue'

const SAVED_VIEW_PLACEHOLDERS = [
  '▸ in-flight only',
  '▸ blocked items',
  '▸ unassigned tasks',
  '▸ this sprint',
  '+ new view',
] as const

const { tree, focusId, focusPath, treeStatus, error, getSiblings } = useHierarchy()

interface SpineEntry {
  node: WorkItem
  isFocused: boolean
  isAncestor: boolean
}

const spineList: ComputedRef<SpineEntry[]> = computed(() => {
  // 'ready' or 'stale-error' both have a usable tree; only loading/error/empty
  // suppress rendering.
  if (treeStatus.value !== 'ready' && treeStatus.value !== 'stale-error') return []

  if (focusId.value === null) {
    return tree.value.map((node) => ({
      node,
      isFocused: false,
      isAncestor: false,
    }))
  }

  const path = focusPath.value
  if (path.length === 0) return []

  const focused = path[path.length - 1]!
  const ancestors = path.slice(0, -1).map<SpineEntry>((node) => ({
    node,
    isFocused: false,
    isAncestor: true,
  }))

  const siblings: SpineEntry[] = getSiblings(focused.id)
    .filter((s) => s.id !== focused.id)
    .map((s) => ({ node: s, isFocused: false, isAncestor: false }))

  return [
    ...ancestors,
    { node: focused, isFocused: true, isAncestor: false },
    ...siblings,
  ]
})
</script>

<template>
  <aside
    class="h-full overflow-y-auto bg-[var(--surface)] border-r border-[var(--border)] px-4 py-3 flex flex-col gap-6"
  >
    <!-- Section 01 — PLANNING GRAPH -->
    <section class="flex flex-col">
      <h2
        class="font-mono text-[10.5px] tracking-wider text-[var(--faint)] sticky top-0 bg-[var(--surface)] py-2 z-10"
      >
        [01 / PLANNING GRAPH]
      </h2>

      <template v-if="treeStatus === 'loading'">
        <div
          v-for="i in 4"
          :key="i"
          class="h-6 bg-[var(--surface-2)] rounded-md animate-pulse-dot mb-2"
        ></div>
      </template>

      <div
        v-else-if="treeStatus === 'error'"
        class="text-blocked text-[12px] font-mono p-2 border border-[var(--border)] rounded-md"
      >
        {{ error }}
      </div>

      <div
        v-else-if="treeStatus === 'empty'"
        class="text-[var(--muted)] text-[12px] font-mono"
      >
        No work items yet
      </div>

      <div
        v-if="treeStatus === 'stale-error'"
        class="text-blocked text-[11px] font-mono px-2 py-1"
      >
        Stale data — last refresh failed: {{ error }}
      </div>

      <ul
        v-if="treeStatus === 'ready' || treeStatus === 'stale-error'"
        class="flex flex-col gap-1 relative"
      >
        <!-- continuous vertical spine line behind all nodes -->
        <span
          aria-hidden="true"
          class="pointer-events-none absolute left-3 top-0 bottom-0 w-px spine-rail"
          style="transform: translateX(-50%);"
        ></span>
        <li v-for="entry in spineList" :key="entry.node.id">
          <SpineNode
            :node="entry.node"
            :is-focused="entry.isFocused"
            :is-ancestor="entry.isAncestor"
          />
        </li>
      </ul>
    </section>

    <!-- Section 02 — SAVED VIEWS -->
    <section class="flex flex-col">
      <h2
        class="font-mono text-[10.5px] tracking-wider text-[var(--faint)] py-2"
      >
        [02 / SAVED VIEWS]
      </h2>
      <!-- deferred: saved views backend -->
      <ul class="flex flex-col">
        <li v-for="label in SAVED_VIEW_PLACEHOLDERS" :key="label">
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            {{ label }}
          </button>
        </li>
      </ul>
    </section>
  </aside>
</template>

<style scoped>
.spine-rail {
  background: linear-gradient(to bottom, transparent 0%, var(--border-strong) 8%, var(--border-strong) 92%, transparent 100%);
}
</style>
