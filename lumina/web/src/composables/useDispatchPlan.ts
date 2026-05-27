// Dispatch-plan composable — module-singleton state + an async refresh
// over the `GET /work-items/{story_id}/dispatch-plan` read.
//
// Mirrors the `useReadiness.ts` shape — same module-singleton refs,
// `Result<T, E>` return, swappable API for test isolation. The aggregate
// here is `DispatchBatchEntry[][]` (one inner array per parallel-safe
// wave); the singleton holds the most recently fetched plan.
//
// Distinct from `useTaskDependencies().refreshBatches()` in two ways:
//   1. Different endpoint — this one returns the FULL row aggregate
//      (effort/complexity/tier/files_touched_count/has_cross_repo per
//      task), the other returns plain task ids per cell.
//   2. No structured cycle envelope — the dispatch-plan endpoint is a
//      read-only composition; a cycle in the underlying graph still
//      surfaces as 422 from the server but is flattened to a thrown
//      `Error` by `handle<T>()` (see the note on `fetchDispatchPlan`).
//      Call sites that need the structured cycle residue should
//      pre-validate via `useTaskDependencies().refreshBatches`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { DispatchBatchEntry } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/**
 * The most recently fetched dispatch plan, as `DispatchBatchEntry[][]`
 * (one inner array per parallel-safe wave). Empty array when no refresh
 * has succeeded since `__resetForTests` (or since module load) — distinct
 * from the success-with-no-tasks case which also returns `[]`. Consumers
 * that need to distinguish "never fetched" from "fetched empty" should
 * watch `error` and `loading` alongside.
 */
const current = ref<DispatchBatchEntry[][]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  fetchDispatchPlan: typeof productionApi.fetchDispatchPlan
}
let api: Api = {
  fetchDispatchPlan: productionApi.fetchDispatchPlan,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  current.value = []
  loading.value = false
  error.value = null
  api = {
    fetchDispatchPlan: productionApi.fetchDispatchPlan,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useDispatchPlan() {
  /**
   * Re-fetch the dispatch plan for `storyId` and seed `current`. Replaces
   * any previously-fetched plan — the singleton holds at most one story's
   * plan at a time.
   *
   * **Cycle limitation**: a cycle in the story's underlying task-dependency
   * graph causes the server to return a 422 with a structured
   * `{ error: { kind: "cycle", edges: [...] } }` envelope.  This composable
   * routes through the generic `handle<T>()` path, so that envelope is
   * flattened to a plain `Error` whose `.message` is the server's error
   * string.  The structured `edges[]` array is **lost**.
   *
   * If a component needs the structured cycle data (e.g. to highlight the
   * offending edges in the dispatch-plan panel), pre-validate the graph via
   * `useTaskDependencies().refreshBatches(storyId)` — that composable uses
   * `handleWithCycleCheck()` and surfaces a typed `CycleError` with the
   * full `edges` array before this fetch is attempted.
   */
  async function refresh(
    storyId: string,
  ): Promise<Result<DispatchBatchEntry[][]>> {
    loading.value = true
    error.value = null
    try {
      const fetched = await api.fetchDispatchPlan(storyId)
      current.value = fetched
      return { ok: true, value: fetched }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    current,
    loading,
    error,
    refresh,
    clearError,
  }
}
