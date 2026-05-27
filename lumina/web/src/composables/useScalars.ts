// Scalar single-column mutator composable — module-singleton state + async
// mutators for the six typed scalar PATCHes (relevance / effort / complexity /
// closure-gate / task-kind / tier).
//
// Mirrors the `useRepoLinks.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useScalars()` shares the same refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites
//     can narrow on success/failure WITHOUT coupling to the singleton
//     `error` ref (which is still set as a side effect for the UI's
//     error-banner subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are
//     required because the module-singleton state itself leaks across test
//     boundaries — overriding the api alone is insufficient.
//
// Pure mutator semantics: this composable does NOT cache the affected work
// item. Each setter PATCHes the wire and returns the re-fetched {@link WorkItem}
// so the caller can fold it back into whichever hierarchy/detail singleton it
// already maintains (typically `useHierarchy().detail`). The local state is
// confined to `lastUpdated` (the most recent successful response — handy for
// optimistic-UI flash effects) plus `loading` / `error`.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type {
  WorkItem,
  Relevance,
  Effort,
  Complexity,
  ClosureGate,
  TaskKind,
  Tier,
} from '@/api'

import type { Result } from './result'
export type { Result }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

/**
 * The most recently mutated work item, or `null` if no scalar setter has been
 * called since `__resetForTests` (or since module load). Consumers that want
 * to flash a "just updated" indicator can watch this ref; consumers that want
 * the canonical detail should keep using `useHierarchy().detail`.
 */
const lastUpdated = ref<WorkItem | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  setRelevance: typeof productionApi.setRelevance
  setEffort: typeof productionApi.setEffort
  setComplexity: typeof productionApi.setComplexity
  setClosureGate: typeof productionApi.setClosureGate
  setTaskKind: typeof productionApi.setTaskKind
  setTier: typeof productionApi.setTier
}
let api: Api = {
  setRelevance: productionApi.setRelevance,
  setEffort: productionApi.setEffort,
  setComplexity: productionApi.setComplexity,
  setClosureGate: productionApi.setClosureGate,
  setTaskKind: productionApi.setTaskKind,
  setTier: productionApi.setTier,
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
    setRelevance: productionApi.setRelevance,
    setEffort: productionApi.setEffort,
    setComplexity: productionApi.setComplexity,
    setClosureGate: productionApi.setClosureGate,
    setTaskKind: productionApi.setTaskKind,
    setTier: productionApi.setTier,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Shared scaffolding for all six setters: flip `loading`, clear `error`, run
 * the wire call, fold the response into `lastUpdated`, and translate any thrown
 * exception into a `Result.error`. The fn captures the loading/error toggles
 * so each setter body stays a single line.
 */
async function runSetter(
  call: () => Promise<WorkItem>,
): Promise<Result<WorkItem>> {
  loading.value = true
  error.value = null
  try {
    const updated = await call()
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

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useScalars() {
  /** Set the `relevance` column (non-nullable; epic/feature/story only). */
  async function setRelevance(id: string, value: Relevance): Promise<Result<WorkItem>> {
    return runSetter(() => api.setRelevance(id, value))
  }

  /** Set the `effort` column (non-nullable; task only). */
  async function setEffort(id: string, value: Effort): Promise<Result<WorkItem>> {
    return runSetter(() => api.setEffort(id, value))
  }

  /** Set the `complexity` column (non-nullable; task only). */
  async function setComplexity(id: string, value: Complexity): Promise<Result<WorkItem>> {
    return runSetter(() => api.setComplexity(id, value))
  }

  /** Set the `closure_gate` column (non-nullable; story only). */
  async function setClosureGate(id: string, value: ClosureGate): Promise<Result<WorkItem>> {
    return runSetter(() => api.setClosureGate(id, value))
  }

  /** Set the per-task `task_kind` column. Pass `null` to clear. */
  async function setTaskKind(id: string, value: TaskKind | null): Promise<Result<WorkItem>> {
    return runSetter(() => api.setTaskKind(id, value))
  }

  /** Set the per-task dispatch `tier`. Pass `null` to clear (re-derive). */
  async function setTier(id: string, value: Tier | null): Promise<Result<WorkItem>> {
    return runSetter(() => api.setTier(id, value))
  }

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    lastUpdated,
    loading,
    error,
    setRelevance,
    setEffort,
    setComplexity,
    setClosureGate,
    setTaskKind,
    setTier,
    clearError,
  }
}
