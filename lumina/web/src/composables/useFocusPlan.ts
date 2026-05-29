// Focus-plan composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/focus-plan` structured-patch route (migration 0010).
//
// Mirrors the `useEpicPlan.ts` / `useStoryPlan.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useFocusPlan()` shares the same refs.
//   - The mutating action returns a discriminated `Result<T, E>` so call
//     sites can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (still set as a side effect for the error-banner).
//   - The API surface is swappable via `__setApiForTests` /  `__resetForTests`.
//
// Pure mutator semantics: each PATCH returns the re-fetched
// {@link WorkItemDetail}; `apply` folds it into the shared hierarchy detail
// singleton so the FocusLens reflects the new `attributes.framing`. Local
// state is confined to `lastUpdated` plus `loading` / `error`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { SetFocusPlanBody, WorkItemDetail } from '@/api'
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
  setFocusPlan: typeof productionApi.setFocusPlan
}
let api: Api = {
  setFocusPlan: productionApi.setFocusPlan,
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
    setFocusPlan: productionApi.setFocusPlan,
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

export function useFocusPlan() {
  /**
   * Apply a partial focus-plan patch. The single `framing` field is optional
   * with present-only JSON-merge semantics; an absent field leaves the stored
   * `attributes.framing` untouched. The repo setter kind-gates to `focus`
   * (non-focus → 422 surfaced on `error`).
   *
   * On success the re-fetched detail is folded into the shared hierarchy
   * singleton (no-op when the focus isn't the focused node).
   */
  async function apply(
    focusItemId: string,
    patch: SetFocusPlanBody,
  ): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const updated = await api.setFocusPlan(focusItemId, patch)
      lastUpdated.value = updated
      await useHierarchy().refresh(focusItemId)
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
