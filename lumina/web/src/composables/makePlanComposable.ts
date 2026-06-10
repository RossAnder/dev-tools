// Shared factory for the epic/focus structured-patch plan composables.
//
// `useEpicPlan` and `useFocusPlan` are ~95% identical: both own module-singleton
// state (`lastUpdated` / `loading` / `error`), a swappable api adapter behind
// `__setApiForTests` / `__resetForTests`, a `toMessage` helper, and an `apply`
// mutator whose loading/error/try/catch/finally + `useHierarchy().refresh`
// contract is byte-for-byte the same. The ONLY axes that vary are the api method
// name + production function and the patch-body type. This factory captures the
// shared contract once; each composable module is a thin call to it.
//
// Module-singleton semantics are PRESERVED, not changed: each call to
// `makePlanComposable` is made exactly once at a composable module's top level,
// so the refs it closes over become that module's singleton — distinct per
// composable (epic and focus do NOT share state), and shared across every caller
// of the returned `useX()` accessor. No Pinia, no provide/inject.

import { ref } from 'vue'
import type { WorkItemDetail } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'

/** Signature shared by `setEpicPlan` / `setFocusPlan` (and any future plan setter). */
type PlanSetter<TBody> = (workItemId: string, body: TBody) => Promise<WorkItemDetail>

/**
 * Build a plan composable bound to a single structured-patch api method.
 *
 * @param apiKey   The api-method name (e.g. `'setEpicPlan'`). It keys the
 *                 swappable adapter so `__setApiForTests` overrides stay
 *                 addressable by that exact name — the shape the test suites
 *                 pass (`__setApiForTests({ setEpicPlan: mock })`).
 * @param setterFn The production api function the adapter defaults to.
 *
 * Returns the public accessor (`use`) plus the two test hooks, which the calling
 * module re-exports under their canonical names.
 */
export function makePlanComposable<TBody, TKey extends string>(
  apiKey: TKey,
  setterFn: PlanSetter<TBody>,
) {
  // -------------------------------------------------------------------------
  // Module-singleton state (one instance per factory call — see file header).
  // -------------------------------------------------------------------------

  const lastUpdated = ref<WorkItemDetail | null>(null)
  const loading = ref(false)
  const error = ref<string | null>(null)

  // -------------------------------------------------------------------------
  // Swappable API adapter for test isolation, keyed by `apiKey`.
  // -------------------------------------------------------------------------

  type Api = Record<TKey, PlanSetter<TBody>>
  const makeApi = (): Api => ({ [apiKey]: setterFn }) as Api
  let api: Api = makeApi()

  /** Replace API adapter entries. Test-only — do NOT call from production code. */
  function setApiForTests(override: Partial<Api>): void {
    api = { ...api, ...override }
  }

  /** Reset all module-singleton state. Test-only — do NOT call from production code. */
  function resetForTests(): void {
    lastUpdated.value = null
    loading.value = false
    error.value = null
    api = makeApi()
  }

  // -------------------------------------------------------------------------
  // Internal helpers.
  // -------------------------------------------------------------------------

  function toMessage(e: unknown): string {
    return e instanceof Error ? e.message : String(e)
  }

  // -------------------------------------------------------------------------
  // Public surface.
  // -------------------------------------------------------------------------

  function use() {
    /**
     * Apply a partial plan patch (present-only JSON-merge — absent fields leave
     * the corresponding `attributes` key unchanged). The repo setter kind-gates
     * the target item (mismatch → 422 surfaced on `error`).
     *
     * On success the re-fetched detail is folded into the shared hierarchy
     * singleton (no-op when the item isn't the focused node).
     */
    async function apply(
      workItemId: string,
      patch: TBody,
    ): Promise<Result<WorkItemDetail>> {
      loading.value = true
      error.value = null
      try {
        const updated = await api[apiKey](workItemId, patch)
        lastUpdated.value = updated
        await useHierarchy().refresh(workItemId)
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

  return { use, setApiForTests, resetForTests }
}
