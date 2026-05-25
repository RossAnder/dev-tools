<script setup vapor lang="ts">
import { onMounted, computed, type ComputedRef } from 'vue'
import { useHierarchy } from '@/composables/useHierarchy'
import AppHeader from '@/components/AppHeader.vue'
import AppFooter from '@/components/AppFooter.vue'
import HierarchySpine from '@/components/HierarchySpine.vue'
import CenterToolbar from '@/components/CenterToolbar.vue'
import Breadcrumbs from '@/components/Breadcrumbs.vue'
import FocusLens from '@/components/FocusLens.vue'
import ChildGrid from '@/components/ChildGrid.vue'
import PortfolioEmpty from '@/components/PortfolioEmpty.vue'

const { focusId, view, tree, loadTree } = useHierarchy()

onMounted(() => {
  loadTree()
})

// Children of the focused node, sourced from the loaded tree so we get the
// recursive WorkItemNode form (childCountFor in ChildGrid needs to recurse).
// When focusId is null, PortfolioEmpty owns the children rendering.
const focusedChildren: ComputedRef<import('@/api').WorkItem[]> = computed(() => {
  if (focusId.value === null) return []
  // Walk tree to find focused node
  const find = (nodes: import('@/api').WorkItemNode[]): import('@/api').WorkItemNode | null => {
    for (const n of nodes) {
      if (n.id === focusId.value) return n
      const deeper = find(n.children)
      if (deeper !== null) return deeper
    }
    return null
  }
  const focused = find(tree.value)
  return focused ? focused.children : []
})
</script>

<template>
  <div
    class="min-h-screen w-screen text-[var(--ink)] bg-[var(--bg)] grid"
    style="grid-template-rows: 56px 1fr 32px; grid-template-columns: 100%;"
  >
    <AppHeader />

    <main
      class="grid overflow-hidden"
      style="grid-template-columns: 280px 1fr 360px;"
    >
      <!-- Left column: spine -->
      <HierarchySpine />

      <!-- Centre column -->
      <div class="overflow-y-auto flex flex-col">
        <CenterToolbar />
        <template v-if="focusId !== null">
          <Breadcrumbs />
          <!-- view toggle: focus shows lens + child grid; tree shows deferred placeholder -->
          <template v-if="view === 'focus'">
            <FocusLens />
            <ChildGrid v-if="focusedChildren.length > 0" :children="focusedChildren" />
          </template>
          <div
            v-else
            class="mx-4 my-6 p-6 border border-[var(--border)] rounded-xl text-[var(--faint)] font-mono text-[12px] italic"
          >
            TREE VIEW — DEFERRED
          </div>
        </template>
        <PortfolioEmpty v-else />
      </div>

      <!-- Right column: deferred sprint + agent stream placeholders -->
      <aside
        class="overflow-y-auto bg-[var(--surface)] border-l border-[var(--border)] px-4 py-3 flex flex-col gap-6"
      >
        <section>
          <h2 class="font-mono text-[10.5px] tracking-wider text-[var(--faint)] py-2">
            [04 / ACTIVE SPRINT]
          </h2>
          <!-- deferred: sprint composer (backend not yet implemented) -->
          <p class="text-[var(--ghost)] font-mono text-[11px] italic">
            Deferred — backend not yet implemented.
          </p>
        </section>
        <section>
          <h2 class="font-mono text-[10.5px] tracking-wider text-[var(--faint)] py-2">
            [05 / AGENT STREAM]
          </h2>
          <!-- deferred: agentic harness telemetry (entire backend missing) -->
          <p class="text-[var(--ghost)] font-mono text-[11px] italic">
            Deferred — backend not yet implemented.
          </p>
        </section>
      </aside>
    </main>

    <AppFooter />
  </div>
</template>
