// Research-notes composable — module-singleton state + async mutators for the
// `research_notes` side of a work-item's WorkItemDetail.
//
// Mirrors the `useRepoLinks.ts` / `useAcceptanceCriteria.ts` shape exactly:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useResearchNotes()` shares the same
//     refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type {
  AddResearchNoteBody,
  ResearchNote,
  UpdateResearchNoteBody,
  WorkItemDetail,
} from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
//
// The note CRUD all happens against the live `superseded_by IS NULL` fold for
// a specific parent work item. `currentParentNotes` holds that live set; the
// mutators refresh from `fetchDetail(parentId)` so the singleton stays
// consistent with what the parent panel renders. The supersede mutator is the
// exception: its response is `{ ok: true }`, so we refresh explicitly after.
// ---------------------------------------------------------------------------

const currentParentNotes = ref<ResearchNote[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  addResearchNote: typeof productionApi.addResearchNote
  updateResearchNote: typeof productionApi.updateResearchNote
  supersedeResearchNote: typeof productionApi.supersedeResearchNote
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  addResearchNote: productionApi.addResearchNote,
  updateResearchNote: productionApi.updateResearchNote,
  supersedeResearchNote: productionApi.supersedeResearchNote,
  fetchDetail: productionApi.fetchDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  currentParentNotes.value = []
  loading.value = false
  error.value = null
  api = {
    addResearchNote: productionApi.addResearchNote,
    updateResearchNote: productionApi.updateResearchNote,
    supersedeResearchNote: productionApi.supersedeResearchNote,
    fetchDetail: productionApi.fetchDetail,
  }
}

// ---------------------------------------------------------------------------
// Internal helpers.
// ---------------------------------------------------------------------------

function toMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e)
}

/**
 * Refresh the singleton from a `fetchDetail(parentId)` call. Used by `add` /
 * `supersede` (whose wrappers don't return the parent detail) — `update`
 * already gets the re-fetched parent in its response and seeds the singleton
 * directly from that, avoiding a second round-trip.
 */
async function refresh(parentId: string): Promise<ResearchNote[]> {
  const detail = await api.fetchDetail(parentId)
  // Defensive `?? []` — the server's serde shape always emits the field, but
  // the zod schema applies `.optional().default([])` so a pre-deploy cache
  // without it would still resolve.
  const notes = detail.research_notes ?? []
  currentParentNotes.value = notes
  return notes
}

/** Seed the singleton from a freshly-returned `WorkItemDetail` (no extra fetch). */
function seedFromDetail(detail: WorkItemDetail): void {
  currentParentNotes.value = detail.research_notes ?? []
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useResearchNotes() {
  /**
   * Seed `currentParentNotes` for a parent work item, without performing a
   * mutation. Call this from a panel's `onMounted` / `watch(parentId)` so the
   * singleton reflects the focused parent's notes.
   */
  async function bindParent(parentId: string): Promise<Result<ResearchNote[]>> {
    loading.value = true
    error.value = null
    try {
      const notes = await refresh(parentId)
      return { ok: true, value: notes }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function add(parentId: string, body: AddResearchNoteBody): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.addResearchNote(parentId, body)
      await refresh(parentId)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  async function update(
    noteId: string,
    patch: UpdateResearchNoteBody,
  ): Promise<Result<WorkItemDetail>> {
    loading.value = true
    error.value = null
    try {
      const detail = await api.updateResearchNote(noteId, patch)
      seedFromDetail(detail)
      return { ok: true, value: detail }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Supersede `oldId` with `newId`. After the server applies the supersession
   * (one txn / one event), refresh the singleton against `parentId` so the
   * live fold drops the old note and surfaces the new one.
   */
  async function supersede(
    parentId: string,
    oldId: string,
    newId: string,
  ): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.supersedeResearchNote(oldId, newId)
      await refresh(parentId)
      return { ok: true, value: undefined }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  return {
    currentParentNotes,
    loading,
    error,
    bindParent,
    add,
    update,
    supersede,
  }
}
