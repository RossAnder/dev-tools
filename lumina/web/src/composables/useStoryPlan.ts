// Story-plan composable — module-singleton state + an async mutator for the
// `PATCH /work-items/{id}/story-plan` structured-patch route.
//
// Mirrors the `useScalars.ts` shape:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useStoryPlan()` shares the same refs.
//   - The mutating action returns a discriminated `Result<T, E>` so call
//     sites can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (which is still set as a side effect for the UI's
//     error-banner subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults).
//
// Pure mutator semantics: this composable does NOT cache the story detail.
// Each PATCH returns the re-fetched {@link WorkItemDetail} so the caller can
// fold it back into whichever detail singleton it already maintains
// (typically `useHierarchy().detail`). The local state is confined to
// `lastUpdated` (the most recent successful response — handy for
// optimistic-UI flash effects) plus `loading` / `error`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { SetStoryPlanBody, WorkItemDetail } from '@/api'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/**
 * The most recently mutated story detail, or `null` if no PATCH has been
 * issued since `__resetForTests` (or since module load). Consumers that
 * want a flash-on-update indicator can watch this ref; consumers that
 * want the canonical detail should keep using `useHierarchy().detail`.
 */
const lastUpdated = ref<WorkItemDetail | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  setStoryPlan: typeof productionApi.setStoryPlan
}
let api: Api = {
  setStoryPlan: productionApi.setStoryPlan,
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
    setStoryPlan: productionApi.setStoryPlan,
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

export function useStoryPlan() {
  /**
   * Apply a partial story-plan patch. Any subset of `problem_statement` /
   * `research_notes` / `execution_strategy` / `not_doing` /
   * `verification_commands` may be supplied; absent fields leave the
   * corresponding `attributes` key unchanged (set-or-leave at the key
   * level). The `verification_commands` sub-object is itself a SHALLOW
   * set — passing `{verification_commands: {build: "x"}}` REPLACES the
   * whole sub-object on the server, it does not deep-merge.
   *
   * Returns the re-fetched {@link WorkItemDetail} so the caller can fold
   * it back into a hierarchy-level singleton if needed.
   */
  async function apply(
    storyId: string,
    patch: SetStoryPlanBody,
  ): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const updated = await api.setStoryPlan(storyId, patch)
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
