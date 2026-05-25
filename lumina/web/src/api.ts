// Thin fetch wrapper over the lumina axum JSON API.
//
// All requests go to `/api/*`; in development Vite proxies that prefix to the
// axum server on 127.0.0.1:8080 (see vite.config.ts), and in production the
// SPA is served from the same origin as the API, so a relative base works in
// both cases. Plain `fetch` is used deliberately — the store layer can be
// swapped to Pinia Colada later without touching this module's contract.

import * as z from 'zod'

// ---------------------------------------------------------------------------
// Wire-enum string-literal unions.
//
// These mirror the closed Rust enums in `lumina/src/domain.rs` (each carrying
// `#[serde(rename_all = "snake_case")]`). Declared here as string-literal
// unions so the backend's closed sets surface as types on the frontend:
// consumers' `===` checks against these constants gain (optional)
// exhaustiveness, and typo'd comparisons fail at compile time. Each is a
// SUBTYPE of `string`, so any existing `node.kind === 'feature'` etc. check
// keeps compiling. Keep these aligned with `domain.rs` — adding a Rust enum
// variant requires adding it here too.
//
// To keep the TS type and the runtime zod schema from drifting, each enum is
// declared once as a `const` tuple of string literals: the TS type is derived
// via `(typeof TUPLE)[number]`, and the zod schema via `z.enum(TUPLE)`. A new
// Rust variant therefore needs ONE edit here.
// ---------------------------------------------------------------------------

const KIND_VALUES = ['project', 'epic', 'feature', 'story', 'task'] as const
/** Mirrors `domain::Kind` — the five legal work-item kinds (parent→child). */
export type Kind = (typeof KIND_VALUES)[number]
export const KindSchema = z.enum(KIND_VALUES)

// Containers (project/epic/feature/story) use 'open' as their default workflow
// status; only tasks cycle through the todo/in_progress/blocked/done/cancelled
// states. The Rust `domain::Status` enum (domain.rs:336) lists only the task
// states because migration 0001 declares `status` as free-text TEXT with no
// CHECK — 'open' is real container-level data, not in the enum. Keep both here
// so the wire schema accepts the actual response shape.
const STATUS_VALUES = ['open', 'todo', 'in_progress', 'blocked', 'done', 'cancelled'] as const
/** Mirrors `domain::Status` — the work-item workflow statuses. */
export type Status = (typeof STATUS_VALUES)[number]
export const StatusSchema = z.enum(STATUS_VALUES)

const RELEVANCE_VALUES = ['active', 'backlog', 'deferred', 'rejected'] as const
/** Mirrors `domain::Relevance` — settable only on epic/feature/story. */
export type Relevance = (typeof RELEVANCE_VALUES)[number]
export const RelevanceSchema = z.enum(RELEVANCE_VALUES)

const EFFORT_VALUES = ['s', 'm', 'l'] as const
/** Mirrors `domain::Effort` — wire form is lowercase `s|m|l` (display: S/M/L). */
export type Effort = (typeof EFFORT_VALUES)[number]
export const EffortSchema = z.enum(EFFORT_VALUES)

const COMPLEXITY_VALUES = ['low', 'medium', 'high'] as const
/** Mirrors `domain::Complexity` — drives model-tier assignment. */
export type Complexity = (typeof COMPLEXITY_VALUES)[number]
export const ComplexitySchema = z.enum(COMPLEXITY_VALUES)

const ORIGIN_VALUES = [
  'plan',
  'implement',
  'review',
  'optimise',
  'tdd',
  'human',
  'none',
] as const
/** Mirrors `domain::Origin` — provenance; `none` is the long-tail sentinel. */
export type Origin = (typeof ORIGIN_VALUES)[number]
export const OriginSchema = z.enum(ORIGIN_VALUES)

const CLOSURE_GATE_VALUES = ['hard', 'soft'] as const
/** Mirrors `domain::ClosureGate` — per-story task→done gate. */
export type ClosureGate = (typeof CLOSURE_GATE_VALUES)[number]
export const ClosureGateSchema = z.enum(CLOSURE_GATE_VALUES)

const SEVERITY_VALUES = ['critical', 'major', 'minor', 'suggestion'] as const
/** Mirrors `domain::Severity` — finding severities. */
export type Severity = (typeof SEVERITY_VALUES)[number]
export const SeveritySchema = z.enum(SEVERITY_VALUES)

const DISPOSITION_VALUES = [
  'fixed',
  'wontfix',
  'verified_clean',
  'deferred',
  'duplicate',
] as const
/** Mirrors `domain::Disposition` — terminal finding dispositions. */
export type Disposition = (typeof DISPOSITION_VALUES)[number]
export const DispositionSchema = z.enum(DISPOSITION_VALUES)

const ACTIVITY_TYPE_VALUES = [
  'execution',
  'verification',
  'deviation',
  'deferral',
  'reconcile',
  'status_transition',
  'checkpoint',
  'vet',
  'comment',
] as const
/** Mirrors `domain::ActivityType` — `work_item_activity.entry_kind`. */
export type ActivityType = (typeof ACTIVITY_TYPE_VALUES)[number]
export const ActivityTypeSchema = z.enum(ACTIVITY_TYPE_VALUES)

const RESEARCH_STATE_VALUES = ['proposed', 'accepted', 'rejected'] as const
/** Mirrors `domain::ResearchState` — `proposed → accepted | rejected`. */
export type ResearchState = (typeof RESEARCH_STATE_VALUES)[number]
export const ResearchStateSchema = z.enum(RESEARCH_STATE_VALUES)

const QUESTION_STATUS_VALUES = ['open', 'answered', 'cancelled'] as const
/** Mirrors `domain::QuestionStatus` — `open → answered | cancelled`. */
export type QuestionStatus = (typeof QUESTION_STATUS_VALUES)[number]
export const QuestionStatusSchema = z.enum(QUESTION_STATUS_VALUES)

const CONFIDENCE_VALUES = ['high', 'medium', 'low'] as const
/** Evidence grade for findings and research notes (free TEXT, repo-validated). */
export type Confidence = (typeof CONFIDENCE_VALUES)[number]
export const ConfidenceSchema = z.enum(CONFIDENCE_VALUES)

// ---------------------------------------------------------------------------
// Object-shape schemas.
//
// The TS interfaces below are kept for documentation/IDE-hover ergonomics; the
// zod schemas next to them are the runtime contract guard. Tests pin the two
// in lock-step by parsing a complete fixture (any drift between interface and
// schema surfaces as a type-mismatch at fixture-construction time).
// ---------------------------------------------------------------------------

/** A node in the `work_items` adjacency-list hierarchy. */
export interface WorkItem {
  id: string
  kind: Kind
  parent_id: string | null
  title: string
  body: string | null
  status: Status
  position: number | null
  attributes: Record<string, unknown> | null
  relevance: Relevance | null
  effort: Effort | null
  complexity: Complexity | null
  origin: Origin | null
  closure_gate: ClosureGate | null
  blocked_by_question_id: string | null
  enabling_option_id: string | null
  created_at: string
  updated_at: string
}

export const WorkItemSchema = z.object({
  id: z.string(),
  kind: KindSchema,
  parent_id: z.string().nullable(),
  title: z.string(),
  body: z.string().nullable(),
  status: StatusSchema,
  position: z.number().nullable(),
  // The server emits `attributes` as an arbitrary JSON object (or null);
  // `z.record(z.string(), z.unknown())` is the zod 4 form (single-arg
  // `z.record` was removed — it now requires both key and value schemas).
  attributes: z.record(z.string(), z.unknown()).nullable(),
  relevance: RelevanceSchema.nullable(),
  effort: EffortSchema.nullable(),
  complexity: ComplexitySchema.nullable(),
  origin: OriginSchema.nullable(),
  closure_gate: ClosureGateSchema.nullable(),
  blocked_by_question_id: z.string().nullable(),
  enabling_option_id: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
})

/**
 * A work item as returned by the tree endpoint: the same fields as
 * {@link WorkItem} plus its recursively-nested children.
 */
export interface WorkItemNode extends WorkItem {
  children: WorkItemNode[]
}

// Recursive schema. Zod 4 supports recursion via the JS getter pattern: the
// returned `WorkItemNodeSchema` is a plain `ZodObject` (full access to .pick /
// .extend / etc.) and `z.infer` resolves the self-reference correctly. This
// supersedes the zod-3 `z.lazy(() => ...)` form, which is still supported but
// no longer the canonical idiom.
export const WorkItemNodeSchema: z.ZodType<WorkItemNode> = WorkItemSchema.extend({
  get children() {
    return z.array(WorkItemNodeSchema)
  },
})

export interface Finding {
  id: string
  work_item_id: string | null
  kind: string
  severity: Severity
  effort: string | null
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
  dedup_id: string | null
  origin: Origin | null
  confidence: Confidence | null
  superseded_by: string | null
  resolved_at: string | null
  resolution: string | null
  defer_reason: string | null
  defer_trigger: string | null
  wontfix_rationale: string | null
}

export const FindingSchema = z.object({
  id: z.string(),
  work_item_id: z.string().nullable(),
  kind: z.string(),
  severity: SeveritySchema,
  effort: z.string().nullable(),
  category: z.string(),
  status: z.string(),
  file: z.string().nullable(),
  line: z.number().nullable(),
  symbol: z.string().nullable(),
  summary: z.string(),
  description: z.string().nullable(),
  first_flagged: z.string().nullable(),
  rounds: z.number().nullable(),
  fingerprint: z.string().nullable(),
  flow: z.string().nullable(),
  dedup_id: z.string().nullable(),
  origin: OriginSchema.nullable(),
  confidence: ConfidenceSchema.nullable(),
  superseded_by: z.string().nullable(),
  resolved_at: z.string().nullable(),
  // `resolution` is free-text describing the change that fixed the finding
  // (e.g. "Added exp check in refresh handler"), NOT a Disposition value. The
  // disposition vocabulary belongs on `status` above. Keep this as a plain
  // nullable string to match the wire.
  resolution: z.string().nullable(),
  defer_reason: z.string().nullable(),
  defer_trigger: z.string().nullable(),
  wontfix_rationale: z.string().nullable(),
  // Migration 0004: `findings.repo_id` — nullable foreign key into `repo_links`.
  // `nullable()` handles the live wire shape (column is nullable, NULL = the
  // file lives in the project's primary repo); `optional()` tolerates absent
  // fields from any pre-deploy caches that wouldn't yet emit the key.
  repo_id: z.string().nullable().optional(),
})

/**
 * A row of `repo_links` (migration 0004): a GitHub repo linked to a `kind='project'`
 * work item. `is_primary` is mirrored from the SQLite INTEGER column as 0/1 to
 * match the Rust wire shape — callers compare `=== 1` to test primacy.
 */
export interface RepoLink {
  id: string
  project_id: string
  slug: string
  position: number
  is_primary: number
  created_at: string
}

export const RepoLinkSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  slug: z.string(),
  position: z.number(),
  is_primary: z.number(),
  created_at: z.string(),
})

export interface ContextBlock {
  id: string
  title: string
  body: string
  created_at: string
  updated_at: string
}

export const ContextBlockSchema = z.object({
  id: z.string(),
  title: z.string(),
  body: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
})

/**
 * A row of `acceptance_criteria` (migration 0003). Note: on the wire `checked`
 * is a 0/1 integer (the Rust side mirrors the SQLite INTEGER column as `i64`),
 * but this boundary normalises it to a JS `boolean` in {@link fetchDetail} so
 * consumers can use truthy semantics directly.
 */
export interface AcceptanceCriterion {
  id: string
  work_item_id: string
  seq: number
  text: string
  checked: boolean
  checked_at: string | null
  checked_by: string | null
  created_at: string
}

// Wire shape: `checked` arrives as 0/1 integer; the normalised boolean form
// (exposed as {@link AcceptanceCriterion}) is produced by fetchDetail's
// post-parse transform.
export const AcceptanceCriterionWireSchema = z.object({
  id: z.string(),
  work_item_id: z.string(),
  seq: z.number(),
  text: z.string(),
  checked: z.number(),
  checked_at: z.string().nullable(),
  checked_by: z.string().nullable(),
  created_at: z.string(),
})

/** A row of `research_notes` (migration 0003). */
export interface ResearchNote {
  id: string
  work_item_id: string
  seq: number
  summary: string
  body: string | null
  confidence: Confidence | null
  state: ResearchState | null
  rationale: string | null
  lens: string | null
  origin: Origin | null
  superseded_by: string | null
  created_at: string
}

export const ResearchNoteSchema = z.object({
  id: z.string(),
  work_item_id: z.string(),
  seq: z.number(),
  summary: z.string(),
  body: z.string().nullable(),
  confidence: ConfidenceSchema.nullable(),
  state: ResearchStateSchema.nullable(),
  rationale: z.string().nullable(),
  lens: z.string().nullable(),
  origin: OriginSchema.nullable(),
  superseded_by: z.string().nullable(),
  created_at: z.string(),
})

/** A row of `question_options` (migration 0003): one branch of an open question. */
export interface QuestionOption {
  id: string
  question_id: string
  seq: number
  label: string
  detail: string | null
  created_at: string
}

export const QuestionOptionSchema = z.object({
  id: z.string(),
  question_id: z.string(),
  seq: z.number(),
  label: z.string(),
  detail: z.string().nullable(),
  created_at: z.string(),
})

/** A row of `open_questions` (migration 0003): a story-scoped decision. */
export interface OpenQuestion {
  id: string
  story_id: string
  seq: number
  question: string
  status: QuestionStatus | null
  answer: string | null
  chosen_option_id: string | null
  decided_at: string | null
  decided_by: string | null
  prompting_finding_id: string | null
  prompting_note_id: string | null
  created_at: string
  options: QuestionOption[]
}

export const OpenQuestionSchema = z.object({
  id: z.string(),
  story_id: z.string(),
  seq: z.number(),
  question: z.string(),
  status: QuestionStatusSchema.nullable(),
  answer: z.string().nullable(),
  chosen_option_id: z.string().nullable(),
  decided_at: z.string().nullable(),
  decided_by: z.string().nullable(),
  prompting_finding_id: z.string().nullable(),
  prompting_note_id: z.string().nullable(),
  created_at: z.string(),
  options: z.array(QuestionOptionSchema),
})

/** A row of `work_item_activity` (migration 0002): the per-item activity log. */
export interface WorkItemActivity {
  id: string
  work_item_id: string
  seq: number
  entry_kind: ActivityType
  author: string | null
  summary: string
  payload: Record<string, unknown> | null
  origin: Origin | null
  created_at: string
}

export const WorkItemActivitySchema = z.object({
  id: z.string(),
  work_item_id: z.string(),
  seq: z.number(),
  entry_kind: ActivityTypeSchema,
  author: z.string().nullable(),
  summary: z.string(),
  payload: z.record(z.string(), z.unknown()).nullable(),
  origin: OriginSchema.nullable(),
  created_at: z.string(),
})

/** Response shape of `GET /api/work-items/{id}` (WorkItemDetail). */
export interface WorkItemDetail {
  item: WorkItem
  children: WorkItem[]
  findings: Finding[]
  context_blocks: ContextBlock[]
  activity: WorkItemActivity[]
  acceptance_criteria: AcceptanceCriterion[]
  research_notes: ResearchNote[]
  open_questions: OpenQuestion[]
  // Migration 0004: populated only when `item.kind === 'project'`; defaults to
  // [] for every other kind. Rust side uses `#[serde(default)]`, so the field
  // may also be absent from any pre-deploy cached responses.
  repo_links: RepoLink[]
}

// Wire-shape schema for the detail endpoint: identical to the consumer-facing
// {@link WorkItemDetail} except `acceptance_criteria[].checked` is an integer
// (0/1). fetchDetail parses this shape, then runs a single-field transform to
// produce the boolean-normalised consumer type.
export const WorkItemDetailWireSchema = z.object({
  item: WorkItemSchema,
  children: z.array(WorkItemSchema),
  findings: z.array(FindingSchema),
  context_blocks: z.array(ContextBlockSchema),
  activity: z.array(WorkItemActivitySchema),
  acceptance_criteria: z.array(AcceptanceCriterionWireSchema),
  research_notes: z.array(ResearchNoteSchema),
  open_questions: z.array(OpenQuestionSchema),
  // `optional().default([])` matches the Rust `#[serde(default)]` semantics so
  // the schema accepts both responses that omit the field (non-project items
  // and pre-deploy cached responses) and explicit empty arrays.
  repo_links: z.array(RepoLinkSchema).optional().default([]),
})

/** Body accepted by `POST /api/work-items`. */
export interface CreateWorkItemRequest {
  kind: Kind
  parent_id?: string | null
  title: string
  body?: string | null
}

/**
 * Shape of the server's error envelope: `{"error":{"kind":...,"message":...}}`.
 * `handle` parses the success body against a caller-supplied schema; the error
 * envelope is parsed loosely (best-effort message extraction) because failure
 * paths must still produce a useful diagnostic when the server is itself
 * broken enough to drift on the error format.
 */
export const ApiErrorEnvelopeSchema = z.object({
  error: z
    .object({
      kind: z.string().optional(),
      message: z.string().optional(),
    })
    .optional(),
})

const API_BASE = '/api'

/**
 * Parse a fetch Response as JSON, raising on a non-2xx status.
 *
 * When `schema` is provided the success-path JSON is validated against it and
 * the parsed value is returned (a `ZodError` is wrapped as a contract-violation
 * Error so callers see a single recognisable failure mode at the wire
 * boundary). When `schema` is omitted the legacy untyped cast is used; new
 * call sites should always pass a schema.
 */
async function handle<T>(res: Response, schema?: z.ZodType<T>): Promise<T> {
  if (!res.ok) {
    // Best-effort error-message extraction. The success-path is strict; the
    // failure-path stays lenient because a broken server might also drift on
    // its error envelope and we'd rather surface SOMETHING than mask it
    // behind a second schema-violation.
    let detail = `${res.status} ${res.statusText}`
    try {
      const raw: unknown = await res.json()
      const parsed = ApiErrorEnvelopeSchema.safeParse(raw)
      if (parsed.success && parsed.data.error?.message) {
        // Clamp to 200 chars so a misbehaving server cannot overflow the
        // error panel (HierarchySpine.vue renders `{{ error }}` directly).
        // Vue interpolation already prevents XSS; this is denial-of-readability
        // defence only.
        const message = parsed.data.error.message
        detail = message.length > 200 ? message.slice(0, 197) + '…' : message
      }
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new Error(`API request failed: ${detail}`)
  }
  const payload: unknown = await res.json()
  if (schema === undefined) {
    return payload as T
  }
  const parsed = schema.safeParse(payload)
  if (!parsed.success) {
    throw new Error(
      `API contract violation: ${parsed.error.issues
        .map((i) => `${i.path.join('.') || '<root>'}: ${i.message}`)
        .join('; ')}`,
    )
  }
  return parsed.data
}

/**
 * `GET /api/work-items` — the full nested tree of ROOT nodes (each with a
 * recursive `children` array).
 */
export async function fetchTree(): Promise<WorkItemNode[]> {
  return handle<WorkItemNode[]>(
    await fetch(`${API_BASE}/work-items`),
    z.array(WorkItemNodeSchema),
  )
}

/** `GET /api/work-items/{id}` — item + children + findings + context blocks.
 *
 * Normalises the wire-level 0/1 integer `acceptance_criteria[].checked` into a
 * JS boolean at this boundary so downstream consumers can use truthy semantics
 * directly (rather than `=== 1`). All other fields pass through unchanged.
 */
export async function fetchDetail(id: string): Promise<WorkItemDetail> {
  const wire = await handle(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}`),
    WorkItemDetailWireSchema,
  )
  return {
    ...wire,
    acceptance_criteria: wire.acceptance_criteria.map((ac) => ({
      ...ac,
      // Wire is 0/1 (SQLite INTEGER mirrored as Rust i64); strict `=== 1`
      // rather than truthy so a stray non-1 numeric does not render as ticked.
      checked: ac.checked === 1,
    })),
  }
}

/** `POST /api/work-items` — create a node; returns the created work item. */
export async function createWorkItem(req: CreateWorkItemRequest): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req),
    }),
    WorkItemSchema,
  )
}

/**
 * Body accepted by `PATCH /api/work-items/{id}` (mirrors `domain::UpdateWorkItemRequest`).
 * Every field is optional with SET-OR-LEAVE semantics: an absent field leaves
 * the column untouched (the repo's `COALESCE(?, col)` write), it does NOT clear
 * the column to NULL.
 */
export interface UpdateWorkItemRequest {
  title?: string
  body?: string
  status?: Status
  position?: number
  attributes?: Record<string, unknown>
}

/**
 * `PATCH /api/work-items/{id}` — general partial-update; any subset of
 * `title`/`body`/`status`/`position`/`attributes` may be supplied.
 */
export async function updateWorkItem(
  id: string,
  patch: UpdateWorkItemRequest,
): Promise<WorkItem> {
  return handle<WorkItem>(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(patch),
    }),
    WorkItemSchema,
  )
}

/**
 * `PATCH /api/work-items/{id}` — narrow status-only wrapper. Delegates to
 * {@link updateWorkItem} so both share the same fetch + validation path; kept
 * as a named export because existing call sites pass a free-text status string.
 */
export async function updateStatus(id: string, status: string): Promise<WorkItem> {
  return updateWorkItem(id, { status: status as Status })
}

// ---------------------------------------------------------------------------
// Repo-link client (migration 0004).
//
// Three thin wrappers over the axum router's repo-link routes. Each goes
// through `handle<T>()` for the JSON-returning verbs (POST/PATCH) so contract
// violations surface as a single recognisable failure mode; the DELETE route
// returns 204 No Content (no JSON body) and so resolves to void without a
// schema parse — bypassing `handle<T>()` deliberately rather than papering
// over an empty body.
// ---------------------------------------------------------------------------

/** Response shape of `POST /api/work-items/{project_id}/repo-links`. */
const AddRepoLinkResponseSchema = z.object({ id: z.string() })

/** Response shape of `PATCH /api/work-items/{project_id}/repo-links/{id}`. */
const OkResponseSchema = z.object({ ok: z.boolean() })

/**
 * `POST /api/work-items/{project_id}/repo-links` — link a GitHub repo to a
 * project. `slug` is canonicalised server-side (lowercased; validated against
 * the GitHub owner/name rules). Returns the new `repo_links.id`.
 */
export async function addRepoLink(
  projectId: string,
  slug: string,
  isPrimary?: boolean,
): Promise<{ id: string }> {
  return handle(
    await fetch(`${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ slug, is_primary: isPrimary }),
    }),
    AddRepoLinkResponseSchema,
  )
}

/**
 * `DELETE /api/work-items/{project_id}/repo-links/{id}` — unlink a repo from a
 * project. Returns 204 No Content on success, hence the bare `res.ok` check
 * rather than the `handle<T>()` JSON-parsing path. The path's `project_id`
 * segment is purely structural REST clarity — the server looks the owning
 * project up from the row itself.
 */
export async function removeRepoLink(projectId: string, id: string): Promise<void> {
  const res = await fetch(
    `${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links/${encodeURIComponent(id)}`,
    { method: 'DELETE' },
  )
  if (!res.ok) {
    let detail = `${res.status} ${res.statusText}`
    try {
      const raw: unknown = await res.json()
      const parsed = ApiErrorEnvelopeSchema.safeParse(raw)
      if (parsed.success && parsed.data.error?.message) {
        const message = parsed.data.error.message
        detail = message.length > 200 ? message.slice(0, 197) + '…' : message
      }
    } catch {
      // non-JSON error body — keep the status line
    }
    throw new Error(`API request failed: ${detail}`)
  }
}

/**
 * `PATCH /api/work-items/{project_id}/repo-links/{id}` — promote a repo link
 * to primary. The body is fixed at `{ is_primary: true }` per the server
 * contract; demotion happens implicitly via promoting another link.
 */
export async function setPrimaryRepo(projectId: string, id: string): Promise<{ ok: boolean }> {
  return handle(
    await fetch(
      `${API_BASE}/work-items/${encodeURIComponent(projectId)}/repo-links/${encodeURIComponent(id)}`,
      {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ is_primary: true }),
      },
    ),
    OkResponseSchema,
  )
}
