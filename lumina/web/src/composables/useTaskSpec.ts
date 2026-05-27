// Task-spec composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/task-spec` structured-patch route.
//
// Mirrors the `useStoryPlan.ts` shape exactly. The only asymmetry is that
// `SetTaskSpecBody` carries an optional `tier` field which the backend
// translates into a SECOND mutation through `set_task_tier` (writes the
// typed `work_items.tier` column directly). Tier-aware apply is therefore a
// single-action call: pass `tier` to `apply` and the column write is
// dispatched server-side. The re-fetched detail's `item.tier` reflects
// the post-PATCH column value.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { SetTaskSpecBody, WorkItemDetail } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const lastUpdated = ref<WorkItemDetail | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  setTaskSpec: typeof productionApi.setTaskSpec
}
let api: Api = {
  setTaskSpec: productionApi.setTaskSpec,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  lastUpdated.value = null
  loading.value = false
  error.value = null
  api = {
    setTaskSpec: productionApi.setTaskSpec,
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

export function useTaskSpec() {
  /**
   * Apply a partial task-spec patch. Any subset of `execution_detail` /
   * `files_touched` / `outcome` / `tier` may be supplied. The first three
   * ride on the task's `attributes` JSON (one `set_work_item_attributes`
   * call); `tier` triggers a SECOND mutation that writes the typed
   * `work_items.tier` column. Pass `tier: null` to clear the column;
   * omit `tier` to leave it unchanged.
   *
   * Returns the re-fetched {@link WorkItemDetail} so the caller can fold
   * it back into a hierarchy-level singleton.
   */
  async function apply(
    taskId: string,
    patch: SetTaskSpecBody,
  ): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const updated = await api.setTaskSpec(taskId, patch)
      lastUpdated.value = updated
      return { ok: true, value: updated }
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
    lastUpdated,
    loading,
    error,
    apply,
    clearError,
  }
}
