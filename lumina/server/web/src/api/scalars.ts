// Scalar single-column PATCH wrappers for the lumina wire layer.
//
// Mirrors the six typed single-column setters exposed by the HTTP slice in
// `lumina/src/http/structured_patches.rs` (round-4 T2). Each wrapper PATCHes
// `/api/work-items/{id}/<column>` with a body of `{ "value": <enum> }` and
// returns the re-fetched {@link WorkItem} (the handler refetches via
// `repo::get_work_item_detail` and returns its `item` field).
//
// Carved out by T8a of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). T7 pre-declared this file as
// a stub re-exported by `api/index.ts`; T8a fills the body.
//
// Nullability convention:
//   - `setRelevance` / `setEffort` / `setComplexity` / `setClosureGate` take
//     a NON-NULLABLE typed value. The corresponding `repo::set_*` fns take a
//     bare enum, and the HTTP handler rejects `{"value": null}` with 422.
//   - `setTaskKind` / `setTier` take `T | null`. The corresponding repo fns
//     take `Option<T>`, and `{"value": null}` clears the column.
//
// Naming + return-shape convention: matches the existing `updateWorkItem` /
// `updateStatus` wrappers in `work-items.ts` — each returns `Promise<WorkItem>`
// validated against `WorkItemSchema`.

import { API_BASE, handle } from './http'
import { WorkItemSchema, type WorkItem } from './work-items'
import {
  type Relevance,
  type Effort,
  type Complexity,
  type ClosureGate,
  type TaskKind,
  type Tier,
  type Shape,
} from './wire-enums'

/**
 * `PATCH /api/work-items/{id}/relevance` — set the `relevance` column. NOT
 * nullable: passing nothing is rejected at the wire (the handler validates a
 * present `value:` field). Settable only on epic/focus/story per
 * `repo::set_relevance`.
 */
export async function setRelevance(id: string, value: Relevance): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/relevance`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/effort` — set the `effort` column (s|m|l). NOT
 * nullable. Per the dispatch-tier derivation, `effort=l` forces Deep tier;
 * see `repo::compute_tier`.
 */
export async function setEffort(id: string, value: Effort): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/effort`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/complexity` — set the `complexity` column
 * (low|medium|high). NOT nullable. `complexity=high` forces Deep tier.
 */
export async function setComplexity(id: string, value: Complexity): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/complexity`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/closure-gate` — set the per-story `closure_gate`
 * column (hard|soft). NOT nullable. `hard` blocks task→done while any
 * acceptance criterion on the story remains unchecked.
 */
export async function setClosureGate(id: string, value: ClosureGate): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/closure-gate`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/task-kind` — set the per-task `task_kind` column
 * (foundation|main|polish). NULLABLE: passing `null` clears the column. NOTE:
 * this is the round-3.5 task-role discriminator stored on `work_items.task_kind`,
 * NOT the hierarchy `kind` column.
 */
export async function setTaskKind(id: string, value: TaskKind | null): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/task-kind`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/tier` — set the per-task `tier` column
 * (lite|deep). NULLABLE: passing `null` clears the column (typically when the
 * tier should be re-derived). Tier is the model-dispatch hint produced by
 * `repo::compute_tier(effort, complexity, files_touched_count, has_cross_repo)`.
 */
export async function setTier(id: string, value: Tier | null): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/tier`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}/shape` — set a focus's `shape` column
 * (vertical-slice|cross-cutting|foundational; migration 0010). NOT nullable:
 * `shape` is mandatory for a focus and is never cleared via this route, so the
 * handler rejects `{"value": null}` with 422 (mirrors `closure-gate`). The repo
 * setter kind-gates to `focus` (non-focus → 422).
 */
export async function setShape(id: string, value: Shape): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}/shape`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ value }),
    }),
    WorkItemSchema,
  )
}
