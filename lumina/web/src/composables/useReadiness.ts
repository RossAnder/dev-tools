// Story-readiness composable — module-singleton state + an async refresh
// over the `GET /work-items/{story_id}/readiness` read.
//
// Mirrors the `useScalars.ts` / `useStoryPlan.ts` shape:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useReadiness()` shares the same
//     refs.
//   - `refresh()` returns a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error`
//     ref (still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults).
//
// State semantics: `current` holds the last-fetched `StoryReadiness`.
// Consumers that render the readiness panel watch this ref directly. There
// is no per-story keying — a second `refresh(otherStoryId)` REPLACES the
// singleton, matching the "one story in focus at a time" UX pattern the
// rest of the composables follow.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { StoryReadiness } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/**
 * The most recently fetched readiness aggregate, or `null` if no
 * `refresh` has succeeded since `__resetForTests` (or since module load).
 */
const current = ref<StoryReadiness | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  fetchReadiness: typeof productionApi.fetchReadiness
}
let api: Api = {
  fetchReadiness: productionApi.fetchReadiness,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  current.value = null
  loading.value = false
  error.value = null
  api = {
    fetchReadiness: productionApi.fetchReadiness,
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

export function useReadiness() {
  /**
   * Re-fetch the readiness aggregate for `storyId` and seed `current`.
   * Replaces any previously-fetched readiness — the singleton holds at most
   * one story's readiness at a time.
   */
  async function refresh(storyId: string): Promise<Result<StoryReadiness>> {
    loading.value = true
    error.value = null
    try {
      const fetched = await api.fetchReadiness(storyId)
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
