// Activity-log composable — module-singleton state + an async mutator that
// appends one row to a work item's activity log (migration 0002).
//
// Mirrors the `useScalars.ts` shape (pure-mutator composable, no
// per-parent-cache because the wrapper does not return the parent detail and
// the singleton stays cheap):
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useActivity()` shares the same refs.
//   - The mutator returns a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.
//
// Pure-mutator semantics: this composable does NOT fold appended rows into a
// local cache. The HTTP route returns only `{ ok: true }` (not the new row),
// so after `api.recordActivity` succeeds the composable triggers a hierarchy
// refresh so panels bound to `useHierarchy().detail` reflect the new row,
// mirroring the convention used by the other family composables. The local
// state is confined to `lastRecorded` (a watch-able ISO timestamp ref —
// handy for optimistic-UI flash effects) plus `loading` / `error`; the new
// activity row surfaces on the hierarchy refresh.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { RecordActivityBody } from '@/api'
import { useHierarchy } from './useHierarchy'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/**
 * ISO timestamp of the most recent successful `record()` call, or `null` if no
 * mutator has run since `__resetForTests` / module load. Consumers that want a
 * "just-recorded" indicator can watch this ref; consumers that need the
 * canonical activity rows should re-fetch via `useHierarchy().detail`.
 */
const lastRecorded = ref<string | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  recordActivity: typeof productionApi.recordActivity
}
let api: Api = {
  recordActivity: productionApi.recordActivity,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  lastRecorded.value = null
  loading.value = false
  error.value = null
  api = {
    recordActivity: productionApi.recordActivity,
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

export function useActivity() {
  /**
   * Append one activity-log entry to a work item. The backend validates
   * `body.entry_kind` against `repo::validate_entry_kind` (free TEXT, not a
   * closed enum at the wire layer); illegal values surface as a 422 here.
   * `body` and `ref_id` on the wire body are folded into the persisted
   * activity row's `payload` JSON by the backend.
   */
  async function record(
    workItemId: string,
    body: RecordActivityBody,
  ): Promise<Result<{ ok: true }>> {
    loading.value = true
    error.value = null
    try {
      const result = await api.recordActivity(workItemId, body)
      await useHierarchy().refresh(workItemId)
      lastRecorded.value = new Date().toISOString()
      return { ok: true, value: result }
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
    lastRecorded,
    loading,
    error,
    record,
    clearError,
  }
}
