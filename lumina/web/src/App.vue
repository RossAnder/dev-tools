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
import ExecutionSection from '@/components/ExecutionSection.vue'
import PortfolioEmpty from '@/components/PortfolioEmpty.vue'
import PtyConsole from '@/components/PtyConsole.vue'

const { focusId, view, loading, focusedNode, loadTree } = useHierarchy()

onMounted(() => {
  loadTree()
})

// Children of the focused node, sourced via the composable's memoised lookup
// (focusedNode) so we share one id→node Map across the app rather than each
// component walking the tree independently. When focusId is null,
// PortfolioEmpty owns the children rendering.
const focusedChildren: ComputedRef<import('@/api').WorkItem[]> = computed(
  () => focusedNode.value?.children ?? [],
)

// Story gate for the below-the-card execution panel: ExecutionSection is
// story-only (dispatch plan + task-dependency graph are story-scoped reads).
// Non-story focuses (epic/focus/task/project) render only ChildGrid, unchanged.
const focusedStoryId: ComputedRef<string | null> = computed(() =>
  focusedNode.value?.kind === 'story' ? focusedNode.value.id : null,
)
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
        <template v-if="focusId !== null && loading">
          <div
            class="flex items-center justify-center h-full text-[var(--faint)] font-mono text-[11px] tracking-[0.16em]"
          >
            LOADING…
          </div>
        </template>
        <template v-else-if="focusId !== null">
          <Breadcrumbs />
          <!-- view toggle: focus shows lens + child grid; tree shows deferred placeholder; pty mounts the supervisor console -->
          <template v-if="view === 'focus'">
            <FocusLens />
            <ChildGrid v-if="focusedChildren.length > 0" :children="focusedChildren" />
            <ExecutionSection v-if="focusedStoryId !== null" :story-id="focusedStoryId" />
          </template>
          <PtyConsole v-else-if="view === 'pty'" />
          <div
            v-else
            class="mx-4 my-6 p-6 border border-[var(--border)] rounded-xl text-[var(--faint)] font-mono text-[12px] italic"
          >
            TREE VIEW — DEFERRED
          </div>
        </template>
        <PtyConsole v-else-if="view === 'pty'" />
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
