<!--
  ExecutionSection — STORY-ONLY execution panel rendered BELOW the hero card
  (alongside ChildGrid), NOT as a tab inside the FocusLens hero. This honours
  the design's "execution belongs below, not in hero tabs" rule.

  Two cooperating reads over a story's task subtree:

    1. DISPATCH PLAN (useDispatchPlan) — `DispatchBatchEntry[][]`: one inner
       array per parallel-safe wave; each task row shows its derived tier
       (lite/deep) + effort + complexity. Read-mostly.

    2. TASK DEPENDENCIES + BATCHES (useTaskDependencies) — the Kahn batches
       (`string[][]`, plain task ids) plus the raw edge list
       (`TaskDependency[]`). Edges are editable: add (pick task + depends-on)
       and remove (ConfirmButton). A cycle introduced by an add surfaces on
       the composable's `cycleError` ref (the 422 `{edges}` envelope); we
       render it inline and let the next successful mutation clear it.

  Bind discipline (PANEL CONTRACT): both composables are module-singletons NOT
  auto-keyed to the focused node, so we seed them on a
  `watch(() => props.storyId, …, { immediate: true })`. The task-dep mutators
  refresh their own refs AND `useHierarchy().refresh(storyId)` internally, so
  this component does NOT manually refresh after a mutation — it only re-seeds
  on story change.

  Task-title resolution: ids in the plan / batches / edges are resolved to
  human titles via the focused story node's task children (sourced from
  `useHierarchy().focusedNode`, the shared id→node Map). When a title is not
  resolvable (stale tree), we fall back to the bare id.

  Vapor mode, inline Tailwind over var(--*) tokens, no <style scoped>.
-->
<script setup vapor lang="ts">
import { ref, computed, watch, type ComputedRef } from 'vue'
import type { WorkItemNode } from '@/api'
import { useHierarchy } from '@/composables/useHierarchy'
import { useDispatchPlan } from '@/composables/useDispatchPlan'
import { useTaskDependencies } from '@/composables/useTaskDependencies'
import ConfirmButton from '@/components/ui/ConfirmButton.vue'

const props = defineProps<{
  storyId: string
}>()

const { focusedNode } = useHierarchy()

const {
  current: dispatchWaves,
  loading: planLoading,
  error: planError,
  refresh: refreshPlan,
  clearError: clearPlanError,
} = useDispatchPlan()

const {
  dependencies,
  batches,
  loading: depsLoading,
  error: depsError,
  cycleError,
  bind: bindDeps,
  refreshBatches,
  addEdge,
  removeEdge,
} = useTaskDependencies()

// Load-bearing bind seeder: neither composable is auto-keyed to the focused
// node, so seed on the initial mount AND re-seed whenever the story changes.
watch(
  () => props.storyId,
  (id) => {
    void refreshPlan(id)
    void bindDeps(id)
    void refreshBatches(id)
  },
  { immediate: true },
)

// The story's direct task children, sourced from the shared hierarchy Map.
// These drive the add-edge pickers and the id→title resolution. We don't
// assume the focused node IS this story (defensive), but in practice App.vue
// only mounts this component for the focused story, so focusedNode is it.
const taskChildren: ComputedRef<WorkItemNode[]> = computed(() => {
  const node = focusedNode.value
  if (node === null || node.id !== props.storyId) return []
  return node.children.filter((c) => c.kind === 'task')
})

const titleById: ComputedRef<Map<string, string>> = computed(() => {
  const map = new Map<string, string>()
  for (const t of taskChildren.value) map.set(t.id, t.title)
  return map
})

function taskLabel(id: string): string {
  return titleById.value.get(id) ?? id
}

// --- Add-edge form state ---------------------------------------------------
const newTaskId = ref('')
const newDependsOnId = ref('')

const canAddEdge: ComputedRef<boolean> = computed(
  () =>
    newTaskId.value !== '' &&
    newDependsOnId.value !== '' &&
    newTaskId.value !== newDependsOnId.value,
)

async function handleAddEdge(): Promise<void> {
  if (!canAddEdge.value) return
  const result = await addEdge(props.storyId, newTaskId.value, newDependsOnId.value)
  if (result.ok) {
    newTaskId.value = ''
    newDependsOnId.value = ''
  }
  // On a cycle/failure the form stays populated so the operator can adjust;
  // `cycleError` / `depsError` render below.
}

async function handleRemoveEdge(dep: { task_id: string; depends_on_id: string }): Promise<void> {
  await removeEdge(props.storyId, dep.task_id, dep.depends_on_id)
}

// Dismiss the dispatch-plan error banner (separate composable, separate ref).
function dismissPlanError(): void {
  clearPlanError()
}

const busy: ComputedRef<boolean> = computed(() => planLoading.value || depsLoading.value)

// Pretty wave label: 1-based "WAVE n".
function waveLabel(index: number): string {
  return `WAVE ${index + 1}`
}

function tierLabel(tier: string | null): string {
  return tier === null ? '—' : tier.toUpperCase()
}

function metaLabel(value: string | null): string {
  return value === null ? '—' : value.toUpperCase()
}
</script>

<template>
  <section class="mx-4 my-4 flex flex-col gap-6">
    <header class="flex items-center justify-between">
      <h2 class="font-mono text-[11px] tracking-[0.18em] text-[var(--faint)] uppercase">
        Execution
        <span class="text-[var(--muted)]">· dispatch + dependencies</span>
      </h2>
      <span
        v-if="busy"
        class="font-mono text-[10px] tracking-[0.16em] text-[var(--ghost)]"
      >LOADING…</span>
    </header>

    <!-- ─── Dispatch plan: waves of task rows with tier/effort/complexity ─── -->
    <div class="flex flex-col gap-3">
      <h3 class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase">
        Dispatch Plan
        <span class="text-[var(--muted)]">· {{ dispatchWaves.length }} wave(s)</span>
      </h3>

      <p
        v-if="planError"
        class="flex items-center justify-between gap-3 text-[12px] italic text-[var(--faint)] border border-[var(--border)] rounded-md px-3 py-2"
        role="alert"
      >
        <span>{{ planError }}</span>
        <button
          type="button"
          class="font-mono text-[10.5px] tracking-[0.16em] px-2 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--faint)] uppercase shrink-0 hover:text-[var(--ink-2)] hover:border-[var(--border-strong)]"
          @click="dismissPlanError"
        >Dismiss</button>
      </p>

      <p
        v-else-if="dispatchWaves.length === 0"
        class="text-[var(--faint)] text-[12.5px] italic"
      >
        No dispatch plan — set task specs (effort/complexity) to derive tiers.
      </p>

      <div
        v-else
        class="flex flex-col gap-4"
      >
        <div
          v-for="(wave, wIdx) in dispatchWaves"
          :key="wIdx"
          class="border border-[var(--border)] rounded-lg p-3 flex flex-col gap-2"
        >
          <h4 class="font-mono text-[10px] tracking-[0.16em] text-[var(--muted)] uppercase">
            {{ waveLabel(wIdx) }}
            <span class="text-[var(--ghost)]">· {{ wave.length }} task(s)</span>
          </h4>
          <ul class="flex flex-col gap-1.5">
            <li
              v-for="entry in wave"
              :key="entry.task_id"
              class="flex items-center justify-between gap-3 text-[13px]"
            >
              <span class="flex-1 truncate text-[var(--ink-2)]">
                {{ taskLabel(entry.task_id) }}
              </span>
              <span class="flex items-center gap-1.5 font-mono text-[10px] tracking-[0.14em] shrink-0">
                <span
                  :class="[
                    'px-1.5 py-0.5 rounded border uppercase',
                    entry.tier === 'deep'
                      ? 'border-[var(--accent)] text-accent'
                      : entry.tier === 'lite'
                        ? 'border-[var(--border-strong)] text-[var(--ink-2)]'
                        : 'border-[var(--border)] text-[var(--ghost)]',
                  ]"
                  :title="`tier: ${entry.tier ?? 'unset'}`"
                >{{ tierLabel(entry.tier) }}</span>
                <span
                  class="px-1.5 py-0.5 rounded border border-[var(--border)] text-[var(--faint)]"
                  :title="`effort: ${entry.effort ?? 'unset'}`"
                >E:{{ metaLabel(entry.effort) }}</span>
                <span
                  class="px-1.5 py-0.5 rounded border border-[var(--border)] text-[var(--faint)]"
                  :title="`complexity: ${entry.complexity ?? 'unset'}`"
                >C:{{ metaLabel(entry.complexity) }}</span>
                <span
                  v-if="entry.has_cross_repo"
                  class="px-1.5 py-0.5 rounded border border-[var(--border)] text-[var(--faint)]"
                  title="touches a non-primary repo"
                >XR</span>
              </span>
            </li>
          </ul>
        </div>
      </div>
    </div>

    <!-- ─── Task dependencies: Kahn batches + editable edge list ─── -->
    <div class="flex flex-col gap-3">
      <h3 class="font-mono text-[10.5px] tracking-[0.18em] text-[var(--faint)] uppercase">
        Task Dependencies
        <span class="text-[var(--muted)]">· {{ dependencies.length }} edge(s)</span>
      </h3>

      <!-- Cycle error: the structured 422 {edges} envelope from a cyclic add. -->
      <div
        v-if="cycleError"
        class="border border-[var(--accent)] rounded-md px-3 py-2 flex flex-col gap-1"
        role="alert"
      >
        <p class="font-mono text-[11px] tracking-[0.14em] text-accent uppercase">
          Cycle detected
        </p>
        <p class="text-[12px] text-[var(--ink-2)]">{{ cycleError.message }}</p>
        <ul class="flex flex-col gap-0.5 mt-1">
          <li
            v-for="(edge, eIdx) in cycleError.edges"
            :key="eIdx"
            class="font-mono text-[11px] text-[var(--faint)]"
          >
            {{ taskLabel(edge.task_id) }} → {{ taskLabel(edge.depends_on_id) }}
          </li>
        </ul>
      </div>

      <!-- Generic deps error (non-cycle). cycleError already covers the cycle case. -->
      <p
        v-else-if="depsError"
        class="text-[var(--faint)] text-[12px] italic"
        role="alert"
      >
        {{ depsError }}
      </p>

      <!-- Computed batches (plain task ids per Kahn phase). -->
      <div class="flex flex-col gap-2">
        <h4 class="font-mono text-[10px] tracking-[0.16em] text-[var(--muted)] uppercase">
          Computed Batches
          <span class="text-[var(--ghost)]">· {{ batches.length }} phase(s)</span>
        </h4>
        <p
          v-if="batches.length === 0"
          class="text-[var(--faint)] text-[12.5px] italic"
        >
          No batches — no tasks under this story yet.
        </p>
        <div
          v-else
          class="flex flex-wrap gap-2"
        >
          <div
            v-for="(batch, bIdx) in batches"
            :key="bIdx"
            class="border border-[var(--border)] rounded-md px-2.5 py-1.5 flex flex-col gap-1"
          >
            <span class="font-mono text-[9.5px] tracking-[0.16em] text-[var(--ghost)] uppercase">
              {{ waveLabel(bIdx) }}
            </span>
            <span
              v-for="taskId in batch"
              :key="taskId"
              class="text-[12px] text-[var(--ink-2)]"
            >{{ taskLabel(taskId) }}</span>
          </div>
        </div>
      </div>

      <!-- Edge list with per-edge remove. -->
      <div class="flex flex-col gap-2">
        <h4 class="font-mono text-[10px] tracking-[0.16em] text-[var(--muted)] uppercase">
          Edges
        </h4>
        <ul
          v-if="dependencies.length > 0"
          class="flex flex-col gap-1.5"
        >
          <li
            v-for="dep in dependencies"
            :key="`${dep.task_id}->${dep.depends_on_id}`"
            class="flex items-center justify-between gap-3 text-[13px]"
          >
            <span class="flex-1 text-[var(--ink-2)]">
              {{ taskLabel(dep.task_id) }}
              <span class="text-[var(--faint)]">depends on</span>
              {{ taskLabel(dep.depends_on_id) }}
            </span>
            <ConfirmButton
              :label="'Remove'"
              :confirm-label="'Confirm?'"
              @confirm="handleRemoveEdge(dep)"
            />
          </li>
        </ul>
        <p
          v-else
          class="text-[var(--faint)] text-[12.5px] italic"
        >
          No dependency edges yet.
        </p>
      </div>

      <!-- Add-edge affordance: pick a task + a depends-on task. -->
      <form
        class="flex flex-wrap items-center gap-2"
        @submit.prevent="handleAddEdge"
      >
        <select
          v-model="newTaskId"
          :disabled="depsLoading || taskChildren.length === 0"
          aria-label="Task"
          class="font-sans text-[13px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] focus:outline-none focus:border-[var(--accent)] disabled:text-[var(--ghost)]"
        >
          <option value="" disabled>Task…</option>
          <option
            v-for="t in taskChildren"
            :key="t.id"
            :value="t.id"
          >{{ t.title }}</option>
        </select>
        <span class="font-mono text-[11px] text-[var(--faint)]">depends on</span>
        <select
          v-model="newDependsOnId"
          :disabled="depsLoading || taskChildren.length === 0"
          aria-label="Depends on"
          class="font-sans text-[13px] bg-[var(--surface)] border border-[var(--border)] rounded-md px-2 py-1 text-[var(--ink-2)] focus:outline-none focus:border-[var(--accent)] disabled:text-[var(--ghost)]"
        >
          <option value="" disabled>Depends on…</option>
          <option
            v-for="t in taskChildren"
            :key="t.id"
            :value="t.id"
          >{{ t.title }}</option>
        </select>
        <button
          type="submit"
          :disabled="!canAddEdge || depsLoading"
          class="font-mono text-[10.5px] tracking-[0.16em] px-3 py-1 rounded-md border border-[var(--border)] bg-[var(--surface-2)] text-[var(--ink-2)] uppercase shrink-0 hover:border-[var(--accent)] disabled:text-[var(--ghost)] disabled:cursor-not-allowed disabled:hover:border-[var(--border)]"
        >
          Add Edge
        </button>
      </form>
    </div>
  </section>
</template>
