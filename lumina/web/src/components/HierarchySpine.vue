<script setup vapor lang="ts">
import { computed, type ComputedRef } from 'vue'
import type { WorkItem, WorkItemNode } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import SpineNode from './SpineNode.vue'

const { tree, focusId, focusPath, treeStatus, error } = useHierarchy()

interface SpineEntry {
  node: WorkItem
  isFocused: boolean
  isAncestor: boolean
}

/**
 * Recursively locate `id` in the tree and return both the node and its parent's
 * children array (the sibling cohort). For root-level nodes the parent's
 * children is `tree.value` itself, so we seed the walk with a synthetic root.
 */
function findNodeWithSiblings(
  roots: WorkItemNode[],
  id: string,
): { node: WorkItemNode; siblings: WorkItemNode[] } | null {
  // Check roots-as-siblings first.
  for (const r of roots) {
    if (r.id === id) return { node: r, siblings: roots }
  }
  // Recurse into each subtree, treating each node's `children` as the sibling
  // pool for the level below.
  const visit = (parentChildren: WorkItemNode[]): { node: WorkItemNode; siblings: WorkItemNode[] } | null => {
    for (const candidate of parentChildren) {
      for (const child of candidate.children) {
        if (child.id === id) return { node: child, siblings: candidate.children }
      }
      const deeper = visit(candidate.children)
      if (deeper !== null) return deeper
    }
    return null
  }
  return visit(roots)
}

const spineList: ComputedRef<SpineEntry[]> = computed(() => {
  if (treeStatus.value !== 'ready') return []

  if (focusId.value === null) {
    return tree.value.map((node) => ({
      node,
      isFocused: false,
      isAncestor: false,
    }))
  }

  const path = focusPath.value
  if (path.length === 0) return []

  const focused = path[path.length - 1]
  const ancestors = path.slice(0, -1).map<SpineEntry>((node) => ({
    node,
    isFocused: false,
    isAncestor: true,
  }))

  const found = findNodeWithSiblings(tree.value, focused.id)
  if (found === null) {
    // Fallback: focused node from focusPath only.
    return [
      ...ancestors,
      { node: focused, isFocused: true, isAncestor: false },
    ]
  }

  const siblings: SpineEntry[] = found.siblings
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

      <ul v-else class="flex flex-col gap-1 relative">
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
        <li>
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            ▸ in-flight only
          </button>
        </li>
        <li>
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            ▸ blocked items
          </button>
        </li>
        <li>
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            ▸ unassigned tasks
          </button>
        </li>
        <li>
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            ▸ this sprint
          </button>
        </li>
        <li>
          <button
            disabled
            aria-disabled="true"
            class="block w-full text-left font-mono text-[11px] text-[var(--ghost)] cursor-not-allowed py-1 bg-transparent border-none"
          >
            + new view
          </button>
        </li>
      </ul>
    </section>
  </aside>
</template>
