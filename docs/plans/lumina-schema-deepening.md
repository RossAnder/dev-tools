# Plan: lumina schema-deepening (Plan 1.5)

**Plan path**: docs/plans/lumina-schema-deepening.md
**Created**: 2026-05-22
**Status**: draft

> Plan 1.5 of the lumina harness-reshape. Plan 1 landed the schema/data/MCP foundation;
> this plan **deepens the schema** with the planning- and decision-grade concepts the
> data model needs, and is sequenced **before the Plan 2 UI** so the SPA's per-kind
> forms don't harden against an incomplete schema. It changes NO `claude/commands/*` or
> `claude/agents/*` flow logic, and (per User Decisions) does NOT build dependency edges
> or the sprint composer — only the substrate.

---

## Context

The lumina vertical slice + Plan 1 proved the thread (SQLite → MCP → axum → Vue) and
landed a hybrid schema: real columns for cross-cutting fields, a per-kind `attributes`
JSON object for narrative fields, a `work_item_activity` FK child for append-only history,
`findings`, and `context_blocks`. A design-research pass (this session) surfaced that the
schema is missing the **planning- and decision-grade** concepts that the eventual sprint
composer + the Plan 2 UI both depend on:

- no per-task **acceptance criteria** (checkable, gateable),
- no **effort/complexity** grading (the composer routes work to models by these),
- a **relevance** dimension is needed to replace the dropped `active-flow` concept
  (structural guidance on what's in play vs parked vs rejected),
- no **origin** provenance (was this work planned up front, or did it surface during
  implementation? review/optimise/tdd?),
- **research notes** are a flat `story.attributes` string, not first-class records that
  can carry confidence, accept/reject state, lens, and supersede competing findings,
- no **open-questions** lifecycle — and specifically no way to pre-define a *branch of
  tasks per answer option* so work can be drafted ahead of a pending decision.

This plan delivers those as schema + repository write-paths + MCP tools + read/export
folds + tests, following the exact idioms Plan 1 established.

## Scope

**In scope:**
- Additive migration `0003`: new `work_items` columns (`relevance`, `effort`,
  `complexity`, `origin`, `closure_gate`, `blocked_by_question_id`, `enabling_option_id`);
  `findings` weighting/provenance (`origin` + `confidence` + `superseded_by`); and four new FK child tables (`acceptance_criteria`,
  `research_notes`, `open_questions`, `question_options`).
- Domain structs + typed enums (`Relevance`, `Effort`, `Complexity`, `Origin`,
  `ResearchState`, `QuestionStatus`, `ClosureGate`) so MCP params advertise legal values;
  `WorkItemDetail` folds the three new child collections; `WorkItem`/`Finding` gain the
  new columns.
- Repository write-paths under the single-mutation-path + events-outbox discipline:
  relevance/effort/complexity/origin setters, acceptance-criteria CRUD + check/uncheck
  (with a **configurable-per-story closure gate** on task→done), research-note CRUD +
  accept/reject + supersede, open-question + option CRUD, and **branch resolution** (pick
  an option → unblock that branch's tasks, cancel the other branches' exclusive tasks).
- Read-side fold (detail returns the new columns + child collections) + git-export fold.
- A `CLAUDE.md` note and `claude/skills/lumina/SKILL.md` catalogue update for the new tools.

**Out of scope (deferred):**
- **All dependency-edge work** (`depends_on`/`related`/cycle-detection/topological
  batching) — its own later plan (User Decision).
- The **sprint composer + dispatch** engine itself (this plan only lays the substrate it
  reads: relevance, effort, complexity, origin).
- Any Vue/webui change (Plan 2) — including the hand-maintained TypeScript interfaces in
  `lumina/web/src/api.ts` (`WorkItem`/`Finding`/`WorkItemDetail`/`CreateWorkItemRequest`),
  which do NOT pick up the new fields for free and are deferred to Plan 2 (note: `api.ts`
  already lacks `activity` from Plan 1); HTTP **write** endpoints beyond the existing generic
  PATCH (new writes stay MCP-only, mirroring Plan 1); the importer (`import.rs` is touched
  only to thread the new `origin` param through its `create_work_item` calls — and imported
  epic/feature/story rows acquire the default `relevance="backlog"`, which is intended);
  Postgres wiring; auth.
- The pre-existing **`Status` divergences** (`create_work_item` seeds the literal `"open"`,
  not a `Status` enum member; `Severity` = `major`/`minor` vs the ledger's `warning`) are
  **left as-is** — fixing them risks existing data/webui and is unrelated to this plan
  (noted in Risks).

**Affected areas:** `lumina/migrations/`, `lumina/src/` (`domain.rs`, `repo.rs`, `mcp.rs`,
`http.rs`, `export.rs`), `lumina/tests/`, `lumina/.sqlx/` (regenerated),
`claude/skills/lumina/`, `CLAUDE.md`. **Estimated ~10 files.** Under the single-plan guard;
waved so no parallel batch shares a file.

## Research Notes

> Sourced from this session's design-research pass (a `flow-research-deep` best-practices
> run + an Explore mining of the tomlctl/harness schemas) and the project memory notes
> `lumina-schema-deepening-decisions` / `lumina-relevance-and-sprint-composer`. Evidence
> grade noted per finding; all modeling recommendations were vetted against lumina's
> pinned hybrid-storage principle.

- **Acceptance criteria → child table, not an attributes array.** GitHub task-lists (no
  per-item state), Jira's static field (checklist add-ons exist precisely because it lacks
  per-item done-state), and BDD/Gherkin (overkill with no automation consumer) converge on:
  a tickable item needs its own `checked`/`checked_at`/`checked_by` state, which an
  `attributes` JSON array cannot query or tick without read-modify-write. *Impact:* child
  table `acceptance_criteria` mirroring the `work_item_activity` idiom (FK CASCADE + `seq` +
  `UNIQUE(work_item_id,seq)`); a check also appends a `verification` activity row (state vs
  immutable audit). Grade: MEDIUM.
- **Effort vs complexity — two columns, not one conflated score.** Story points
  deliberately conflate effort+complexity+risk (you then can't ask "big because hard or
  risky?"); t-shirt S/M/L is reproducible for an LLM where numeric risk scores are not
  (arxiv 2210.13701: numeric confidence is a weak signal). *Impact:* `effort` (S/M/L → batch
  sizing) and `complexity` (low/med/high → model tier) as **separate typed columns** (User
  Decision). Grade: MEDIUM.
- **Origin → typed enum with a `none` sentinel, not a tag table.** Label literature
  (GitHub/Linear) reserves free-form tags for optional open-vocabulary multi-value
  annotation; a single-valued, closed, high-frequency-filtered axis like "which command
  produced this" should be a typed field. The `plan` vs `implement` split (created up front
  vs surfaced during implementation) is the load-bearing distinction. *Impact:* `origin`
  enum `plan|implement|review|optimise|tdd|human|none` on work_items, findings, activity,
  research_notes. Grade: MEDIUM.
- **Research notes → first-class records with confidence + accept/reject + supersession.**
  ADR practice never deletes the loser — it marks `superseded-by` so the chain is auditable;
  KG-fusion attaches a confidence + provenance to every fact. The project's own
  high/medium/low evidence-grade is more reproducible than a 0–1 score. *Impact:*
  `research_notes` table with `confidence`, `state ∈ {proposed,accepted,rejected}`,
  `rationale`, `lens`, `origin`, and a self-FK `superseded_by`. **`findings` carry the same
  weighting/supersession pair (`confidence` + `superseded_by`)** — distinct scope: findings
  are tied to in-place implementation/tasks, research notes are generic to the work. Grade:
  MEDIUM.
- **Open questions → first-class child entity with an option→branch mechanism.** ADR/RFC
  "open questions" have a real lifecycle (open→answered, folded into a decision) and a
  blocking relationship; an immutable activity row can't carry mutable status. *Impact:*
  `open_questions` (story-scoped, status lifecycle, `prompting_finding_id`/`prompting_note_id`
  back-links) + `question_options`; tasks gain `blocked_by_question_id` and
  `enabling_option_id` (set only when the task is exclusive to that branch). Resolving =
  pick an option → unblock that branch, cancel the other branches' exclusive tasks. Grade:
  MEDIUM.
- **Cross-cutting (pin, don't necessarily build):** new child writes need the same
  idempotency discipline `findings` columns imply; the attribute read-modify-merge is a
  lost-update race under future multi-writer Postgres (latent — SQLite serializes today).
  Grade: HIGH (grounded in current schema).

## User Decisions

> Captured from the Phase-4 directed questions (this session). Treated as design data.

1. **Dependency edges** → **defer all dependency work** to a dedicated later plan. This
   plan is purely the record-richness axes; it lays no `depends_on`/`related` edges and no
   cycle/topo logic.
2. **Relevance scope** → **epic + feature + story** carry the relevance axis (task and
   project do NOT). Relevance is structural context; tasks are selected for a sprint via
   `status` + relations under `active` ancestors, never via their own relevance.
3. **Closure gate** → **configurable per story**: a story-level `closure_gate ∈
   {hard,soft}` flag decides whether tasks under it reject a `→done` transition while
   acceptance criteria remain unchecked (`hard`) or merely flag it (`soft`, default).
4. **Complexity grading** → **distinct scale → model tier**: `complexity ∈
   {low,medium,high}` (drives model assignment) is a separate column from `effort ∈
   {S,M,L}` (drives batch sizing; wire form is lowercase `s/m/l` — `S/M/L` is display-only).

### Phase 5 outcome

_Skipped — every Phase-4 answer's key terms (relevance levels, closure gate, complexity
scale, dependency deferral) are design choices fully covered by Research Notes and the
prior design-research pass; no unresearched library/API surfaced._

## Approach

**Storage — columns for cross-cutting/queryable, child tables for tight history/lifecycle**
(unchanged hybrid principle). The composer filters/sorts on `relevance`, `effort`,
`complexity`, `origin`, so those are **real typed columns** (validated in the repo, free
TEXT in the DB, mirroring how Plan 1 typed `Status`/`Severity`). Acceptance criteria,
research notes, and open questions are **FK child tables** (CASCADE, per-item `seq`,
`UNIQUE(parent,seq)` — the `work_item_activity` idiom) because each is append-or-lifecycle
state that folds onto the owning record. `WorkItemDetail` folds the three new collections
exactly as it folds `activity` today.

**Relevance** (`active|backlog|deferred|rejected`) — settable only on epic/feature/story
(repo rejects task/project with a typed `Validation`); `create_work_item` defaults a new
epic/feature/story to `backlog`, leaves task/project `NULL`.

**Acceptance criteria + closure gate.** `acceptance_criteria(work_item_id, seq, text,
checked, checked_at, checked_by)`. `check_acceptance_criterion` flips `checked` and appends
a `verification` activity entry (the immutable audit). The `update_work_item_status` repo fn
(which the `transition_status` MCP tool wraps; `repo.rs:525`, today a blind UPDATE with no
kind/parent read) gains a `→done` guard: on a **task** it reads the parent story's
`closure_gate`: `hard` ⇒ reject (`Validation`) if any criterion is unchecked; `soft` ⇒ allow
but the unchecked count surfaces in detail/export. `closure_gate` is a story-only column
defaulting to `soft`. If a task's immediate `parent_id` is not a story (the hierarchy can
nest a task under a feature/epic), the gate is inert (treated as `soft`) — no multi-level
ancestor walk.

**Research notes** — `research_notes(work_item_id, seq, summary, body, confidence, state,
rationale, lens, origin, superseded_by)`. `state` lifecycle `proposed→accepted|rejected`
with a `rationale`; `superseded_by` is a self-FK for the supersession chain (live notes =
`WHERE superseded_by IS NULL`). Humans and agents both write/curate. **Findings get the same
`confidence` + `superseded_by` pair** (and the live-findings fold filters `superseded_by IS
NULL`) — the mechanism is identical, but findings are tied to in-place implementation/tasks
where research notes are generic to the work.

**Open questions + branch tasks** — `open_questions(story_id, seq, question, status, answer,
chosen_option_id, decided_at, decided_by, prompting_finding_id?, prompting_note_id?)` +
`question_options(question_id, seq, label, detail?)`. A task gains `blocked_by_question_id`
(status `blocked` while the question is open) and `enabling_option_id` (set **only when the
task is exclusive to that branch**; non-exclusive tasks left NULL). `resolve_open_question`
is **propose-and-confirm**: select an option → set `status=answered`, `chosen_option_id`;
unblock the chosen branch's tasks (`blocked→todo`); flip the other branches'
exclusive tasks to `status=cancelled`. (No task-level relevance involved.)

**Origin** — `origin` enum on work_items/findings/activity/research_notes, defaulting
`NULL`; `CreateWorkItemRequest`/`add_finding`/`record_task_activity` accept it so the
writing command stamps `plan`/`implement`/`review`/… (or `none` for the long tail).

**Reuse:** mirror `0002`'s child-table DDL + the single-mutation-path + `record_event` +
`rows_affected()==0 ⇒ NotFound` + `normalise_object` + the `toml::Table::try_from(&detail)`
export idiom verbatim. New reads route through repo fns so `.sqlx/` regen stays single-point.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo test --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
```

Additional gates (acceptance, not the standard triplet):
- `cd lumina && cargo sqlx prepare --check` — fails if the committed `.sqlx/` cache is stale
  after the new `query!` macros (benign "potentially unused queries" warning is EXPECTED).
- `cargo audit --file lumina/Cargo.lock` — RUSTSEC check (release cadence).
- **No `npm run build`** — Plan 1.5 makes no web changes.

## Tasks

> Waved so no parallel batch shares a file. ≤3 agents/wave. Build/test run with
> `SQLX_OFFLINE=true` until the dev DB is migrated (per Plan 1's note).

### Wave A — schema + domain (foundation)

#### 1. Additive migration 0003 (columns + four child tables) [M]
- **Files:** `lumina/migrations/0003_planning_and_decisions.sql`
- **Depends on:** —
- **Action:** `ALTER TABLE work_items ADD COLUMN` for `relevance`, `effort`, `complexity`,
  `origin`, `closure_gate`, `blocked_by_question_id`, `enabling_option_id` (all nullable
  TEXT). `ALTER TABLE findings ADD COLUMN origin TEXT;`, `ALTER TABLE findings ADD
  COLUMN confidence TEXT;`, `ALTER TABLE findings ADD COLUMN superseded_by TEXT REFERENCES
  findings(id);`, and `ALTER TABLE work_item_activity ADD COLUMN origin TEXT;` (so
  `record_task_activity` can stamp origin per Task 5). `CREATE TABLE` for
  `acceptance_criteria`, `research_notes`, `open_questions`, `question_options` — each with
  the `0002` child-table idiom (`id` PK, parent FK `ON DELETE CASCADE`, `seq INTEGER NOT
  NULL`, `created_at` default, `UNIQUE(parent_id, seq)`); `research_notes.superseded_by TEXT
  REFERENCES research_notes(id)`; `open_questions.story_id REFERENCES work_items(id)` +
  `prompting_finding_id REFERENCES findings(id)` + `prompting_note_id REFERENCES
  research_notes(id)`; `question_options.question_id REFERENCES open_questions(id) ON DELETE
  CASCADE`. Indexes on each child's `(parent_id, seq)`.
- **Detail:** `PRAGMA foreign_keys = ON;` at top is **consistency-only** — FKs are actually
  enforced per-connection via `SqliteConnectOptions::foreign_keys(true)` (`db.rs:30`), so the
  CASCADE acceptance test depends on the pool option, not this PRAGMA. The self-FK
  `ADD COLUMN ... REFERENCES` columns (`findings.superseded_by`, `research_notes.superseded_by`)
  are legal **only** with their implicit NULL default — do NOT add a non-NULL `DEFAULT` or the
  migration aborts (SQLite restriction on adding a REFERENCES column under `foreign_keys=ON`).
  Plain TEXT (Postgres-port comment). No new JSON columns ⇒ no new validity triggers.
  `ADD COLUMN` leaves existing rows NULL (= unset).
- **Acceptance:** `sqlx migrate run` applies 0003 cleanly to a fresh DB; a test asserts
  (a) deleting a work_item cascades its acceptance_criteria/research_notes/open_questions,
  (b) deleting an open_question cascades its question_options, (c) `UNIQUE(parent,seq)` holds.
  Create the test rows parent-first (research_note before its referencing open_question,
  open_question before its options) so the insert-time FK check is actually exercised.

#### 2. Domain structs + typed enums [L]
- **Files:** `lumina/src/domain.rs`
- **Depends on:** 1
- **Action:** Add read structs (Serialize-only) `AcceptanceCriterion`, `ResearchNote`,
  `OpenQuestion` (with nested `options: Vec<QuestionOption>`), `QuestionOption`. Add enums
  (Deserialize+Serialize+JsonSchema, `rename_all="snake_case"`) `Relevance`
  (active/backlog/deferred/rejected), `Effort` (s/m/l), `Complexity` (low/medium/high),
  `Origin` (plan/implement/review/optimise/tdd/human/none), `ResearchState`
  (proposed/accepted/rejected), `QuestionStatus` (open/answered/cancelled), `ClosureGate`
  (hard/soft). Extend `WorkItem` with `relevance/effort/complexity/origin/closure_gate/
  blocked_by_question_id/enabling_option_id` (all `Option<…>`); extend `Finding` with
  `origin`, `confidence`, `superseded_by` (all `Option<String>`); extend `WorkItemDetail`
  with `acceptance_criteria`, `research_notes`, `open_questions`. Add the partial-update
  request structs needed by Task 3/4 (e.g. `UpdateResearchNoteRequest`); extend the
  existing `UpdateFindingRequest` with `confidence`.
- **Detail:** `///`-doc each enum/request field (becomes JSON-schema description). Keep read
  structs Serialize-only (no JsonSchema) per Plan 1's deferred decision. **Declaration order
  matters for export:** every new scalar field on `WorkItem`/`Finding`/`OpenQuestion` MUST
  precede any `Vec`/nested-struct field, or `toml::to_string_pretty` (Task 7) fails at runtime
  with `ValueAfterTable` (TOML's tables-last rule). `Effort` wire form
  is `s|m|l` (lowercase snake) — note the divergence from the plan-doc `S/M/L` display.
- **Acceptance:** `cargo build` compiles; a unit test round-trips each new enum through
  serde (snake_case) and asserts the `Relevance` schema lists all four variants (reuse the
  Plan-1 `collect_schema_variants` helper shape).

### Wave B — repository write-paths (sequential; both own repo.rs)

#### 3. Repo part 1 — columns, acceptance criteria, closure gate [L]
- **Files:** `lumina/src/repo.rs`
- **Depends on:** 2
- **Action:** Under the single-mutation-path + `record_event` discipline: `set_relevance(id,
  Relevance)` (reject on task/project kind → `Validation`); `set_effort`/`set_complexity`
  (task scope); accept `origin` on `create_work_item` (default a new epic/feature/story
  `relevance="backlog"`); `set_closure_gate(story_id, ClosureGate)` (story scope);
  acceptance-criteria CRUD: `add_acceptance_criterion(work_item_id, text)->Uuid` (seq=MAX+1),
  `check_acceptance_criterion(id, by?)` / `uncheck_acceptance_criterion(id)` — a check also
  `append_activity(entry_kind="verification", …)`; `remove_acceptance_criterion(id)`. Wire the
  **closure gate** into `update_work_item_status` (the repo fn the `transition_status` MCP
  tool wraps; `repo.rs:525`, which gains its first read-before-write): when target is `done`
  and the item is a `task`, read parent story's `closure_gate`; `hard` + any unchecked
  criterion ⇒ `Validation`. **Also guard the HTTP PATCH path** — `update_work_item`
  (`repo.rs:600`, `SET status = COALESCE(?4, status)`) can today set `status="done"` directly
  and would bypass the gate; either route its task-status writes through the gated path or
  explicitly document the generic PATCH as gate-exempt. Fold
  `acceptance_criteria` into `get_work_item_detail`.
- **Detail:** Validate enums via the Task-2 types (typed `Validation`, not panic). `NotFound`
  via `rows_affected()==0` before any event. Do the `.sqlx` regen in Task 4 (single point).
- **Acceptance:** repo tests prove: `set_relevance` on a task returns `Validation`; a `hard`
  story blocks task→done with an unchecked criterion and allows it once all checked; checking
  a criterion writes +1 activity (`verification`) + the criterion state + one event; soft
  story allows done with unchecked criteria; detail folds the criteria.

#### 4. Repo part 2 — research notes, open questions, branch resolution + .sqlx regen [L]
- **Files:** `lumina/src/repo.rs`, `lumina/.sqlx/` (regenerated)
- **Depends on:** 3
- **Action:** `add_research_note(work_item_id, …)->Uuid`; `update_research_note(id,
  &UpdateResearchNoteRequest)` (confidence/state/rationale/lens); `supersede_research_note(old,
  new)` (set `superseded_by`). **Findings weighting/supersession (axis B):** accept
  `confidence` on `create_finding`/`update_finding`; add `supersede_finding(old, new)` (set
  `findings.superseded_by`); filter superseded findings (`superseded_by IS NULL`) from the
  `get_work_item_detail` findings fold — same mechanism as research notes (findings are tied
  to in-place implementation/tasks; research notes are generic).
  `add_open_question(story_id, question)->Uuid` (reject non-story
  → `Validation`); `add_question_option(question_id, label, detail?)->Uuid`;
  `block_task_on_question(task_id, question_id)` (set FK + `status=blocked`);
  `set_enabling_option(task_id, option_id)` (exclusive-branch tie);
  `resolve_open_question(question_id, chosen_option_id, by?)` — set `status=answered` +
  `chosen_option_id`; chosen branch's blocked tasks → `todo`; other branches' tasks whose
  `enabling_option_id` ≠ chosen → `status=cancelled`. Fold `research_notes` (live, i.e.
  `superseded_by IS NULL` ordering) and `open_questions`(+`options`) into
  `get_work_item_detail`. **Regenerate `.sqlx/` with `cargo sqlx prepare -- --all-targets`**
  (covers all Task-3 + Task-4 macros) and confirm `--check` clean.
- **Detail:** `resolve_open_question` is several writes in one transaction (the question + N
  task transitions) — keep it one tx and emit **exactly one `open_question.resolved` event**
  for the whole resolution (NOT per-task events), so the +1-event invariant stays auditable.
  This task is large: if splitting, **4a** = research-notes + findings confidence/supersede +
  their folds; **4b** = open-questions/options/branch-resolution + folds + the single `.sqlx`
  regen (4a issues no `sqlx prepare` — the regen stays single-point in 4b, preserving Wave-C
  parallel-safety).
- **Acceptance:** repo tests prove: `add_open_question` on a non-story → `Validation`;
  resolving a question with two option-branches unblocks the chosen branch's task (`→todo`)
  and cancels the other branch's exclusive task (`→cancelled`); a superseded research note is
  excluded from the live detail fold; resolving emits exactly one `open_question.resolved`
  event (a `count_events` assertion proves the +1-event invariant holds across the
  multi-write); `cargo sqlx prepare --check` clean.

### Wave C — entry points (parallel after Wave B; disjoint files, NO new query! macros)

#### 5. MCP domain tools for the new surface [L]
- **Files:** `lumina/src/mcp.rs`
- **Depends on:** 4
- **Action:** Add `#[tool]` methods (own `Parameters<T>` structs, Task-2 enums, annotations)
  mapping 1:1 to the new repo fns: `set_relevance`, `set_effort`, `set_complexity`,
  `set_closure_gate`, `add_acceptance_criterion`, `check_acceptance_criterion`,
  `uncheck_acceptance_criterion`, `add_research_note`, `update_research_note`,
  `supersede_research_note`, `add_open_question`, `add_question_option`,
  `block_task_on_question`, `set_enabling_option`, `resolve_open_question`, `supersede_finding`.
  Add `origin` to the existing `create_work_item`/`add_finding`/`record_task_activity` param
  structs, and `confidence` to `add_finding`/`update_finding`.
- **Detail:** Annotate setters/idempotent writes `idempotent_hint`, reads none-added.
  `open_world_hint=false`. Reuse `app_error_to_mcp`; writes → `structured({id})`.
  **Issue NO new `query!` macros** — route through Task-3/4 repo fns (keeps `.sqlx/`
  single-point so Wave-C stays parallel-safe).
- **Acceptance:** `#[tokio::test]` asserts the new tool names + annotations are advertised
  (extend the existing tool-advertisement membership list in `mcp.rs` — the `for expected in
  [...]` check — with the ~16 new tool names); a `resolve_open_question` tool call performs
  the branch unblock/cancel; an illegal `relevance` enum value is rejected `invalid_params`.

#### 6. HTTP read fold for the new collections [S]
- **Files:** `lumina/src/http.rs`
- **Depends on:** 4
- **Action:** Confirm `GET /api/work-items/{id}` surfaces the new `WorkItem` columns +
  `WorkItemDetail` child collections (largely free via struct serialization — verify no
  handler reshapes/strips; add code only if it does). No new write endpoints.
- **Detail:** Issue NO new `query!` macro. Both entry points keep calling the same repo fns.
- **Acceptance:** a handler test shows detail returns `acceptance_criteria`,
  `research_notes`, `open_questions`, and the new columns (`relevance`/`effort`/etc.).

#### 7. Git-export fold for the new collections [S]
- **Files:** `lumina/src/export.rs`
- **Depends on:** 4
- **Action:** Confirm `render_work_item`'s `toml::Table::try_from(&detail)` carries the new
  columns + child collections into the snapshot (free via whole-struct serialize — verify;
  the new fields are scalars/objects so no null/scalar-root TOML hazard — but honour Task 2's
  declaration-order constraint, and add a round-trip export test over a `WorkItemDetail`
  carrying a populated `open_questions`(+`options`) to gate the tables-last ordering). No
  tombstone change.
- **Detail:** Issue NO new `query!` macro / no second `.sqlx/` regen.
- **Acceptance:** an export test after adding a research note + acceptance criterion shows
  both round-trip in the snapshot TOML; a story snapshot carries `relevance`.

### Wave D — docs + end-to-end

#### 8. End-to-end test + CLAUDE.md + SKILL.md [M]
- **Files:** `lumina/tests/e2e.rs`, `CLAUDE.md`, `claude/skills/lumina/SKILL.md`
- **Depends on:** 5, 6, 7
- **Action:** Extend the in-process e2e thread: create a story (relevance), add acceptance
  criteria + check them under a `hard` story and assert task→done is gated then allowed;
  add a research note + accept it; add an open question with two options + a branch task per
  option, resolve → assert chosen-branch task `todo` and other-branch task `cancelled`;
  drain `export_pending` and assert the new fields in the snapshot; `GET` the detail over
  HTTP. Add new in-process `Parameters<T>` drive helpers. Update CLAUDE.md's lumina section
  (including the "~17 tools" count, which roughly doubles, and the inline tool catalogue)
  and SKILL.md's tool catalogue with the new tools.
- **Acceptance:** `cargo test` (incl. the deterministic, sleep-free e2e) passes; `cd lumina
  && cargo sqlx prepare --check` clean; SKILL.md lists every new tool; CLAUDE.md documents
  the new surface.

## Dependency Graph

```
Wave A:  1 ── 2
Wave B:       2 ── 3 ── 4        (3,4 sequential — both own repo.rs; 4 does the .sqlx regen)
Wave C:            4 ──┬── 5     (mcp.rs)
                       ├── 6     (http.rs)
                       └── 7     (export.rs)   (5,6,7 parallel — disjoint files, no new query!)
Wave D:        (5,6,7) ── 8
```
Critical path: 1 → 2 → 3 → 4 → 5 → 8. Parallel-safety in Wave C holds only while 5/6/7 issue
no new `query!` macros (each task pins this) — the single `.sqlx/` regen lives in Task 4.

## Verification

1. **Build/lint/test** — the three Verification Commands pass on `lumina/`.
2. **Migration** — `0003` applies to a fresh DB; cascades + `UNIQUE(parent,seq)` hold (Task 1).
3. **Relevance** — settable on epic/feature/story, rejected on task/project (Tasks 3, 8).
4. **Closure gate** — `hard` story blocks task→done with unchecked criteria; `soft` allows
   (Tasks 3, 8).
5. **Research notes & findings** — accept/reject + weighting (`confidence`) + supersession;
   superseded entries drop from the live fold (Task 4, 8).
6. **Open questions** — resolve selects an option, unblocks the chosen branch, cancels the
   other branches' exclusive tasks (Tasks 4, 8).
7. **Reads/export** — detail + snapshot carry the new columns + collections (Tasks 6, 7, 8).
8. **Offline cache** — `cargo sqlx prepare --check` clean (Tasks 4, 8).

## Risks

- **`.sqlx/` offline-cache drift** — Tasks 3+4 add many `query!` macros; the regen
  (`-- --all-targets`) + committed `.sqlx/` lives in Task 4 only; Tasks 5/6/7 issue no new
  macros so the regen is single-point. *Mitigation:* `--check` gate in Task 8.
- **`resolve_open_question` is multi-write** — it transitions the question plus N tasks in
  one logical operation, stretching the "one domain write per tx" rule. *Mitigation:* keep
  it one transaction emitting **exactly one** `open_question.resolved` event (not per-task) so
  the +1-event invariant stays auditable; a `count_events` assertion + e2e assert the branch
  outcome.
- **Column sprawl on `work_items`** — seven new nullable columns. *Mitigation:* all are
  cross-cutting/queryable (composer-facing) so they earn columns per the hybrid principle;
  narrative one-offs stay in `attributes`. These composer-facing filter columns
  (`relevance`/`effort`/`complexity`/`origin`) are intentionally **unindexed** here (only the
  child `(parent_id,seq)` indexes are added) — negligible at current scale; index choice is
  deferred to the composer plan when the query shapes are known.
- **Pre-existing `Status`/`Severity` divergences left unfixed** — `create_work_item` still
  seeds `"open"` (not a `Status` member) and `Severity` diverges from the ledger vocab.
  *Mitigation:* explicitly out of scope (fixing risks existing data + the webui status list);
  flagged here so the next plan/importer addresses it deliberately.
- **Migrating the live dev DB** — `0003` `ADD COLUMN` is non-destructive but the running
  server holds `lumina.db` open. *Mitigation:* stop the server before `migrate run`, or
  migrate a copy; acceptance tests use fresh/in-memory DBs regardless.
