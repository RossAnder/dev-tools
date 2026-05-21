import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  fetchTree,
  fetchDetail,
  createWorkItem as apiCreateWorkItem,
  updateStatus as apiUpdateStatus,
  type WorkItemNode,
  type WorkItemDetail,
  type CreateWorkItemRequest,
} from '@/api'

/**
 * Pinia *setup* store (Pinia 3 `defineStore(id, () => {...})` form) holding the
 * work-item hierarchy tree, the currently-selected node id, and the loaded
 * detail for that node.
 *
 * The actions delegate to the plain-`fetch` wrapper in `@/api`; the store keeps
 * no fetch logic of its own, so the data layer can be swapped to Pinia Colada
 * later without changing this store's public surface.
 */
export const useHierarchyStore = defineStore('hierarchy', () => {
  // --- state ---
  /** Root nodes of the nested hierarchy (each carries a recursive `children`). */
  const tree = ref<WorkItemNode[]>([])
  /** The id of the node the user has selected in the tree, if any. */
  const selectedId = ref<string | null>(null)
  /** The detail (children + findings + context) for the selected node. */
  const detail = ref<WorkItemDetail | null>(null)
  /** True while a tree or detail request is in flight. */
  const loading = ref(false)
  /** The last error message from an action, or null. */
  const error = ref<string | null>(null)

  // --- actions ---

  /** Load the full hierarchy tree from `GET /api/work-items`. */
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

  /** Select a node and load its detail from `GET /api/work-items/{id}`. */
  async function selectNode(id: string): Promise<void> {
    selectedId.value = id
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

  /** Clear the current selection and its loaded detail. */
  function clearSelection(): void {
    selectedId.value = null
    detail.value = null
  }

  /**
   * Create a work item via `POST /api/work-items`, then refresh the tree so the
   * new node appears. Returns the created node's id, or null on failure.
   */
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

  /**
   * Update a node's status via `PATCH /api/work-items/{id}`, then refresh the
   * loaded detail if it is the selected node.
   */
  async function changeStatus(id: string, status: string): Promise<void> {
    error.value = null
    try {
      await apiUpdateStatus(id, status)
      if (selectedId.value === id) {
        detail.value = await fetchDetail(id)
      }
      await loadTree()
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e)
    }
  }

  return {
    tree,
    selectedId,
    detail,
    loading,
    error,
    loadTree,
    selectNode,
    clearSelection,
    createNode,
    changeStatus,
  }
})
