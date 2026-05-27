// Work-item wire schemas + fetch wrappers.
//
// Carved out of the monolithic `lumina/web/src/api.ts` by T7 of the round-4
// plan (docs/plans/lumina-story-planning-round-4.md). This module owns the
// `WorkItem` / `WorkItemNode` / `WorkItemDetail` shapes and the four
// associated CRUD verbs (`fetchTree`, `fetchDetail`, `createWorkItem`,
// `updateWorkItem`, `updateStatus`).
//
// Nullability convention: every `Option<T>` on `domain::WorkItem` (and every
// other Rust read-aggregate folded into `WorkItemDetail`) uses `.nullable()`
// here. Verified at T7-implementation time by grepping `lumina/src/` for
// `skip_serializing_if` and finding zero hits — serde therefore emits `None`
// as JSON `null` rather than omitting the key. If a future Rust change adds
// `#[serde(skip_serializing_if = "Option::is_none")]` to any field on these
// types, the corresponding zod field must switch to `.nullish()` to tolerate
// the now-absent key.
//
// Cross-task note: this file inline-declares `AcceptanceCriterion`,
// `ResearchNote`, `OpenQuestion`, `QuestionOption`, `Risk`,
// `RejectedAlternative`, `TaskDependency`, `ContextBlock`, `Finding`,
// `WorkItemActivity`, `RepoLink` schemas because `WorkItemDetailWireSchema`
// references them. T9 / T10 / T11 will MOVE the per-family schemas out into
// their owned files (`acceptance-criteria.ts`, `research-notes.ts`,
// `risks.ts`, `rejected-alternatives.ts`, `task-deps.ts`, `findings.ts`,
// `activity.ts`, `open-questions.ts`, `context-blocks.ts`) and re-import them
// here. `RepoLink` lives in its own `repo-links.ts` (carved by T7); we
// re-import it from there.

import * as z from 'zod'

import { API_BASE, handle } from './http'
import {
  type Kind,
  KindSchema,
  type Status,
  StatusSchema,
  type Relevance,
  RelevanceSchema,
  type Effort,
  EffortSchema,
  type Complexity,
  ComplexitySchema,
  type Origin,
  OriginSchema,
  type ClosureGate,
  ClosureGateSchema,
  type Severity,
  SeveritySchema,
  type ActivityType,
  ActivityTypeSchema,
  type ResearchState,
  ResearchStateSchema,
  type QuestionStatus,
  QuestionStatusSchema,
  type Confidence,
  ConfidenceSchema,
  type TaskKind,
  TaskKindSchema,
  type Tier,
  TierSchema,
  type RiskSeverity,
  RiskSeveritySchema,
} from './wire-enums'
import { type RepoLink, RepoLinkSchema } from './repo-links'

// ---------------------------------------------------------------------------
// WorkItem (the row-level read aggregate) + nested-tree variant.
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
  // Round-4 additions (T7): the round-2/3 dispatch columns.
  task_kind: TaskKind | null
  tier: Tier | null
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
  // Round-4 additions (T7). `.nullable()` is correct: domain.rs has zero
  // `skip_serializing_if` attributes (grep-verified during T7), so an unset
  // column is emitted as JSON `null` rather than as an absent key.
  task_kind: TaskKindSchema.nullable(),
  tier: TierSchema.nullable(),
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

// ---------------------------------------------------------------------------
// Aggregate row types referenced by WorkItemDetail.
//
// T9/T10/T11 will MOVE each of these blocks to its own per-family file under
// `lumina/web/src/api/`; they live here for now so `WorkItemDetailWireSchema`
// has its dependencies in-scope without a forward-import chain.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Round-4 additions (T7): risks, rejected alternatives, task dependencies.
//
// Mirrors `domain::Risk` / `domain::RejectedAlternative` / `domain::TaskDependency`.
// T10 will move these to per-family files (`risks.ts`, `rejected-alternatives.ts`,
// `task-deps.ts`) and re-import from here.
// ---------------------------------------------------------------------------

/** A row of `risks` (migration 0005): a per-work-item risk register entry. */
export interface Risk {
  id: string
  work_item_id: string
  seq: number
  summary: string
  body: string | null
  rationale: string | null
  /**
   * CHECK-enforced `low|medium|high|critical`. Carried as `Option<String>` on
   * the Rust row to match the codebase's "row stores plain string, MCP-param
   * layer carries the typed enum" idiom; we mirror that here as a
   * `RiskSeverity | null`. In practice NOT NULL on the column, so it should
   * always be set on read — but we accept null to mirror the Rust shape.
   */
  severity: RiskSeverity | null
  mitigation: string | null
  superseded_by: string | null
  created_at: string
}

export const RiskSchema = z.object({
  id: z.string(),
  work_item_id: z.string(),
  seq: z.number(),
  summary: z.string(),
  body: z.string().nullable(),
  rationale: z.string().nullable(),
  severity: RiskSeveritySchema.nullable(),
  mitigation: z.string().nullable(),
  superseded_by: z.string().nullable(),
  created_at: z.string(),
})

/** A row of `rejected_alternatives` (migration 0005): a discarded planning option. */
export interface RejectedAlternative {
  id: string
  work_item_id: string
  seq: number
  summary: string
  body: string | null
  rationale: string | null
  /**
   * Free-text confidence grade (`high|medium|low` validated in the repo,
   * mirroring `research_notes.confidence`). Free TEXT on the Rust side, so we
   * type it as `Confidence | null` to surface the closed enum at the wire
   * while still tolerating any historical free-text values.
   */
  confidence: Confidence | null
  superseded_by: string | null
  created_at: string
}

export const RejectedAlternativeSchema = z.object({
  id: z.string(),
  work_item_id: z.string(),
  seq: z.number(),
  summary: z.string(),
  body: z.string().nullable(),
  rationale: z.string().nullable(),
  confidence: ConfidenceSchema.nullable(),
  superseded_by: z.string().nullable(),
  created_at: z.string(),
})

/** A row of `task_dependencies` (migration 0005): a task→task prerequisite edge. */
export interface TaskDependency {
  task_id: string
  depends_on_id: string
  /** Edge category — `data|sequence|…`; free TEXT, default `'data'`. */
  kind: string
  created_at: string
}

export const TaskDependencySchema = z.object({
  task_id: z.string(),
  depends_on_id: z.string(),
  kind: z.string(),
  created_at: z.string(),
})

// ---------------------------------------------------------------------------
// WorkItemDetail (the GET /work-items/{id} response).
// ---------------------------------------------------------------------------

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
  // Migration 0005 (round-4 T7): risks, rejected alternatives, task
  // dependencies. Same `#[serde(default)]` semantics → `.default([])`.
  risks: Risk[]
  rejected_alternatives: RejectedAlternative[]
  task_dependencies: TaskDependency[]
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
  // Round-4 (T7): the three new aggregates. Same `optional().default([])`
  // contract as `repo_links` — backend stamps `#[serde(default)]` on each.
  risks: z.array(RiskSchema).optional().default([]),
  rejected_alternatives: z.array(RejectedAlternativeSchema).optional().default([]),
  task_dependencies: z.array(TaskDependencySchema).optional().default([]),
})

// ---------------------------------------------------------------------------
// Fetch wrappers.
// ---------------------------------------------------------------------------

/** Body accepted by `POST /api/work-items`. */
export interface CreateWorkItemRequest {
  kind: Kind
  parent_id?: string | null
  title: string
  body?: string | null
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
