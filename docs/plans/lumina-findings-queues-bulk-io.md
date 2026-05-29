# Plan: lumina — review/optimise findings queues + bulk-efficient atomic IO

**Plan path**: docs/plans/lumina-findings-queues-bulk-io.md
**Created**: 2026-05-29
**Status**: draft
**Plan review**: round 1 applied 2026-05-29 (all 20 findings folded in; see User Decisions D5–D12 and Risks)

## Context

lumina's MCP/HTTP surface is single-item-per-call on every write. Driving review/optimise findings and task/story creation through it means the fan-out lands at the *agent* layer — N tool calls = N model round-trips = N transactions, with no cross-item atomicity (fail at call 47/100 → 46 committed rows, partial state). tomlctl solves the same problem at the *tool* layer (`items add-many`, `items apply`, `items list --count-by`): one invocation, one atomic write, push-down aggregation. This plan translates tomlctl's bulk-and-aggregate **contract** into lumina's relational/MCP form — not its in-memory scan logic, since SQLite is the query engine.

Two coupled needs:
1. **Bulk efficiency + atomicity + controlled edits** — typed, domain-shaped batch tools (`add_findings`, `create_work_items`, controlled `batch_update_findings`) that run as one `BEGIN IMMEDIATE` transaction (all-or-nothing), plus bounded structured query/aggregation tools (`query_findings`). Dedup-on-insert reuses the existing-but-unpopulated `findings.dedup_id`.
2. **A new domain model** — review/optimise findings live in **story-level queues**. A first-class **run** (review|optimise) targets a completed **sprint** (now persisted via a task junction) or a story's progress. Findings are triaged into decisions (spawn task / spawn story / defer / dismiss / resolve) with full provenance (a backlink FK on the spawned item **and** a `finding_decisions` audit log).

Git-export coupling is **dropped** for bulk paths (export reserved for ad-hoc calls), so batch writes emit **one coarse event** (stamped with a non-`work_item` aggregate type) instead of per-row events.

## Scope

**In scope:**
- Migration `0011` carrying all new schema (one file, up front).
- `repo.rs` extraction of **2** inner `*_tx(&mut tx, …)` helpers (`create_finding_tx`, `create_work_item_full_tx`) + content-hash helper, plus the struct/SELECT updates the new columns force.
- Batch write tools: `add_findings`, `create_work_items` (under **existing** parents), `batch_update_findings` (non-terminal triage-field edits).
- Structured read tools: `query_findings` (bounded filters + a `severity` aggregation axis), `get_story_finding_queue`.
- Domain tools: `create_run`, `create_sprint`, `add_tasks_to_sprint`, `record_finding_decision`.
- All tools mirrored on MCP **and** HTTP `/api`; tests; docs.

**Out of scope (follow-ups):**
- **Inline `depends_on` in `create_work_items`** (deferred per D10 — server mints UUIDs, so batch-local refs need a temp-key scheme not worth v1; wire edges afterward via `block_task_on_task`). Consequently `create_work_items` references only **existing** parents — no batch-local parent/child chains.
- Folding `runs`/`sprints`/`finding_decisions` into `WorkItemDetail` (read-tool-only per D11; only the `spawned_from_finding_id` column rides the `WorkItem` struct).
- Rebuilding the dynamic sprint composer — ship sprint **persistence primitives** only.
- `target_kind='focus'` runs (queue fan-out ambiguity) — ship `sprint` + `story` targeting first.
- A generic ops-applier / SQL gateway — rejected; typed domain-shaped tools only.
- An export renderer for batch/run/sprint events — the drain (export.rs:139) renders only `aggregate_type="work_item"`; others drain inert.
- `set_work_item_attributes_tx` extraction — `batch_update_findings` is findings-only, so not needed.

**Affected areas:** lumina/migrations/, lumina/src/repo.rs, lumina/src/mcp.rs, lumina/src/domain.rs, lumina/src/http/, lumina/src/export.rs, lumina/Cargo.toml, lumina/.sqlx/, lumina/tests/, lumina/CLAUDE.md, CLAUDE.md, claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md

**Estimated file count:** ~13 unique files (repo.rs, mcp.rs, domain.rs each touched across multiple waves — they serialize; see Risks).

## User Decisions

| # | Question | Decision | Prompting finding |
|---|----------|----------|-------------------|
| 1 | Run a first-class entity? | **Yes — `runs` table** + `findings.run_id` FK | No run/queue entity exists; findings carry only `origin` |
| 2 | How does a run target a "completed sprint"? | **Persist sprints via a task junction** (`sprints` + `sprint_tasks`); run targets a sprint or a story; story/detail derived via `task.parent_id` | Sprints are dynamic compositions today, not persisted |
| 3 | Finding→spawned-work provenance? | **Both** — `work_items.spawned_from_finding_id` FK *and* a `finding_decisions` audit table | `open_questions.prompting_finding_id` is the only precedent |
| 4 | Dedup scope? | **Per target work_item** — `UNIQUE(work_item_id, dedup_id) WHERE dedup_id IS NOT NULL AND superseded_by IS NULL` | `findings.dedup_id` exists but unpopulated |

**Derived / review-resolved decisions:**
- **D5 — `sha2` is already in the lockfile** (was framed as a new dependency). `sha2 0.10.9`/`0.11.0` already resolve transitively via sqlx/rmcp (`Cargo.lock:2196`); `sha2 = "0.10"` as a direct dep adds zero crates. No RUSTSEC advisory affects 0.10.x. Content-hash over `(work_item_id, file, line, symbol, summary)`. *(review P19)*
- **D6 — `triage_state` denormalized column** on `findings` (`pending|accepted|dismissed`, `DEFAULT 'pending'`), maintained in the same tx as the `finding_decisions` insert. It is **orthogonal to `status`** (which carries the terminal `Disposition`): triage = queue management, status = terminal disposition. *(review P5; precedent: `open_questions.status`)*
- **D7 — queue scoping**: `get_story_finding_queue(story_id)` scopes by the finding's **own `work_item_id`** (the story, or a task whose `parent_id` is the story), with run/sprint as a secondary filter; one static JOIN that filters `work_items.deleted_at IS NULL` (tombstoned items hidden — D-confirmed per P16).
- **D8 — coarse batch event**: each batch emits one event (e.g. `findings.batch_added {count, skipped, run_id}`) stamped with a **non-`work_item` `aggregate_type`** (e.g. `"run"`) so the export drain (export.rs:139, renders only `work_item`) does not spuriously re-render. Single-item tools keep per-row events. *(review P4)*
- **D9 — `batch_update_findings` scope**: `triage_state` + `severity` + `category` + a **non-terminal** `status` only. It must NOT write a terminal `Disposition`; terminal transitions go through `resolve_finding`. `record_finding_decision(resolve)` delegates to `resolve_finding` semantics (stamps `resolved_at`). *(review P5)*
- **D10 — inline `depends_on` DEFERRED**: `create_work_items` bulk-creates items under **existing** parents (no batch-local parent/child or dependency refs); dependency edges are wired afterward via `block_task_on_task`. Removes the cycle-check concern from the batch path. *(review P1, P3)*
- **D11 — new columns ride the read structs**: `run_id`/`triage_state` are added to the `Finding` struct + `list_findings` SELECT; `spawned_from_finding_id` is added to the `WorkItem` struct + both work_items SELECTs + both hand-construction sites + the `export.rs` test fixture (so provenance is visible in `GET /api/work-items/{id}` + export). `runs`/`sprints`/`finding_decisions` are **read-tool-only** (NOT folded into `WorkItemDetail`). *(review P2, P7)*
- **D12 — query shapes pinned**: `enum FindingAxis { Severity }` (v1) and `struct AxisCount { key: String, count: i64 }`; `query_findings` returns rows or `Vec<AxisCount>`. *(review P13)*

## Research Notes

_In-codebase extension; no external library research drove the design. Versions verified during plan review: sqlx **0.9**, rmcp **1.7**, axum **0.8**, schemars **1**, uuid **1 (v7)** (`lumina/Cargo.toml`). `sha2 0.10.9` already in `Cargo.lock`. SQLite confirmed: `INSERT … ON CONFLICT(c1,c2) WHERE <literal predicate> DO NOTHING` against a partial unique index requires the conflict target to **repeat the index's WHERE predicate verbatim** (and the predicate must be a literal, no placeholders); `rows_affected()` returns 0 on a DO NOTHING skip; `ADD COLUMN … DEFAULT '<const>'` is legal, `ADD COLUMN … REFERENCES` requires a NULL default._

## Approach

### Schema (migration 0011, all up front) — concrete DDL

```sql
CREATE TABLE runs (
    id          TEXT PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN ('review','optimise')),
    target_id   TEXT NOT NULL,                              -- soft pointer, resolved by target_kind
    target_kind TEXT NOT NULL CHECK (target_kind IN ('sprint','story')),
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','triaged','closed')),
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE sprints (
    id          TEXT PRIMARY KEY,
    title       TEXT,
    status      TEXT NOT NULL DEFAULT 'open',
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE sprint_tasks (
    sprint_id   TEXT NOT NULL REFERENCES sprints(id),
    task_id     TEXT NOT NULL REFERENCES work_items(id),
    PRIMARY KEY (sprint_id, task_id)
);
CREATE TABLE finding_decisions (
    id                   TEXT PRIMARY KEY,
    finding_id           TEXT NOT NULL REFERENCES findings(id),
    decision             TEXT NOT NULL CHECK (decision IN ('spawn_task','spawn_story','defer','dismiss','resolve')),
    spawned_work_item_id TEXT REFERENCES work_items(id),
    decided_by           TEXT,
    decided_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
ALTER TABLE findings   ADD COLUMN run_id       TEXT REFERENCES runs(id);          -- nullable (ADD-COLUMN-REFERENCES rule)
ALTER TABLE findings   ADD COLUMN triage_state TEXT DEFAULT 'pending';            -- constant default legal
ALTER TABLE work_items ADD COLUMN spawned_from_finding_id TEXT REFERENCES findings(id);
CREATE UNIQUE INDEX ux_findings_dedup ON findings(work_item_id, dedup_id)
    WHERE dedup_id IS NOT NULL AND superseded_by IS NULL;
```
`findings.dedup_id` already exists (migration 0001) — only the index + hash population are new. `target_id` is a soft pointer (same `aggregate_type`/`aggregate_id` idiom events use); `create_run` validates it (P6).

### Repo transaction layer
Follow the `resolve_open_question` template (multiple statements + one event in one `begin_write` tx). Extract **2** inner helpers:
- `create_finding_tx(&mut tx, …)` — clean extraction (no pre-tx validation today); now also binds `dedup_id` (computed pre-tx) + `triage_state` default, and uses `INSERT … ON CONFLICT(work_item_id, dedup_id) WHERE dedup_id IS NOT NULL AND superseded_by IS NULL DO NOTHING` (predicate repeated verbatim, literal).
- `create_work_item_full_tx(&mut tx, …)` — validation reads against `&mut *tx`. Because `create_work_items` references only **existing** (committed) parents (D10), a fresh tx sees them identically — the move is mechanical, low-risk, and does NOT enable batch-local parentage.

`add_task_dependency_tx` is **not** extracted (inline deps deferred, D10). Public single-item fns keep identical signatures.

### Struct/SELECT updates the columns force (D11)
- `Finding` struct (domain.rs) gains `run_id`, `triage_state`; `list_findings` `query_as!(Finding, …)` SELECT (repo.rs:754) gains the two columns. (Only `query_as!(Finding)` site.)
- `WorkItem` struct (domain.rs) gains `spawned_from_finding_id`; both `query!` SELECTs (repo.rs:~368, ~444) + both hand-construction sites (repo.rs:~404, ~474) + the `export.rs` test fixture gain the field.
- Regen `.sqlx` with `-- --all-targets` (the existing `create_finding` INSERT macro mutates).

### Batch write — dedup + atomicity
`add_findings` is a **loop of `create_finding_tx`** under one `begin_write`: compute `dedup_id` (D5) *before* the tx, then per row inspect `rows_affected()` (1 = added, 0 = skipped → push to `skipped_ids`). Validation error `?`-propagates → tx drops → zero writes. Returns `{added, skipped, skipped_ids}`. One coarse event (D8) at commit. A single multi-row INSERT is rejected — it can't attribute `skipped_ids`.

### Query — static macros only
`query_findings` filters use the `(?n IS NULL OR col = ?n)` static `query!` pattern (one `.sqlx` entry); the `severity` aggregation is a fixed `GROUP BY severity` `query!` dispatched by `count_by: Option<FindingAxis>` (D12). `QueryBuilder` is rejected. `get_story_finding_queue` is one static JOIN (`findings` ↔ `work_items`, `deleted_at IS NULL`) per D7.

### Domain tools + triage
`record_finding_decision` writes the `finding_decisions` row, updates `findings.triage_state`, and (for spawn decisions) stamps `work_items.spawned_from_finding_id` — one tx. For a `resolve` decision it delegates to `resolve_finding` (stamps `resolved_at`) (D9). `create_run` validates `target_id` exists, is live (`deleted_at IS NULL`), and matches `target_kind` (P6). `add_tasks_to_sprint` batches the junction with `ON CONFLICT DO NOTHING` and validates `kind='task'`.

### Dual surface
Each tool: repo fn → MCP `#[tool]` (params struct + **`schemars::JsonSchema` on every batch element struct**, not just the wrapper) → HTTP route. New families get a `http/<name>.rs` + one `.merge()` in `http/mod.rs` (edited once per phase).

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo test --manifest-path lumina/Cargo.toml      # or cargo nextest run …
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets
sqlx:  cd lumina && cargo sqlx prepare --check           # benign "unused queries" warning — expected exit 0 (project-observed on sqlx 0.9; confirm after the first 0.9 regen)
```
Regenerate the offline cache (with the installed **sqlx 0.9** CLI) whenever a wave adds OR mutates a `query!`/`query_as!` macro: `cd lumina && cargo sqlx prepare -- --all-targets`.

## Tasks

### Phase 1: Schema foundation (parallel)
#### T1: Write migration 0011 + promote sha2 + migration test
- **Files**: `lumina/migrations/0011_runs_sprints_findings_queue.sql` (new), `lumina/Cargo.toml`, `lumina/tests/migration_0011.rs` (new)
- **Action**: Write the migration exactly as the Approach §Schema DDL (tables + ALTERs + index, all column types/CHECKs/defaults pinned). Promote `sha2 = "0.10"` to a direct `[dependencies]` entry (resolves to the existing lockfile 0.10.9 — zero new crates). Add `tests/migration_0011.rs` mirroring `tests/migration_0010.rs`: assert each new table/column/index exists and a CHECK-violating insert is rejected. Header comment in the migration-0010 style (Why / forward-only / recovery).
- **Acceptance**: `cargo test --manifest-path lumina/Cargo.toml migration_0011` passes; `cargo build` applies migrations via `db::init` cleanly.
- **Blocked-by**: none
- **Effort**: M

#### T2: Add NEW domain structs + enums
- **Files**: `lumina/src/domain.rs`
- **Action**: Add NEW enums (`RunKind`, `RunStatus`, `TargetKind`, `FindingDecisionKind`, `TriageState`, `FindingAxis { Severity }`) and NEW structs (`BatchInsertResult{added, skipped, skipped_ids}`, `AxisCount{key, count}`, `QueryFindingsFilter`, `NewRun`, `NewSprint`, finding-decision input) — all with serde + `schemars::JsonSchema` + `enum_to_str` round-trip. Do NOT touch the existing `Finding`/`WorkItem` structs here (that's T3, so the crate stays compiling).
- **Acceptance**: `cargo build` compiles; enum wire-strings match the migration CHECK vocabularies.
- **Blocked-by**: none
- **Effort**: M

### Phase 2: Repo transaction refactor (after Phase 1)
#### T3: Extract `*_tx` helpers + content-hash + extend read structs
- **Files**: `lumina/src/domain.rs`, `lumina/src/repo.rs`, `lumina/src/export.rs`, `lumina/.sqlx/`
- **Action**: (a) Extract `create_finding_tx` (with dedup `ON CONFLICT` + `triage_state`) and `create_work_item_full_tx` (validation reads via `&mut *tx`); public fns delegate, signatures unchanged. (b) Add `fn finding_dedup_hash(...) -> String` (sha2, pre-tx). (c) Per D11: add `run_id`/`triage_state` to the `Finding` struct + `list_findings` SELECT; add `spawned_from_finding_id` to the `WorkItem` struct + both work_items SELECTs + both constructors + the `export.rs` test fixture. (d) `cargo sqlx prepare -- --all-targets` (the `create_finding` INSERT macro mutates).
- **Acceptance**: existing tests pass unchanged (no behaviour change in single-item paths); `cargo clippy` clean; `cargo sqlx prepare --check` passes.
- **Blocked-by**: T1, T2
- **Effort**: M

### Phase 3: Batch write tools (after Phase 2 — T4a/b/c serialize on repo.rs)
#### T4a: Repo `add_findings`
- **Files**: `lumina/src/repo.rs`
- **Action**: Loop `create_finding_tx` under one `begin_write`; per-row `rows_affected()` → `{added, skipped, skipped_ids}`; one coarse event with `aggregate_type="run"` (or `"finding"` when no run). Validation error aborts whole batch.
- **Acceptance**: unit tests incl. **dedup against a COMMITTED prior finding** (re-run skips it, count unchanged) and abort-on-validation; `.sqlx` regen + `--check`.
- **Blocked-by**: T3
- **Effort**: M

#### T4b: Repo `create_work_items`
- **Files**: `lumina/src/repo.rs`
- **Action**: Loop `create_work_item_full_tx` (existing parents only, NO inline `depends_on`); optional `spawned_from_finding_id` stamp per item; one coarse event. All-or-nothing.
- **Acceptance**: unit tests for bulk create under an existing story + provenance stamp + abort-on-validation.
- **Blocked-by**: T4a (repo.rs serialization)
- **Effort**: M

#### T4c: Repo `batch_update_findings`
- **Files**: `lumina/src/repo.rs`
- **Action**: Bulk-update `triage_state`/`severity`/`category`/non-terminal `status` (D9 — reject terminal disposition values); one coarse event.
- **Acceptance**: unit test; a terminal-status value is rejected as `Validation`.
- **Blocked-by**: T4b (repo.rs serialization)
- **Effort**: S

#### T5: MCP batch tools
- **Files**: `lumina/src/mcp.rs`
- **Action**: `#[tool]` handlers + `Parameters<T>` for the three batch fns; **every batch element struct derives `schemars::JsonSchema`**; map errors via `app_error_to_mcp`. Add an advisory batch-size note (≤~500 rows) to each tool description.
- **Acceptance**: `cargo build` + a params-deserialise unit test per tool (invalid enum → `invalid_params`). Behaviour proven in T13.
- **Blocked-by**: T4c
- **Effort**: M

#### T6: HTTP batch routes
- **Files**: `lumina/src/http/findings.rs`, `lumina/src/http/work_items.rs`, `lumina/src/http/mod.rs`
- **Action**: POST batch routes (body minus path id) → same `repo::*` fns.
- **Acceptance**: `oneshot` round-trip returns the batch result JSON.
- **Blocked-by**: T4c (parallel with T5 — distinct files)
- **Effort**: M

### Phase 4: Query / aggregation tools (after Phase 2; repo/mcp serialize after Phase 3)
#### T7: Repo query functions
- **Files**: `lumina/src/repo.rs`
- **Action**: `query_findings` (static `(?n IS NULL OR …)` filter + `severity` `GROUP BY` via `FindingAxis`, returning rows or `Vec<AxisCount>`) and `get_story_finding_queue(story_id)` (one static JOIN, `work_items.deleted_at IS NULL`).
- **Acceptance**: tests for filter combos + count-by + queue composition (tombstoned items excluded); `.sqlx` regen + `--check`.
- **Blocked-by**: T3 (serialize on repo.rs after T4c)
- **Effort**: M

#### T8: MCP query tools
- **Files**: `lumina/src/mcp.rs`
- **Action**: `query_findings` + `get_story_finding_queue` handlers.
- **Acceptance**: `cargo build` + params-deserialise test; behaviour in T13.
- **Blocked-by**: T7 (serialize on mcp.rs after T5)
- **Effort**: S

#### T9: HTTP query routes
- **Files**: `lumina/src/http/queries.rs` (new), `lumina/src/http/mod.rs`
- **Action**: GET routes with query-param extraction → the query fns.
- **Acceptance**: `oneshot` GET returns expected rows/aggregates.
- **Blocked-by**: T7 (parallel with T8)
- **Effort**: S

### Phase 5: Run / sprint / triage domain tools (after Phase 2; repo/mcp serialize after Phase 4)
#### T10: Repo domain functions
- **Files**: `lumina/src/repo.rs`
- **Action**: `create_run` (validate `target_id` exists + live + matches `target_kind`, else `Validation`), `create_sprint`, `add_tasks_to_sprint` (junction batch + `kind='task'` validation + `ON CONFLICT DO NOTHING`), `record_finding_decision` (decision row + `triage_state` update + `spawned_from_finding_id` stamp; `resolve` delegates to `resolve_finding`), one tx each.
- **Acceptance**: tests for target validation (reject wrong-kind/dangling/tombstoned), junction dedup, decision provenance + triage transition + resolve delegation; `.sqlx` regen + `--check`.
- **Blocked-by**: T3 (serialize on repo.rs after T7)
- **Effort**: L

#### T11: MCP domain tools
- **Files**: `lumina/src/mcp.rs`
- **Action**: `#[tool]` handlers for the four domain fns.
- **Acceptance**: `cargo build` + params-deserialise tests; behaviour in T13.
- **Blocked-by**: T10 (serialize on mcp.rs after T8)
- **Effort**: M

#### T12: HTTP domain routes
- **Files**: `lumina/src/http/runs.rs` (new), `lumina/src/http/sprints.rs` (new), `lumina/src/http/mod.rs`
- **Action**: POST routes for runs/sprints/sprint-tasks/finding-decisions.
- **Acceptance**: `oneshot` round-trips.
- **Blocked-by**: T10 (parallel with T11)
- **Effort**: M

### Phase 6: Tests + docs (after Phases 3–5)
#### T13: End-to-end + per-family tests
- **Files**: `lumina/tests/bulk_e2e.rs` (new), `#[cfg(test)]` blocks in the new/edited `http/*.rs`
- **Action**: Reuse the established harness — `db::connect_in_memory()`, MCP via direct method calls, HTTP via `tower::ServiceExt::oneshot`, DB asserts via runtime `sqlx::query_scalar` — and **skip the export drain** (git-export dropped for bulk). Cover dedup skip (committed prior), abort-on-validation, queue composition (tombstoned excluded), finding→decision→spawned-item provenance + resolve delegation, `create_run` target validation.
- **Acceptance**: `cargo test --manifest-path lumina/Cargo.toml` green.
- **Blocked-by**: T5, T6, T8, T9, T11, T12
- **Effort**: L

#### T14: Documentation
- **Files**: `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`
- **Action**: Add the new MCP tools + a new `### Findings/Runs/Sprints batch + query` HTTP route block (lumina/CLAUDE.md "HTTP routes"); document the findings-queue/run/sprint model + the dropped-export/coarse-event behaviour. **Reconcile the count drift**: root `CLAUDE.md` says "39 tools", `lumina/CLAUDE.md` says "Tool surface is now 58" — update BOTH count strings (and SKILL.md) to the new total.
- **Acceptance**: docs match the implemented surface; both count strings agree; route catalogue complete.
- **Blocked-by**: T5, T6, T8, T9, T11, T12 (parallel with T13)
- **Effort**: M

## Dependency Graph

```
T1 ─┐
T2 ─┴─> T3 ─> T4a ─> T4b ─> T4c ─> T7 ─> T10     (repo.rs critical path — serialized)
                         └> T5 ─> T8 ─> T11      (mcp.rs — serialized after T4c)
                         └> T6   T9    T12       (http/* — one task per phase; mod.rs edited once per phase)
   T4c─>T5,T6 ; T7─>T8,T9 ; T10─>T11,T12
   {T5,T6,T8,T9,T11,T12} ─> T13, T14
```
Note: T6/T9/T12 are NOT mutually parallel (each is the sole http task in its phase and each edits `http/mod.rs` once); they serialize naturally by phase.

## Verification
- [ ] `cargo build --manifest-path lumina/Cargo.toml`
- [ ] `cargo test --manifest-path lumina/Cargo.toml` (incl. `migration_0011` + `bulk_e2e`)
- [ ] `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- [ ] `cd lumina && cargo sqlx prepare --check`
- [ ] Manual: batch `add_findings` returns `{added, skipped, skipped_ids}` and re-run on the same work_item skips committed duplicates; `query_findings count_by=severity` → grouped `AxisCount`s; `record_finding_decision spawn_task` sets `spawned_from_finding_id` + `triage_state=accepted` + a `finding_decisions` row; `create_run` rejects a wrong-kind `target_id`.

## Risks
- **R1 — `repo.rs`/`mcp.rs`/`domain.rs` are single-file serialization points.** T4a→T4b→T4c→T7→T10 (repo) and T5→T8→T11 (mcp) cannot parallelize. Implement in wave order; Phases 3/4/5 are each a natural `/implement` run. Only migrations, per-family `http/*.rs`, tests, and docs parallelize. `http/mod.rs` is touched once per phase (no race, phase-gated).
- **R2 — `create_work_item_full_tx` validation move (pool→tx reads).** Now low-risk: `create_work_items` references only existing (committed) parents (D10), so a fresh tx sees them identically; no batch-local parentage. Existing test suite gates it.
- **R3 — Partial-index `ON CONFLICT`.** The conflict target MUST repeat `WHERE dedup_id IS NOT NULL AND superseded_by IS NULL` **verbatim and as a literal** (no placeholders), or SQLite silently fails to bind the index and inserts a duplicate with no error. T4a's dedup test MUST: (a) commit a finding, (b) re-run `add_findings` with the same tuple, (c) assert `skipped_ids` contains it AND the row count is unchanged — a return-value-only test passes against a mis-bound index.
- **R4 — `.sqlx` cache churn.** Every wave adding OR mutating a `query!` macro needs `cargo sqlx prepare -- --all-targets` (preserves test-only entries). **T3 is the first task to mutate a committed macro** (the `create_finding` INSERT) — its acceptance makes `--all-targets` explicit. Use the installed sqlx 0.9 CLI.
- **R5 — `sha2` is already in the lockfile (D5).** No-cost default-accept (not a vetoable risk); `sha2 = "0.10"` resolves to the existing 0.10.9. No RUSTSEC advisory affects 0.10.x.
- **R6 — Soft-delete in the queue JOIN.** `get_story_finding_queue` filters `work_items.deleted_at IS NULL` so findings on tombstoned stories/tasks are hidden (D7 decision: hide, not retain). `findings` has no `deleted_at` of its own — liveness there is via `superseded_by`/`resolved_at`.
- **R7 — Coarse events drain inert.** The export drain renders only `aggregate_type="work_item"`; `run`/`finding`-typed batch events are stamped `exported_at` with no file. Do NOT stamp a batch event with `aggregate_type="work_item"` (would re-render). No `*.batch_*` renderer is added.
- **R8 — Migration 0011 is forward-only and purely additive** (new tables + nullable ADD COLUMNs + index). It breaks no existing MCP/HTTP consumer, no committed `.sqlx` entry, and no existing test (verified — unlike the 0006 `dispatch`→`tier` removal). Rollback = `git revert` + recreate the gitignored dev DB.
