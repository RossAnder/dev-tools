// Epic-plan composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/epic-plan` structured-patch route (migration 0010).
//
// Mirrors the `useStoryPlan.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useEpicPlan()` shares the same refs.
//   - The mutating action returns a discriminated `Result<T, E>` so call
//     sites can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (which is still set as a side effect for the UI's
//     error-banner subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults).
//
// Pure mutator semantics: this composable does NOT cache the epic detail.
// Each PATCH returns the re-fetched {@link WorkItemDetail}; `apply` also folds
// it into the shared hierarchy detail singleton (`useHierarchy().refresh`) so
// the FocusLens reflects the new `attributes.outcome` / `attributes.context`
// without a manual reload. The local state is confined to `lastUpdated` plus
// `loading` / `error`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { SetEpicPlanBody, WorkItemDetail } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

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
  setEpicPlan: typeof productionApi.setEpicPlan
}
let api: Api = {
  setEpicPlan: productionApi.setEpicPlan,
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
    setEpicPlan: productionApi.setEpicPlan,
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

export function useEpicPlan() {
  /**
   * Apply a partial epic-plan patch. Any subset of `outcome` / `context` may
   * be supplied; absent fields leave the corresponding `attributes` key
   * unchanged (present-only JSON-merge, mirroring `useStoryPlan.apply`). The
   * repo setter kind-gates to `epic` (non-epic → 422 surfaced on `error`).
   *
   * On success the re-fetched detail is folded into the shared hierarchy
   * singleton (no-op when the epic isn't the focused node).
   */
  async function apply(
    epicId: string,
    patch: SetEpicPlanBody,
  ): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const updated = await api.setEpicPlan(epicId, patch)
      lastUpdated.value = updated
      await useHierarchy().refresh(epicId)
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
