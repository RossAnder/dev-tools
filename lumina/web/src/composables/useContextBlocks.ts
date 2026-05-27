// Context-blocks composable — module-singleton state + async mutators for the
// `context_blocks` side of a work-item's WorkItemDetail.
//
// Mirrors the `useAcceptanceCriteria.ts` shape:
//   - Singleton refs declared once at module scope (no Pinia; no
//     provide/inject); every caller of `useContextBlocks()` shares the same
//     refs.
//   - Mutating actions return a discriminated `Result<T, E>` so call sites can
//     narrow on success/failure WITHOUT coupling to the singleton `error` ref
//     (which is still set as a side effect for the UI's error-banner
//     subscription).
//   - The API surface is swappable via `__setApiForTests` (override) and
//     `__resetForTests` (clear-state-and-restore-defaults). Both are required
//     because the module-singleton state itself leaks across test boundaries
//     — overriding the api alone is insufficient.
//
// The link / unlink mutators all touch the context-blocks fold on a specific
// parent work item — exactly like `useAcceptanceCriteria` binds to a parent.
// `currentParentBlocks` holds the live fold; the mutators refresh from
// `fetchDetail(parentId)` so the singleton stays consistent with what the
// parent panel renders. `create` is parent-less (the row exists before any
// link) so it does NOT refresh — the typical UX is to `create` then `link`,
// and the second call seeds the singleton.

import { ref } from 'vue'
import * as productionApi from '@/api'
import type { ContextBlock, CreateContextBlockBody } from '@/api'

/** See {@link import('./useHierarchy').Result} for the design rationale. */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// ---------------------------------------------------------------------------
// Module-singleton state.
// ---------------------------------------------------------------------------

const currentParentBlocks = ref<ContextBlock[]>([])
const loading = ref(false)
const error = ref<string | null>(null)

// ---------------------------------------------------------------------------
// Swappable API adapter for test isolation.
// ---------------------------------------------------------------------------

type Api = {
  createContextBlock: typeof productionApi.createContextBlock
  linkContextBlock: typeof productionApi.linkContextBlock
  unlinkContextBlock: typeof productionApi.unlinkContextBlock
  fetchDetail: typeof productionApi.fetchDetail
}
let api: Api = {
  createContextBlock: productionApi.createContextBlock,
  linkContextBlock: productionApi.linkContextBlock,
  unlinkContextBlock: productionApi.unlinkContextBlock,
  fetchDetail: productionApi.fetchDetail,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  currentParentBlocks.value = []
  loading.value = false
  error.value = null
  api = {
    createContextBlock: productionApi.createContextBlock,
    linkContextBlock: productionApi.linkContextBlock,
    unlinkContextBlock: productionApi.unlinkContextBlock,
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
 * Refresh the singleton from a `fetchDetail(parentId)` call. Used by `link` /
 * `unlink` (whose wrappers don't return the parent detail) — `create` is
 * parent-less so it skips the refresh.
 */
async function refresh(parentId: string): Promise<ContextBlock[]> {
  const detail = await api.fetchDetail(parentId)
  // Defensive `?? []` — the server's serde shape always emits the field, but
  // mirroring the other composables keeps the fallback path explicit.
  const blocks = detail.context_blocks ?? []
  currentParentBlocks.value = blocks
  return blocks
}

// ---------------------------------------------------------------------------
// Public surface.
// ---------------------------------------------------------------------------

export function useContextBlocks() {
  /**
   * Seed `currentParentBlocks` for a parent work item, without performing a
   * mutation. Call this from a panel's `onMounted` / `watch(parentId)` so the
   * singleton reflects the focused parent's linked context blocks.
   */
  async function bindParent(parentId: string): Promise<Result<ContextBlock[]>> {
    loading.value = true
    error.value = null
    try {
      const blocks = await refresh(parentId)
      return { ok: true, value: blocks }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Create a context block (no parent association yet — see {@link link}).
   * Returns the new row's id. Does NOT refresh the singleton; the typical UX
   * is to `create` immediately followed by `link(parentId, id)`, and the
   * second call seeds `currentParentBlocks`.
   */
  async function create(body: CreateContextBlockBody): Promise<Result<string>> {
    loading.value = true
    error.value = null
    try {
      const created = await api.createContextBlock(body)
      return { ok: true, value: created.id }
    } catch (e) {
      const message = toMessage(e)
      error.value = message
      return { ok: false, error: message }
    } finally {
      loading.value = false
    }
  }

  /**
   * Link an existing context block to a parent work item. Refreshes
   * `currentParentBlocks` from the parent's detail so the singleton reflects
   * the new fold.
   */
  async function link(parentId: string, contextBlockId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.linkContextBlock(parentId, contextBlockId)
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

  /**
   * Unlink a context block from a parent work item. The block row itself is
   * NOT deleted (other work items may still reference it); only the join-row
   * is removed. Refreshes `currentParentBlocks` from the parent's detail.
   */
  async function unlink(parentId: string, contextBlockId: string): Promise<Result<void>> {
    loading.value = true
    error.value = null
    try {
      await api.unlinkContextBlock(parentId, contextBlockId)
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

  /** Clear `error.value` — for the UI's "dismiss banner" button. */
  function clearError(): void {
    error.value = null
  }

  return {
    currentParentBlocks,
    loading,
    error,
    bindParent,
    create,
    link,
    unlink,
    clearError,
  }
}
