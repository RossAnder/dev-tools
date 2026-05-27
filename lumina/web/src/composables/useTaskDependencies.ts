// Task-dependencies composable — module-singleton state + async mutators
// over the `task-deps.ts` fetch wrappers (migration 0005).
//
// Distinct from `useRisks`/`useRejectedAlternatives` in two ways:
//
//   1. The bound aggregate is STORY-scoped, not work-item-scoped — every
//      edge under a story is owned by its two task endpoints, but the
//      story's `task_dependencies` slice on the detail endpoint is the
//      canonical read surface. The `bind()` method seeds the singleton.
//
//   2. The mutating actions surface a structured CYCLE error (with the
//      offending `edges` from the server's Kahn residue) alongside the
//      plain-string error path. The discriminated `Result` arm carries
//      `CycleOrError` so the future UI can render the edges; the
//      singleton-scoped `error` ref still gets a flattened string for the
//      generic error banner. Both `addEdge()` and `computeBatches()` are
//      cycle-aware paths; `removeEdge()` is not (DELETE never raises a
//      cycle, per `lumina/src/error.rs`).

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { CycleError, CycleOrError, TaskDependency, TaskDependencyKind } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// Re-export so consumers can `import type { CycleError } from
// '@/composables/useTaskDependencies'` without dipping into the api layer.
export type { CycleError, CycleOrError } from '@/api'

// ---------------------------------------------------------------------------
// Module-singleton state.
//
// Two collection refs because this composable's bound aggregate is two-part:
// the raw edge list (`dependencies`) and the Kahn batches (`batches`). The
// canonical "items" naming used by the other family composables would be
// ambiguous here, so we keep two domain-specific ref names.
// ---------------------------------------------------------------------------

const dependencies = ref<TaskDependency[]>([])
const batches = ref<string[][]>([])
const loading = ref(false)
const error = ref<string | null>(null)
/**
 * The most recent cycle error encountered (sticky until the next
 * cycle-aware action). The plain-string `error` ref is also set when this
 * is populated — consumers can subscribe to either depending on whether
 * they want to render the edges or the flat message.
 */
const cycleError = ref<CycleError | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addTaskDependency: typeof productionApi.addTaskDependency
  removeTaskDependency: typeof productionApi.removeTaskDependency
  listTaskDependencies: typeof productionApi.listTaskDependencies
  computeTaskBatches: typeof productionApi.computeTaskBatches
}
let api: Api = {
  addTaskDependency: productionApi.addTaskDependency,
  removeTaskDependency: productionApi.removeTaskDependency,
  listTaskDependencies: productionApi.listTaskDependencies,
  computeTaskBatches: productionApi.computeTaskBatches,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  dependencies.value = []
  batches.value = []
  loading.value = false
  error.value = null
  cycleError.value = null
  api = {
    addTaskDependency: productionApi.addTaskDependency,
    removeTaskDependency: productionApi.removeTaskDependency,
    listTaskDependencies: productionApi.listTaskDependencies,
    computeTaskBatches: productionApi.computeTaskBatches,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Project a `CycleOrError` into the singleton refs and return its string
 * form for the action's failure-arm. Sets `cycleError.value` ONLY for the
 * structured cycle case so consumers can distinguish "the operation failed
 * because of a cycle (and here are the edges)" from "the operation failed
 * for some other reason".
 */
function projectCycleOrError(e: CycleOrError): string {
  if (e.kind === 'cycle') {
    cycleError.value = e
    error.value = e.message
    return e.message
  }
  cycleError.value = null
  error.value = e.message
  return e.message
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useTaskDependencies() {
  /**
   * Seed `dependencies` from the list endpoint. Use this variant when only
   * the edge list is needed (e.g. a deps panel) — does NOT compute batches.
   * Call `refreshBatches(storyId)` separately if both are needed.
   */
  async function bind(storyId: string): Promise<Result<TaskDependency[]>> {
    loading.value = true
    error.value = null
    cycleError.value = null
    try {
      const edges = await api.listTaskDependencies(storyId)
      dependencies.value = edges
      return { ok: true, value: edges }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Recompute Kahn's per-phase batches for `storyId`. On a graph cycle,
   * returns `{ ok: false, error: { kind: 'cycle', edges } }` and ALSO
   * populates the singleton `cycleError` ref so a panel can render the
   * offending edges without re-driving the action.
   */
  async function refreshBatches(
    storyId: string,
  ): Promise<Result<string[][], CycleOrError>> {
    loading.value = true
    error.value = null
    cycleError.value = null
    try {
      const result = await api.computeTaskBatches(storyId)
      if (!result.ok) {
        projectCycleOrError(result.error)
        return result
      }
      batches.value = result.value
      return { ok: true, value: result.value }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return {
        ok: false,
        error: { kind: 'error', message },
      }
    } finally {
      loading.value = false
    }
  }

  /**
   * Add a task→task edge under a story. After the insert succeeds, both
   * the edge list and the batches are refreshed so a downstream cycle
   * (introduced by THIS edge but only detectable on the next batch
   * compute, per the repo's lazy-cycle-check) surfaces immediately on the
   * singleton refs.
   *
   * Cycle handling lives entirely on the downstream `computeTaskBatches`
   * arm: the POST insert never raises `AppError::Cycle` (the repo PRE-CHECK
   * is only kind=task + non-self-loop — see
   * `lumina/src/http/task_dependencies.rs::block_task_on_task_handler`),
   * so any cycle introduced by THIS edge surfaces lazily on the batch
   * recompute. Round-4 R13 simplified the wrapper's return shape: the
   * insert path now throws generic errors via `handle<T>` instead of
   * returning a never-occurring cycle-Result arm.
   */
  async function addEdge(
    storyId: string,
    taskId: string,
    dependsOnId: string,
    kind?: TaskDependencyKind,
  ): Promise<Result<true, CycleOrError>> {
    loading.value = true
    error.value = null
    cycleError.value = null
    try {
      await api.addTaskDependency(taskId, dependsOnId, kind)
      // Refresh the edge list so the panel reflects the new row.
      try {
        dependencies.value = await api.listTaskDependencies(storyId)
      } catch (e) {
        // A failure to refresh the read shouldn't undo the successful
        // mutation; surface the message on the singleton but keep the
        // success Result so the caller's optimistic UI is correct.
        error.value = toMessage(e)
      }
      // Recompute batches — a cycle introduced by this edge surfaces here.
      const result = await api.computeTaskBatches(storyId)
      if (!result.ok) {
        projectCycleOrError(result.error)
        return { ok: false, error: result.error }
      }
      batches.value = result.value
      await useHierarchy().refresh(storyId)
      return { ok: true, value: true }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: { kind: 'error', message } }
    } finally {
      loading.value = false
    }
  }

  /**
   * Drop a task→task edge. No cycle path (DELETE cannot introduce one).
   * Returns a plain-string `Result` like the other family composables.
   */
  async function removeEdge(
    storyId: string,
    taskId: string,
    dependsOnId: string,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.removeTaskDependency(taskId, dependsOnId)
      // Refresh both list + batches so the panel reflects the drop.
      dependencies.value = await api.listTaskDependencies(storyId)
      const result = await api.computeTaskBatches(storyId)
      if (result.ok) {
        batches.value = result.value
      } else {
        // Cycle on a non-cycle-introducing op means the graph was already
        // cyclic — surface as a structured cycleError but return success
        // for the DELETE itself (the row IS gone).
        projectCycleOrError(result.error)
      }
      await useHierarchy().refresh(storyId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /** Clear `error.value` and `cycleError.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
    cycleError.value = null
  }

  return {
    dependencies,
    batches,
    loading,
    error,
    cycleError,
    bind,
    refreshBatches,
    addEdge,
    removeEdge,
    clearError,
  }
}
