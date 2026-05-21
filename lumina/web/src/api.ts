// Thin fetch wrapper over the lumina axum JSON API.
//
// All requests go to `/api/*`; in development Vite proxies that prefix to the
// axum server on 127.0.0.1:8080 (see vite.config.ts), and in production the
// SPA is served from the same origin as the API, so a relative base works in
// both cases. Plain `fetch` is used deliberately — the store layer can be
// swapped to Pinia Colada later without touching this module's contract.

/** A node in the `work_items` adjacency-list hierarchy. */
export interface WorkItem {
  id: string
  kind: string
  parent_id: string | null
  title: string
  body: string | null
  status: string
  position: number
  created_at: string
  updated_at: string
}

/**
 * A work item as returned by the tree endpoint: the same fields as
 * {@link WorkItem} plus its recursively-nested children.
 */
export interface WorkItemNode extends WorkItem {
  children: WorkItemNode[]
}

export interface Finding {
  id: string
  work_item_id: string
  kind: string
  severity: string
  effort: string
  category: string
  status: string
  file: string | null
  line: number | null
  symbol: string | null
  summary: string
  description: string | null
  first_flagged: string | null
  rounds: number | null
  fingerprint: string | null
  flow: string | null
  resolution: string | null
  defer_reason: string | null
  defer_trigger: string | null
  wontfix_rationale: string | null
}

export interface ContextBlock {
  id: string
  title: string
  body: string
  created_at: string
  updated_at: string
}

/** Response shape of `GET /api/work-items/{id}` (WorkItemDetail). */
export interface WorkItemDetail {
  item: WorkItem
  children: WorkItem[]
  findings: Finding[]
  context_blocks: ContextBlock[]
}

/** Body accepted by `POST /api/work-items`. */
export interface CreateWorkItemRequest {
  kind: string
  parent_id?: string | null
  title: string
  body?: string | null
}

const API_BASE = '/api'

/** Parse a fetch Response as JSON, raising on a non-2xx status. */
async function handle<T>(res: Response): Promise<T> {
  if (!res.ok) {
    // The server emits `{"error":{"kind":...,"message":...}}` on failure;
    // surface the message when present, otherwise fall back to the status.
    let detail = `${res.status} ${res.statusText}`
    try {
      const body = (await res.json()) as { error?: { message?: string } }
      if (body?.error?.message) detail = body.error.message
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new Error(`API request failed: ${detail}`)
  }
  return (await res.json()) as T
}

/**
 * `GET /api/work-items` — the full nested tree of ROOT nodes (each with a
 * recursive `children` array).
 */
export async function fetchTree(): Promise<WorkItemNode[]> {
  return handle<WorkItemNode[]>(await fetch(`${API_BASE}/work-items`))
}

/**
 * `GET /api/work-items?parent_id=` / `?kind=` — a flat, filtered array of work
 * items (no recursive nesting). Pass either or both filters.
 */
export async function fetchWorkItems(filters: {
  parentId?: string
  kind?: string
}): Promise<WorkItem[]> {
  const params = new URLSearchParams()
  if (filters.parentId !== undefined) params.set('parent_id', filters.parentId)
  if (filters.kind !== undefined) params.set('kind', filters.kind)
  const query = params.toString()
  const url = query ? `${API_BASE}/work-items?${query}` : `${API_BASE}/work-items`
  return handle<WorkItem[]>(await fetch(url))
}

/** `GET /api/work-items/{id}` — item + children + findings + context blocks. */
export async function fetchDetail(id: string): Promise<WorkItemDetail> {
  return handle<WorkItemDetail>(await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}`))
}

/** `POST /api/work-items` — create a node; returns the created work item. */
export async function createWorkItem(req: CreateWorkItemRequest): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req),
    }),
  )
}

/** `PATCH /api/work-items/{id}` — update a node's free-text status. */
export async function updateStatus(id: string, status: string): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ status }),
    }),
  )
}
