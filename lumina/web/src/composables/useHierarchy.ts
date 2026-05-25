import { ref, computed, type ComputedRef } from 'vue'
import * as productionApi from '@/api'
import type {
  WorkItem,
  WorkItemNode,
  WorkItemDetail,
  CreateWorkItemRequest,
  Status,
} from '@/api'
import {
  indexTree,
  focusPathFrom,
  collectCounts,
  countSubtree,
  countMatchingIn,
  type DescendantCounts,
} from './treeUtils'

/**
 * Discriminated result type for mutating composable operations. Lets callers
 * disambiguate failure from success-with-empty-value against a LOCAL return
 * (rather than coupling the call site to the singleton `error` ref).
 *
 * The failure-path side effect on `error.value` is preserved alongside the
 * returned `{ ok: false, ... }` so the UI's existing error-banner subscription
 * keeps working — the new shape is additive to the side-effect, not a
 * replacement.
 */
export type Result<T, E = string> = { ok: true; value: T } | { ok: false; error: E }

// Module-singleton reactive state — the simplest store pattern that works
// natively in Vapor without registering a plugin on the app instance. Every
// caller of useHierarchy() sees the same refs because they are defined once
// when the module first loads.
//
// We dropped Pinia for this: with a single store of five refs and no need
// for devtools-plugin integration, setup-style `defineStore(id, () => …)`
// was pure ceremony around a composable. If the surface grows beyond what
// one module can comfortably hold, lift parts into nested composables
// before reaching back for a store library.

const tree = ref<WorkItemNode[]>([])
const focusId = ref<string | null>(null)
const detail = ref<WorkItemDetail | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)
const view = ref<'focus' | 'tree'>('focus')

// Swappable API adapter for test isolation. Production code calls the real
// helpers from `@/api`; tests can override individual entries via
// `__setApiForTests`. The seam is necessary because module-singleton state
// makes the standard vi.mock pattern awkward at the boundary.
type Api = {
  fetchTree: typeof productionApi.fetchTree
  fetchDetail: typeof productionApi.fetchDetail
  createWorkItem: typeof productionApi.createWorkItem
  updateStatus: typeof productionApi.updateStatus
}
let api: Api = {
  fetchTree: productionApi.fetchTree,
  fetchDetail: productionApi.fetchDetail,
  createWorkItem: productionApi.createWorkItem,
  updateStatus: productionApi.updateStatus,
}

/** Replace API adapter entries. Test-only — do NOT call from production code. */
export function __setApiForTests(override: Partial<Api>): void {
  api = { ...api, ...override }
}

/** Reset all module-singleton state. Test-only — do NOT call from production code. */
export function __resetForTests(): void {
  tree.value = []
  focusId.value = null
  detail.value = null
  loading.value = false
  error.value = null
  view.value = 'focus'
  abortCounter = 0
  inFlightStatus.clear()
  api = {
    fetchTree: productionApi.fetchTree,
    fetchDetail: productionApi.fetchDetail,
    createWorkItem: productionApi.createWorkItem,
    updateStatus: productionApi.updateStatus,
  }
}

// Request-id pattern for loadTree: each call bumps the counter and checks the
// token on resolution, so a late-returning fetch from a stale invocation never
// overwrites a fresher one. Full AbortController plumbing belongs in a
// follow-up that updates api.ts to accept a signal.
let abortCounter = 0

async function loadTree(): Promise<void> {
  const myToken = ++abortCounter
  loading.value = true
  error.value = null
  try {
    const result = await api.fetchTree()
    if (myToken !== abortCounter) return
    tree.value = result
    // Reconcile stale focus: if the focused id no longer exists in the
    // refreshed tree, clear focus rather than leaving focusPath silently empty.
    if (focusId.value !== null && !byId.value.get(focusId.value)) {
      focusId.value = null
      detail.value = null
    }
  } catch (e) {
    if (myToken !== abortCounter) return
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    if (myToken === abortCounter) loading.value = false
  }
}

/**
 * Set the focused work item. Passing `null` clears the focus and detail
 * (portfolio empty-state); passing an id loads the detail. The return type is
 * `Promise<void>` so callers that want to `await` the detail-load can; the
 * portfolio empty path returns immediately with no fetch.
 */
async function setFocus(id: string | null): Promise<void> {
  focusId.value = id
  if (id === null) {
    detail.value = null
    return
  }
  loading.value = true
  error.value = null
  try {
    detail.value = await api.fetchDetail(id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

/**
 * Create a work item. Returns a discriminated {@link Result}:
 * - `{ ok: true, value: id }` on success (the new work item's id), with the
 *   reactive tree refreshed via loadTree().
 * - `{ ok: false, error: message }` on failure, with `error.value` ALSO set
 *   for the UI's existing singleton-ref subscription (the return shape and
 *   the side-effect are intentionally redundant — see {@link Result} doc).
 *
 * Callers narrow with `if (!r.ok) { /* handle r.error * / return }` and then
 * use `r.value` as a string.
 */
async function createNode(req: CreateWorkItemRequest): Promise<Result<string>> {
  error.value = null
  try {
    const created = await api.createWorkItem(req)
    await loadTree()
    return { ok: true, value: created.id }
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    return { ok: false, error: message }
  }
}

/**
 * Mutate the status of a node in-place inside the reactive tree. Returns
 * true if the node was found. Mutating `node.status` on a member of a `ref`'d
 * array triggers Vue's reactivity because the inner objects are wrapped in a
 * reactive proxy.
 */
function patchTreeStatus(nodes: WorkItemNode[], id: string, status: Status): boolean {
  for (const node of nodes) {
    if (node.id === id) {
      node.status = status
      return true
    }
    if (patchTreeStatus(node.children, id, status)) return true
  }
  return false
}

// Per-id in-flight guard: drop a second concurrent status flip for the same
// node rather than racing the two responses to land in unspecified order.
const inFlightStatus = new Set<string>()

async function changeStatus(id: string, status: Status): Promise<void> {
  if (inFlightStatus.has(id)) return
  inFlightStatus.add(id)
  error.value = null
  try {
    const updated = await api.updateStatus(id, status)
    patchTreeStatus(tree.value, id, updated.status)
    if (focusId.value === id) {
      detail.value = await api.fetchDetail(id)
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    inFlightStatus.delete(id)
  }
}

/**
 * Memoised id→node lookup. Rebuilds only when `tree` changes, so consumers
 * (focusPath, descendantCounts, focusedNode, getSiblings, descendantCountFor)
 * share one Map instead of each constructing their own per access.
 */
const byId: ComputedRef<Map<string, WorkItemNode>> = computed(() => indexTree(tree.value))

/**
 * The breadcrumb chain from the root down to the focused node. Empty when
 * `focusId === null`. Returned root-first so consumers can render
 * `root › … › focused` directly.
 */
const focusPath: ComputedRef<WorkItem[]> = computed(() =>
  focusPathFrom(byId.value, focusId.value),
)

/** The currently-focused node, or null if there is no focus or the id is stale. */
const focusedNode: ComputedRef<WorkItemNode | null> = computed(() => {
  const id = focusId.value
  return id === null ? null : byId.value.get(id) ?? null
})

/**
 * Count descendants of the node with the given id. Returns 0 if the id is not
 * in the current tree. O(1) lookup via `byId` plus an O(subtree) walk.
 */
export function descendantCountFor(id: string): number {
  const node = byId.value.get(id)
  if (node === undefined) return 0
  return countSubtree(node.children)
}

/**
 * Return the sibling cohort for the node with `id` — i.e. the children array
 * of its parent (or the tree roots, for a root-level node). Empty if the id
 * is not in the current tree.
 */
export function getSiblings(id: string): WorkItemNode[] {
  const node = byId.value.get(id)
  if (node === undefined) return []
  if (node.parent_id === null) return tree.value
  const parent = byId.value.get(node.parent_id)
  return parent?.children ?? []
}

/** Count every node in the tree (roots + descendants) matching `predicate`. */
export function countMatching(predicate: (node: WorkItemNode) => boolean): number {
  return countMatchingIn(tree.value, predicate)
}

/**
 * Aggregate descendant counts for the FocusLens KPI grid. With a focused node
 * we count its descendants only; with no focus (`focusId === null`) we roll
 * up the entire portfolio.
 */
const descendantCounts: ComputedRef<DescendantCounts> = computed(() => {
  const id = focusId.value
  if (id === null) return collectCounts(tree.value)
  const node = byId.value.get(id)
  if (node === undefined) {
    return {
      features: 0,
      stories: 0,
      tasks: 0,
      doneTasks: 0,
      totalTasks: 0,
      size: 0,
    }
  }
  return collectCounts(node.children)
})

/**
 * Coarse-grained state for the tree pane: drives the loading/error/empty/ready
 * render branches without forcing every consumer to reconstruct the same
 * boolean ladder. `stale-error` is set when an error occurs but a previous
 * tree is still present — consumers can render the stale data alongside an
 * inline warning rather than blanking the pane.
 */
type TreeStatus = 'loading' | 'error' | 'empty' | 'ready' | 'stale-error'
const treeStatus: ComputedRef<TreeStatus> = computed(() => {
  if (loading.value && tree.value.length === 0) return 'loading'
  if (error.value !== null && tree.value.length === 0) return 'error'
  if (error.value !== null && tree.value.length > 0) return 'stale-error'
  if (!loading.value && error.value === null && tree.value.length === 0) return 'empty'
  return 'ready'
})

export function useHierarchy() {
  return {
    tree,
    focusId,
    detail,
    loading,
    error,
    view,
    focusPath,
    focusedNode,
    descendantCounts,
    treeStatus,
    loadTree,
    setFocus,
    createNode,
    changeStatus,
    descendantCountFor,
    getSiblings,
    countMatching,
  }
}
