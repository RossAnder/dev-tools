import { ref, computed, type ComputedRef } from 'vue'
import {
  fetchTree,
  fetchDetail,
  createWorkItem as apiCreateWorkItem,
  updateStatus as apiUpdateStatus,
  type WorkItem,
  type WorkItemNode,
  type WorkItemDetail,
  type CreateWorkItemRequest,
} from '@/api'

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

async function loadTree(): Promise<void> {
  loading.value = true
  error.value = null
  try {
    tree.value = await fetchTree()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
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
    detail.value = await fetchDetail(id)
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    loading.value = false
  }
}

async function createNode(req: CreateWorkItemRequest): Promise<string | null> {
  error.value = null
  try {
    const created = await apiCreateWorkItem(req)
    await loadTree()
    return created.id
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
    return null
  }
}

async function changeStatus(id: string, status: string): Promise<void> {
  error.value = null
  try {
    await apiUpdateStatus(id, status)
    if (focusId.value === id) {
      detail.value = await fetchDetail(id)
    }
    await loadTree()
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

/**
 * Flatten the recursive tree into a Map keyed by node id. Used by the
 * focusPath walker to climb the parent_id chain in O(1) per hop without
 * re-descending the tree at every step.
 */
function indexTree(nodes: WorkItemNode[]): Map<string, WorkItemNode> {
  const out = new Map<string, WorkItemNode>()
  const visit = (node: WorkItemNode): void => {
    out.set(node.id, node)
    for (const child of node.children) visit(child)
  }
  for (const root of nodes) visit(root)
  return out
}

/**
 * The breadcrumb chain from the root down to the focused node. Empty when
 * `focusId === null`. Returned root-first so consumers can render
 * `root › … › focused` directly.
 */
const focusPath: ComputedRef<WorkItem[]> = computed(() => {
  const id = focusId.value
  if (id === null) return []
  const byId = indexTree(tree.value)
  const chain: WorkItem[] = []
  let cursor: WorkItemNode | undefined = byId.get(id)
  while (cursor !== undefined) {
    chain.push(cursor)
    if (cursor.parent_id === null) break
    cursor = byId.get(cursor.parent_id)
  }
  chain.reverse()
  return chain
})

/** Effort weights for the rollup `size` field; unknown values contribute 0. */
function effortWeight(value: string | null | undefined): number {
  switch (value) {
    case 's':
      return 2
    case 'm':
      return 5
    case 'l':
      return 8
    default:
      return 0
  }
}

interface DescendantCounts {
  features: number
  stories: number
  tasks: number
  doneTasks: number
  totalTasks: number
  size: number
}

/**
 * Walk a node's `children` recursively and accumulate kind/status/effort
 * counts. The starting node itself is NOT included — callers pass the focused
 * node (or a synthetic root with all top-level nodes as children for the
 * portfolio rollup).
 */
function collectCounts(children: WorkItemNode[]): DescendantCounts {
  const counts: DescendantCounts = {
    features: 0,
    stories: 0,
    tasks: 0,
    doneTasks: 0,
    totalTasks: 0,
    size: 0,
  }
  const visit = (node: WorkItemNode): void => {
    if (node.kind === 'feature') counts.features += 1
    else if (node.kind === 'story') counts.stories += 1
    else if (node.kind === 'task') {
      counts.tasks += 1
      counts.totalTasks += 1
      if (node.status === 'done') counts.doneTasks += 1
      // Narrow defensively: `effort` may or may not be on WorkItemNode
      // depending on whether T1's type extension has landed yet.
      const eff = (node as { effort?: string | null }).effort ?? null
      counts.size += effortWeight(eff)
    }
    for (const child of node.children) visit(child)
  }
  for (const child of children) visit(child)
  return counts
}

/**
 * Aggregate descendant counts for the FocusLens KPI grid. With a focused node
 * we count its descendants only; with no focus (`focusId === null`) we roll
 * up the entire portfolio.
 */
const descendantCounts: ComputedRef<DescendantCounts> = computed(() => {
  const id = focusId.value
  if (id === null) return collectCounts(tree.value)
  const byId = indexTree(tree.value)
  const node = byId.get(id)
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
 * boolean ladder.
 */
const treeStatus: ComputedRef<'loading' | 'error' | 'empty' | 'ready'> = computed(() => {
  if (loading.value && tree.value.length === 0) return 'loading'
  if (error.value !== null && tree.value.length === 0) return 'error'
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
    descendantCounts,
    treeStatus,
    loadTree,
    setFocus,
    createNode,
    changeStatus,
  }
}
