// Task-dependency fetch wrappers (migration 0005).
//
// Filled in by Phase-5 task T10 of the round-4 plan
// (docs/plans/lumina-story-planning-round-4.md). Two mutators + two reads
// over the axum router's task-dependency routes (see
// `lumina/src/http/task_dependencies.rs`).
//
// Cycle-422 contract — the load-bearing piece of this module.
// `compute_task_batches` MAY return a 422 with a structured cycle envelope:
//
//   { "error": { "kind": "cycle", "message": "...",
//                "edges": [ { "task_id": "...", "depends_on_id": "..." }, ... ] } }
//
// (See `lumina/src/error.rs::AppError::Cycle` + the `IntoResponse` impl —
// confirmed by reading the file. Each edge is a `{task_id, depends_on_id}`
// object, NOT a tuple.)
//
// `add_task_dependency` does NOT raise a cycle on insert. The repo PRE-CHECK
// is only kind=task + non-self-loop; a cycle introduced by an INSERT is a
// graph-level property that surfaces lazily on the next `compute_task_batches`
// read (per `lumina/src/http/task_dependencies.rs::block_task_on_task_handler`
// comment line 70: "the row itself goes in successfully — the cycle is a
// property of the GRAPH, not the single INSERT"). Round-4 R13 removed the
// dead cycle-Result error-arm from `addTaskDependency`.
//
// `handle<T>()` does NOT distinguish cycle errors from generic 4xx/5xx — it
// flattens every non-2xx into a thrown `Error`. We therefore introduce a
// local `handleWithCycleCheck<T>()` for the ONE read endpoint that actually
// raises cycles (`compute_task_batches`); the wrapper parses success
// identically but returns a `Result<T, CycleError | string>` on failure,
// surfacing the structured `edges` distinctly so the future UI can render the
// offending pairs without re-running the topo sort.
//
// Schema re-export note: `TaskDependencySchema` is defined inline in
// `work-items.ts`. `TaskIdSchema` is NOT pre-declared there — the
// `compute_task_batches` route returns a `Vec<Vec<String>>` of plain task
// ids (see `http::task_dependencies::compute_task_batches_handler`), not a
// row aggregate. We expose `TaskIdSchema = z.string()` here as the
// per-cell schema in case future code wants to compose it; the read wrapper
// returns `string[][]` directly.

import * as z from 'zod'

import { API_BASE, ApiErrorEnvelopeSchema, handle, handleVoid } from './http'
import {
  type TaskDependency,
  TaskDependencySchema,
} from './work-items'

export { type TaskDependency, TaskDependencySchema }

// ---------------------------------------------------------------------------
// Cycle-422 envelope.
// ---------------------------------------------------------------------------

/**
 * One offending edge in the cycle residue — matches the JSON shape emitted
 * by `AppError::Cycle::into_response` in `lumina/src/error.rs`.
 */
export interface CycleEdge {
  task_id: string
  depends_on_id: string
}

export const CycleEdgeSchema = z.object({
  task_id: z.string(),
  depends_on_id: z.string(),
})

/**
 * Structured cycle-error: parsed out of the 422 envelope when the server
 * sets `error.kind === "cycle"`. `message` is the server's human-readable
 * summary ("task-dependency cycle detected (N edge(s) in the residue)");
 * `edges` is the strongly-connected residue from Kahn's algorithm.
 */
export interface CycleError {
  kind: 'cycle'
  message: string
  edges: CycleEdge[]
}

/** Loose schema for the cycle envelope's `error` block. */
const CycleEnvelopeErrorSchema = z.object({
  kind: z.literal('cycle'),
  message: z.string().optional(),
  edges: z.array(CycleEdgeSchema),
})

const CycleEnvelopeSchema = z.object({ error: CycleEnvelopeErrorSchema })

// ---------------------------------------------------------------------------
// Result + cycle-aware handle.
// ---------------------------------------------------------------------------

/**
 * Discriminated-Result mirror of `useHierarchy::Result`. The error arm
 * defaults to `CycleOrError` (the union used by both cycle-aware verbs in
 * this module); callers may override `E` for a narrower type.
 */
export type Result<T, E = CycleOrError> =
  | { ok: true; value: T }
  | { ok: false; error: E }

/** Per-call error type for the two cycle-aware verbs. */
export type CycleOrError = CycleError | { kind: 'error'; message: string }

/**
 * Variant of `handle<T>()` that, on a 422 with a structured cycle envelope,
 * returns `{ ok: false, error: <CycleError> }` rather than throwing. Every
 * other non-2xx flattens to `{ ok: false, error: { kind: 'error', ... } }`.
 *
 * Success-path parsing is identical to `handle<T>()`: schema-validate the
 * JSON body and return it as `{ ok: true, value }`. A schema violation on
 * the success path throws (same as `handle<T>()`) — contract violations
 * still surface as a single recognisable failure mode at the wire boundary.
 */
async function handleWithCycleCheck<T>(
  res: Response,
  schema: z.ZodType<T>,
): Promise<Result<T, CycleOrError>> {
  if (res.ok) {
    const payload: unknown = await res.json()
    const parsed = schema.safeParse(payload)
    if (!parsed.success) {
      throw new Error(
        `API contract violation: ${parsed.error.issues
          .map((i) => `${i.path.join('.') || '<root>'}: ${i.message}`)
          .join('; ')}`,
      )
    }
    return { ok: true, value: parsed.data }
  }

  // Failure path — first try to peel a structured cycle envelope. Only the
  // 422 status carries a `kind: "cycle"` in this codebase (see
  // `AppError::status()` in lumina/src/error.rs); we still check the parsed
  // `kind` field rather than just the status because that's the actual
  // discriminator.
  let raw: unknown
  try {
    raw = await res.json()
  } catch {
    return {
      ok: false,
      error: {
        kind: 'error',
        message: `API request failed: ${res.status} ${res.statusText}`,
      },
    }
  }

  if (res.status === 422) {
    const cycle = CycleEnvelopeSchema.safeParse(raw)
    if (cycle.success) {
      return {
        ok: false,
        error: {
          kind: 'cycle',
          message:
            cycle.data.error.message ??
            `task-dependency cycle detected (${cycle.data.error.edges.length} edge(s))`,
          edges: cycle.data.error.edges,
        },
      }
    }
  }

  // Generic error envelope — best-effort message extraction.
  let detail = `${res.status} ${res.statusText}`
  const generic = ApiErrorEnvelopeSchema.safeParse(raw)
  if (generic.success && generic.data.error?.message) {
    const message = generic.data.error.message
    detail = message.length > 200 ? message.slice(0, 197) + '…' : message
  }
  return {
    ok: false,
    error: { kind: 'error', message: `API request failed: ${detail}` },
  }
}

// ---------------------------------------------------------------------------
// Schemas for the read endpoints.
// ---------------------------------------------------------------------------

const TaskDependencyListSchema = z.array(TaskDependencySchema)

/**
 * Per-cell schema for `compute_task_batches` rows. Backend currently emits
 * `Vec<Vec<String>>` (plain task ids), so each cell is a task-id string.
 * Surfaced here so future code can compose it; `computeTaskBatches` returns
 * `string[][]` directly to match the live wire.
 */
export const TaskIdSchema = z.string()
const TaskBatchesSchema = z.array(z.array(TaskIdSchema))

// ---------------------------------------------------------------------------
// Wrappers.
// ---------------------------------------------------------------------------

/** Edge kind on `task_dependencies.kind`. Defaults to `'data'` server-side. */
export type TaskDependencyKind = 'data' | string

/** Response shape of `POST /api/work-items/{task_id}/depends-on/{depends_on_id}`. */
const AddTaskDependencyResponseSchema = z.object({ ok: z.boolean() })

/**
 * `POST /api/work-items/{task_id}/depends-on/{depends_on_id}` — add a
 * task→task prerequisite edge. Returns void on success (server replies
 * `{ ok: true }` with 201 Created; we discard the envelope).
 *
 * Cycle handling: the repo PRE-CHECK is only kind=task + non-self-loop, so
 * the insert never raises `AppError::Cycle`. A cycle introduced by THIS edge
 * goes in successfully and surfaces lazily on the next `computeTaskBatches`
 * call — see the file-level comment and
 * `lumina/src/http/task_dependencies.rs::block_task_on_task_handler`. The
 * round-4 R13 sweep removed this wrapper's dead `Result<true, CycleOrError>`
 * return shape: only generic 4xx/5xx can come back here, and `handle<T>`'s
 * thrown-Error path is the right surface for those.
 */
export async function addTaskDependency(
  taskId: string,
  dependsOnId: string,
  kind?: TaskDependencyKind,
): Promise<void> {
  await handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(taskId)}/depends-on/${encodeURIComponent(dependsOnId)}`,
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(kind === undefined ? {} : { kind }),
      },
    ),
    AddTaskDependencyResponseSchema,
  )
}

/**
 * `DELETE /api/work-items/{task_id}/depends-on/{depends_on_id}` — drop an
 * edge. 204 on success; 404 when the edge does not exist. Bypasses the
 * cycle handler (DELETE never raises a cycle).
 */
export async function removeTaskDependency(
  taskId: string,
  dependsOnId: string,
): Promise<void> {
  const res = await fetch(
    `${API_BASE}/work-items/${encodeURIComponent(taskId)}/depends-on/${encodeURIComponent(dependsOnId)}`,
    { method: 'DELETE' },
  )
  return handleVoid(res)
}

/**
 * `GET /api/work-items/{story_id}/task-dependencies` — list every
 * task→task edge whose both endpoints are direct task children of
 * `story_id`. No cycle possible on a read; goes through plain `handle<T>()`.
 */
export async function listTaskDependencies(
  storyId: string,
): Promise<TaskDependency[]> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(storyId)}/task-dependencies`,
    ),
    TaskDependencyListSchema,
  )
}

/**
 * `GET /api/work-items/{story_id}/task-batches` — Kahn's per-phase batching
 * of the story's tasks. Returns `string[][]` on success (one inner array
 * per parallel-safe phase). 422 + structured cycle envelope on a graph
 * cycle, surfaced as `{ ok: false, error: { kind: 'cycle', edges } }`.
 */
export async function computeTaskBatches(
  storyId: string,
): Promise<Result<string[][], CycleOrError>> {
  const res = await fetch(
    `${API_BASE}/work-items/${encodeURIComponent(storyId)}/task-batches`,
  )
  return handleWithCycleCheck(res, TaskBatchesSchema)
}
