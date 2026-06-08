# Plan: Sprint-lifecycle & worktree substrate (layer 2)

**Plan path**: `docs/plans/sprint-lifecycle-worktree-substrate.md`
**Created**: 2026-06-08
**Status**: Draft
**Architecture**: layer 2 of [ADR-0002](../adr/0002-sprint-execution-architecture.md); commit/checkpoint provenance per [ADR-0003](../adr/0003-commit-checkpoint-provenance.md). Builds on layer 1 — the execution substrate (`docs/plans/eventual-leaping-metcalfe.md`, landed, status=review). The composer/overseer **engine** (layer 3) stays deferred; this plan builds none of it.

## Context

Layer 1 (`eventual-leaping-metcalfe`) landed the atomic pull-queue: `claim_next_task` / `release_task` / `renew_lease` / `complete_task` cascade / `get_sprint_quiescence`, migration `0013_team_execution.sql`, 6 MCP tools + HTTP mirrors. It deliberately left two seams for layer 2 (per ADR-0002 §Consequences and ADR-0003 §Consequences): (a) the claim's sprint-status guard treats *any non-terminal* sprint as runnable — the full sprint lifecycle + stricter guard was deferred; (b) the `work_items.checkpoint` flag, the claim-queue freeze, and the `task_commits` cross-reference were deferred entirely.

This plan makes the **Sprint** a fully-tracked lifecycle entity and introduces a first-class **Worktree** as the inter-sprint isolation + merge unit. Per the user's Phase-4 decisions, the model is: a worktree is **owned by exactly one sprint**, its status is **derived from the owning sprint** (no independent worktree status), and **follow-up sprints may target the same unmerged worktree** to fix it (chained via provenance) — so one worktree hosts a *chain* of sprints (implementation → optional review/fix) and merges once, minimising churn on merged worktrees. lumina records the worktree/merge lifecycle as a durable **audit/intent log**; git stays the source of truth and lumina never polices it. This is the substrate the deferred composer/overseer engine will later drive.

Since the stub was written, the codebase moved on: `joyful-singing-crane` split `repo.rs`/`mcp.rs`/`domain.rs`/`http/` into submodule directories (so independent additions now parallelise far better than layer-1's single-giant-file serialisation), and migration `0015_session_corpus.sql` + the `get_session_context` tool landed (current surface = **74 tools**; next migration = **0016**). This plan supersedes the stub's stale `0014`/tool-count/path references.

## Scope

- **In scope**: one additive migration `0016_sprint_lifecycle_worktrees.sql` (a `worktrees` table; `sprints.worktree_id` + `sprints.predecessor_sprint_id`; a `work_items.checkpoint` flag; a `task_commits` cross-reference table; indexes; an `'open'→'active'` sprint-status backfill). A typed `SprintStatus` lifecycle (`draft→ready→active→review→done` +`cancelled`) enforced at the repo layer. Worktree CRUD + merge/rejection audit (record-only). Run-chaining provenance. The **stricter sprint-status guard** (claim runnable ⟺ `active`) + the **checkpoint-freeze** clause in `claim_next_task`, mirrored in `get_sprint_quiescence`. `record_task_commits` (explicit task-id list) + reads. 9 new MCP tools + HTTP mirrors (74→83). e2e + lifecycle/guard tests. Doc/glossary/ADR/catalogue updates.
- **Out of scope**: actually creating, mutating, or merging git worktrees (consumer/overseer — lumina shells out to git NEVER); the **commit choreography** (stage/message-draft/commit — the lead's job at a checkpoint barrier, ADR-0003); the composer/overseer engine; any *automatic* review-before-merge decision (a human/agent judgement — lumina records the disposition + outcome only); an idempotent already-merged git reconcile (pure audit for v1); a `sprint↔story/focus/epic` link table (inferred via the task hierarchy — ADR-0002); auto-wiring the checkpoint as a DAG dependency (runtime-freeze only — layer-3 owns smart ordering).
- **Affected areas**: `lumina/migrations/`, `lumina/src/domain/` (`enums.rs`, `planning.rs`, `work_items.rs`), `lumina/src/repo/` (`worktrees.rs` (new), `runs_sprints.rs`, `team_execution.rs`, `readiness.rs`, `work_items_meta.rs`, `shared.rs`, `reads.rs`, `mod.rs`), `lumina/src/export.rs`, `lumina/src/mcp/` (`worktrees.rs` (new), `runs_sprints.rs`, `mod.rs`), `lumina/src/http/` (`worktrees.rs` (new), `sprints.rs`, `mod.rs`), `lumina/tests/`, `lumina/CLAUDE.md`, `lumina/CONTEXT.md`, `CLAUDE.md`, `docs/adr/`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.
- **Estimated file count**: ~26 unique files (1 migration, ~10 src, 3 new-feature files, 3 tests, 5 docs). Higher than the ~15-file guideline — see Risks; the submodule split keeps every task ≤3 files with no parallel-batch file overlap, and layer-1 (a comparable single-migration ~13-task plan) executed cleanly in one flow.

## Research Notes

**External research deliberately skipped — entire design space covered by in-repo precedent.** This is a purely additive internal-schema extension mirroring five prior migrations; there are zero new external dependencies and no library-version or API-signature questions. Sources are codebase files (verified during exploration), not external docs:

- **Migration conventions** — `lumina/migrations/0011_runs_sprints_findings_queue.sql` (the `runs`/`sprints`/`sprint_tasks`/`finding_decisions` precedent + the ADD-COLUMN-REFERENCES NULL-default rule, header-comment style, FK-safe statement order), `0013_team_execution.sql` (nullable + CHECK column + partial indexes), `0014_repo_local_path.sql` (single nullable column), `0015_session_corpus.sql` (new table + `ON DELETE RESTRICT` FK + constant-string DEFAULT backfill + 3 indexes). **`sprints.status` is FREE TEXT today (0011:53), no CHECK** — only `'open'` is ever written; SQLite cannot `ALTER ... ADD CONSTRAINT`, so the new vocab is enforced at the repo layer (mirroring `work_items.status`, also free TEXT + Rust-enforced), not by a table rebuild.
- **Single-mutation + inert-event model** — `repo/events.rs:24` `record_event` (work_item: +1 work_items / +1 events), `:72` `record_inert_event` (inert aggregates `run|sprint|finding|batch|session` — NEVER git-exported; this plan ADDS `worktree` to that vocab, exactly as 0015 added `session`). `db.begin()` = `BEGIN IMMEDIATE` (RESERVED lock). Batch precedent (`add_tasks_to_sprint`, `0011` Part-B) = N rows + 1 coarse inert event.
- **Domain/serde** — `domain/enums.rs` (snake_case wire default; `Status`/`Tier`/`Lane`/`RunStatus` patterns), `domain/planning.rs` (`ClaimedTask` L65, `SprintQuiescence` L100, `NewSprint` L285, `NewRun` L271), `domain/work_items.rs` (`WorkItem` L10-106).
- **Repo seams** — `repo/team_execution.rs` (`claim_next_task` L113; **sprint-status guard L155-172** `NON_RUNNABLE_SPRINT_STATUSES` L38; **the E5 deviation `status IN ('todo','open')` at candidate-select + lease-UPDATE — must be preserved by any edit here**), `repo/runs_sprints.rs` (`create_sprint` L135, `create_run` L61, `record_finding_decision`), `repo/readiness.rs` (`get_sprint_quiescence` L61 — `claimable` is byte-consistent with the claim predicate), `repo/shared.rs` (`WorkItemRow` L41, `FromRow` L74, `create_work_item_full_tx` L672), `repo/reads.rs` (SELECT lists L54-66), `repo/work_items_meta.rs` (scalar setters), `repo/mod.rs` (`mod x; pub use x::*`).
- **Surface** — `mcp/mod.rs` (router-sum L225-240, `app_error_to_mcp` L118-140 `AppError::{NotFound→404, Validation→422, Cycle→422}`, count-invariant test L474 (74) + membership L356-451 + annotations L569-673); `mcp/runs_sprints.rs` / `mcp/team_execution.rs` tool patterns (write→`structured_result`, read→`json_result` + `read_only_hint`); `http/mod.rs` mount L40-66 (each family `pub fn router()`), `http/execution.rs` / `http/sprints.rs` handler pattern (delegate to the same `repo::*`).
- **`checkpoint`** already exists as an `ActivityType` variant (`enums.rs:88`) but NOT as a `work_items` column. No `worktrees`/`task_commits`/`checkpoint`-column code exists yet (grep clean).
- **Tests** — `tests/e2e.rs` (in-process MCP→DB→export-drain→HTTP via `tower oneshot`, no socket/sleep), `tests/migration_0013.rs` (PRAGMA + CHECK-reject + EXPLAIN QUERY PLAN), `tests/claim_concurrency.rs` (N=8 no-double-claim; lazy-reclaim via SEEDED past lease — no-sleep rule).

## User Decisions

> From the Phase-4 directed-questions gate. The answers materially refine the stub.

1. **Sprint status vocabulary** → **`draft → ready → active → review → done`** (+ `cancelled`). Prompted by the stub's Open Question 2 (free-TEXT `sprints.status`, layer-1 only writes `'open'`). User intent: an explicit *action between composition and submission*; the worktree→main merge may need separate review/optimise **Runs** before acceptance; follow-up sprints must be able to **target the same unmerged worktree** of a predecessor; **minimise churn on merged worktrees**. Derived: claim runnable ⟺ `active`; `active→done` legal for worktree-less sprints; `active→review→done` for the worktree merge path; `cancelled` = abandoned/rejected.
2. **Worktree ownership & status** → **owned by one sprint; status WHOLLY DERIVED from the owning sprint** (no independent worktree status). Prompted by the stub's Open Questions 1/4/6. Follow-up sprints **target** but do **not own** the worktree; the owning sprint stays `review` until the worktree is **merged or rejected**. Worktree carries merge-AUDIT only. Inverts the stub's free-floating-status model.
3. **Checkpoint ordering** → **runtime freeze only** (ADR-0003 open item). The flag + sprint-wide claim-freeze; NOT auto-wired as a task→task dependency.
4. **`task_commits` coverage** → **explicit task-id list** (ADR-0003 open item). The committing lead passes covered task-ids; pure audit.

**Folded-in (settled, not asked):** Q5 lumina is **record-only** (ADR-0002 — never shells to git); Q3 run-chaining = **explicit nullable `sprints.predecessor_sprint_id`**; Q6 merge = **pure audit** v1.

## Approach

**One additive, forward-only migration** (`0016`) mirroring 0011/0013/0015. The `worktrees` table is created before `sprints.worktree_id` REFERENCES it; `sprints` gains `worktree_id` + self-FK `predecessor_sprint_id` (both nullable per the ADD-COLUMN-REFERENCES rule); `work_items` gains a nullable `checkpoint INTEGER`; `task_commits` is a new cross-reference table; a `UPDATE sprints SET status='active' WHERE status='open'` backfills legacy rows so none violate the new vocab.

**Worktree-ownership model (the user's inversion).** `worktrees.owning_sprint_id` is a UNIQUE FK→`sprints(id)` (1:1 with its owner). `sprints.worktree_id` records which worktree a sprint *runs in* — the owner and every follow-up share the same `worktree_id`; the owner is the one row where `worktrees.owning_sprint_id = sprint.id`. There is **no `worktrees.status` column**: `get_worktree` returns `effective_status` by JOINing the owning sprint. The worktree carries audit-only terminal fields (`merged_at`, `merge_ref`, `outcome ∈ merged|rejected`).

**Sprint lifecycle** is a typed `SprintStatus { Draft, Ready, Active, Review, Done, Cancelled }` enforced in the repo layer (`sprints.status` stays free TEXT — adding a CHECK would need a SQLite table rebuild; this mirrors `work_items.status`). `create_sprint` now writes `'draft'` explicitly (the column DEFAULT `'open'` becomes vestigial). `set_sprint_status` validates legal transitions (`draft→ready→active→{review,done,cancelled}`, `ready→{active,cancelled}`, `review→{done,cancelled}`; `done`/`cancelled` terminal; illegal → `AppError::Validation`→422). **Guard:** `set_sprint_status` REJECTS a terminal transition (`review→done|cancelled`) for a sprint that *owns* a worktree — those must go through `record_worktree_merge` / `record_worktree_rejection` so the merge audit is never skipped.

**Merge/rejection are pure-audit compositions.** `record_worktree_merge(worktree_id, merge_ref?)` validates the owner is in `review`, stamps `merged_at`/`merge_ref`/`outcome='merged'`, and transitions the owner `review→done` — one `BEGIN IMMEDIATE` txn, one coarse inert `worktree` event (worktree + sprint are both inert aggregates; consistent with the inert-batch precedent). `record_worktree_rejection(worktree_id, reason?)` → `outcome='rejected'` + owner `review→cancelled`. lumina never verifies git state.

**Run-chaining** reuses existing primitives: a review/optimise **Run** over the owner sprint (existing `create_run` target_kind='sprint') produces findings; `record_finding_decision(spawn_task)` (layer-1 §E, already stamps `lane='implement'`/`tier=NULL`/sprint-bind) spawns fix tasks. The follow-up fix sprint is created via the **widened `create_sprint`** (NewSprint gains optional `worktree_id` + `predecessor_sprint_id`) so it targets the predecessor's worktree and records provenance — no new chaining tool needed.

**Checkpoint barrier (ADR-0003, runtime-freeze only).** `set_task_checkpoint(task_id, on)` sets the `work_items.checkpoint` flag (a standard work_item scalar setter — exported event, threaded through the row mapping like layer-1's columns). `claim_next_task` gains (a) the **stricter guard** — runnable ⟺ `status='active'` (replacing the `NON_RUNNABLE` set; the E5 `status IN ('todo','open')` task predicate is preserved) and (b) the **freeze clause** — return `Ok(None)` while any checkpoint task in the sprint is `in_progress`. `get_sprint_quiescence` mirrors both (claimable=0 when the sprint isn't `active` or a checkpoint is in-flight) to stay byte-consistent. `record_task_commits(commit_sha, task_ids[], sprint_id?)` inserts one `task_commits` row per task (idempotent via `UNIQUE(commit_sha, task_id)` + ON CONFLICT DO NOTHING) + one coarse inert event; `list_task_commits` reads by `task_id` | `commit_sha` | `story_id` (story→commits via the hierarchy).

**Surface (74→83):** a new `mcp/worktrees.rs` family (`create_worktree`, `get_worktree`, `list_worktrees`, `record_worktree_merge`, `record_worktree_rejection`, `set_task_checkpoint`, `record_task_commits`, `list_task_commits`) added to the router-sum + count/membership/annotations in `mcp/mod.rs`; `set_sprint_status` added to the existing `mcp/runs_sprints.rs` family. HTTP mirrors in a new `http/worktrees.rs` + sprint-status route in `http/sprints.rs`, mounted in `http/mod.rs`. Each route/tool delegates 1:1 to the same `repo::*` mutation.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets
smoke: rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests   # must print 0
```

## Tasks

### Phase 1: Schema & domain types

#### 1. Add migration `0016_sprint_lifecycle_worktrees.sql` + migration test [M]
- **Files**: `lumina/migrations/0016_sprint_lifecycle_worktrees.sql` (new), `lumina/tests/migration_0016.rs` (new)
- **Depends on**: —
- **Action**: Write the additive migration per Approach; add a migration test mirroring `tests/migration_0013.rs`.
- **Detail**: FK-safe order — (1) `CREATE TABLE worktrees (id TEXT PRIMARY KEY, owning_sprint_id TEXT NOT NULL UNIQUE REFERENCES sprints(id), path TEXT NOT NULL, base_ref TEXT, branch TEXT, merged_at TEXT, merge_ref TEXT, outcome TEXT CHECK(outcome IS NULL OR outcome IN ('merged','rejected')), created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, deleted_at TEXT)`; (2) `ALTER TABLE sprints ADD COLUMN worktree_id TEXT REFERENCES worktrees(id)` (nullable); (3) `ALTER TABLE sprints ADD COLUMN predecessor_sprint_id TEXT REFERENCES sprints(id)` (nullable self-FK); (4) `ALTER TABLE work_items ADD COLUMN checkpoint INTEGER` (nullable 0/1); (5) `CREATE TABLE task_commits (id TEXT PRIMARY KEY, commit_sha TEXT NOT NULL, task_id TEXT NOT NULL REFERENCES work_items(id), sprint_id TEXT REFERENCES sprints(id), recorded_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)`; (6) indexes — `idx_sprints_worktree ON sprints(worktree_id) WHERE worktree_id IS NOT NULL`, `idx_task_commits_task ON task_commits(task_id)`, `idx_task_commits_commit ON task_commits(commit_sha)`, `CREATE UNIQUE INDEX ux_task_commits ON task_commits(commit_sha, task_id)`; (7) `UPDATE sprints SET status='active' WHERE status='open'`. Header comment in the 0011/0015 forward-only/purely-additive style; document the `'open'→'active'` backfill rationale and the "no CHECK on sprints.status (free TEXT, repo-enforced; SQLite can't ALTER ADD CONSTRAINT)" decision. **Never edit an applied migration — 0016 is new.**
- **Acceptance**: `cargo nextest run` drives `db::init` clean on a fresh DB; `migration_0016.rs` asserts `PRAGMA table_info` includes the new `sprints`/`work_items` columns + the `worktrees`/`task_commits` tables, that `worktrees.outcome` CHECK rejects an out-of-vocab value, that `owning_sprint_id` UNIQUE rejects a duplicate, and `EXPLAIN QUERY PLAN` shows `idx_sprints_worktree` / `ux_task_commits` are used (SEARCH not SCAN).

#### 2. Add `SprintStatus`/`WorktreeOutcome` enums + `Worktree`/`NewWorktree`/`TaskCommit` structs + widen `NewSprint`/`WorkItem` [M]
- **Files**: `lumina/src/domain/enums.rs`, `lumina/src/domain/planning.rs`, `lumina/src/domain/work_items.rs`
- **Depends on**: —
- **Action**: Add the typed domain layer mirroring the existing enum/struct derive sets + serde conventions.
- **Detail**: `enums.rs` — `enum SprintStatus { Draft, Ready, Active, Review, Done, Cancelled }` and `enum WorktreeOutcome { Merged, Rejected }` (both `#[serde(rename_all="snake_case")]`, mirror `RunStatus`'s derive set; add a `SprintStatus::can_transition_to(&self, next) -> bool` helper encoding the legal-transition table from Approach). `planning.rs` — `Worktree { id, owning_sprint_id, path, base_ref: Option<String>, branch: Option<String>, merged_at: Option<String>, merge_ref: Option<String>, outcome: Option<WorktreeOutcome>, effective_status: SprintStatus, created_at, updated_at, deleted_at: Option<String> }`, `NewWorktree { owning_sprint_id, path, base_ref: Option, branch: Option }`, `TaskCommit { id, commit_sha, task_id, sprint_id: Option<String>, recorded_at }`; widen `NewSprint` with `worktree_id: Option<String>` + `predecessor_sprint_id: Option<String>` (`#[serde(default)]`). `work_items.rs` — add `checkpoint: Option<bool>` to `WorkItem` (migration-0003 `skip_serializing_if = Option::is_none` convention).
- **Acceptance**: compiles; a unit test round-trips `SprintStatus` + `WorktreeOutcome` wire forms and asserts a representative `can_transition_to` matrix (e.g. `Active→Review` true, `Done→Active` false).

#### 3. Thread `work_items.checkpoint` through the row mapping + export fixture [S]
- **Files**: `lumina/src/repo/shared.rs`, `lumina/src/repo/reads.rs`, `lumina/src/export.rs`
- **Depends on**: 1, 2
- **Action**: Carry the new `checkpoint` column through `WorkItemRow` (+ its manual `FromRow`), `work_item_from_row`, and both SELECT lists, so it reaches `WorkItemDetail` + git-export — exactly as layer-1 threaded its four columns.
- **Detail**: Add `checkpoint: Option<i64>` to `WorkItemRow` (`shared.rs:41`) + `try_get` in the `FromRow` (`:74`); map to `Option<bool>` in `work_item_from_row`; add `checkpoint` to `LIST_WORK_ITEMS_SQL` + `GET_WORK_ITEM_DETAIL_SQL` (`reads.rs:54-66`) BEFORE `created_at` to satisfy the tables-last export-ordering gate; update the `export.rs` tables-last fixture if it pins the column set.
- **Acceptance**: `get_work_item_detail` returns `checkpoint` (null on legacy rows); build + `migration_0016` green.

### Phase 2: Repo mutations (parallel — distinct submodule files)

#### 4. Worktree + task_commits mutators/reads in `repo/worktrees.rs` [L]
- **Files**: `lumina/src/repo/worktrees.rs` (new), `lumina/src/repo/mod.rs`
- **Depends on**: 1, 2
- **Action**: Create the new submodule (`mod worktrees; pub use worktrees::*;` in `mod.rs`) with the worktree + commit-provenance mutators, all single-`BEGIN IMMEDIATE` + coarse inert `worktree` events (add `worktree` to `record_inert_event`'s inert-aggregate vocab in `repo/events.rs:72` — one-line widen, like 0015's `session`).
- **Detail**: `create_worktree(db, &NewWorktree)` — INSERT worktree + UPDATE the owning sprint's `worktree_id` (one txn; validates the owning sprint exists). `get_worktree(db, id)` — SELECT + JOIN `sprints` for `effective_status`. `list_worktrees(db, status_filter: Option<SprintStatus>)` — live rows, optional effective-status filter. `record_worktree_merge(db, id, merge_ref: Option<&str>)` — validate owner is `review` (else `Validation`), stamp `merged_at`/`merge_ref`/`outcome='merged'`, transition owner `review→done`. `record_worktree_rejection(db, id, reason: Option<&str>)` — `outcome='rejected'`, owner `review→cancelled`. `record_task_commits(db, commit_sha, task_ids: &[&str], sprint_id: Option<&str>)` — N INSERTs `ON CONFLICT(commit_sha,task_id) DO NOTHING` + one coarse inert event (batch precedent). `list_task_commits(db, by: TaskCommitQuery)` — by task_id | commit_sha | story_id (story→direct-task-children→commits). Reuse `record_inert_event`; NEVER shell to git.
- **Acceptance**: unit tests — create→get returns `effective_status` tracking the owner; merge on a non-`review` owner is `Validation`; merge stamps audit + flips owner to `done`; rejection flips owner to `cancelled`; `record_task_commits` is idempotent on re-record; `list_task_commits` resolves all three query directions.

#### 5. Sprint lifecycle: `set_sprint_status` + widen `create_sprint` in `repo/runs_sprints.rs` [M]
- **Files**: `lumina/src/repo/runs_sprints.rs`
- **Depends on**: 1, 2
- **Action**: Add typed `set_sprint_status` (legal-transition validation) and widen `create_sprint` to consume the new `NewSprint` fields.
- **Detail**: `set_sprint_status(db, sprint_id, next: SprintStatus)` — read current, gate on `SprintStatus::can_transition_to` (illegal → `Validation`), and **reject a `review→done|cancelled` transition when the sprint OWNS a worktree** (`EXISTS worktrees WHERE owning_sprint_id=sprint_id` → `Validation: "use record_worktree_merge/rejection"`); one txn + one inert event. `create_sprint` — write `status='draft'` explicitly (not the column default) and persist `worktree_id`/`predecessor_sprint_id` when present (validate referenced worktree/sprint exist). Keep the existing inert `sprint.created` event.
- **Acceptance**: unit tests — each legal transition succeeds; an illegal one (`draft→done`) is `Validation`; a worktree-owning sprint's `review→done` via `set_sprint_status` is rejected; `create_sprint` defaults to `draft`; a chained sprint persists `worktree_id`+`predecessor_sprint_id`.

#### 6. `set_task_checkpoint` in `repo/work_items_meta.rs` [S]
- **Files**: `lumina/src/repo/work_items_meta.rs`
- **Depends on**: 1, 3
- **Action**: Add the checkpoint-flag scalar setter beside the other task setters.
- **Detail**: `set_task_checkpoint(db, task_id, on: bool)` — `UPDATE work_items SET checkpoint=:on` (work_item mutation → standard `record_event`, exported), one txn; validates the item is a task. Mirror `set_task_tier`'s shape.
- **Acceptance**: unit test — setting/clearing the flag round-trips through `get_work_item_detail`; one event emitted per change.

#### 7. Tighten claim guard + add checkpoint-freeze in `repo/team_execution.rs` [M]
- **Files**: `lumina/src/repo/team_execution.rs`
- **Depends on**: 1
- **Action**: Replace the layer-1 runnable rule with `status='active'`, and add the sprint-wide checkpoint-freeze, inside `claim_next_task`.
- **Detail**: Sprint-status guard (`L155-172`) → `runnable = (sprint_status.as_deref() == Some("active"))` (drop/retire `NON_RUNNABLE_SPRINT_STATUSES`); `Ok(None)` otherwise. Add a freeze guard returning `Ok(None)` when `EXISTS (SELECT 1 FROM sprint_tasks st JOIN work_items c ON c.id=st.task_id WHERE st.sprint_id=:sprint AND c.checkpoint=1 AND c.status='in_progress' AND c.deleted_at IS NULL)`. **Preserve the E5 `status IN ('todo','open')` predicate** at both candidate-select and lease-UPDATE. Do not move the advisory file-overlap scan into the txn.
- **Acceptance**: unit tests — a `draft`/`ready`/`review` sprint yields `None`; an `active` sprint claims normally; while a checkpoint task is `in_progress` the claim yields `None`, and resumes once it completes; the E5 `open`-status claimability test still passes.

#### 8. Mirror the guard + freeze in `get_sprint_quiescence` [S]
- **Files**: `lumina/src/repo/readiness.rs`
- **Depends on**: 1, 7
- **Action**: Keep `claimable` byte-consistent with the tightened claim predicate.
- **Detail**: `get_sprint_quiescence` (`L61`) — `claimable=0` when the sprint isn't `active` OR a checkpoint task is `in_progress` in the sprint; otherwise the existing readiness count. Keep `done`/`stalled` verdict logic.
- **Acceptance**: unit test — quiescence reports `claimable=0` during a freeze and for a non-`active` sprint; reverts to the real count once active + unfrozen.

### Phase 3: Surface (parallel — distinct files; tasks 9+10 coordinate the count)

#### 9. MCP worktree/checkpoint/commit family in `mcp/worktrees.rs` + register in `mcp/mod.rs` [L]
- **Files**: `lumina/src/mcp/worktrees.rs` (new), `lumina/src/mcp/mod.rs`
- **Depends on**: 4, 6
- **Action**: Add the 8-tool family and wire it into the constructor router-sum + the invariant tests.
- **Detail**: `#[tool_router(router = tool_router_worktrees, vis = "pub(crate)")]` with `create_worktree`/`record_worktree_merge`/`record_worktree_rejection`/`set_task_checkpoint`/`record_task_commits` (writes → `structured_result(json!{..})`) and `get_worktree`/`list_worktrees`/`list_task_commits` (reads → `json_result` + `annotations(read_only_hint=true, open_world_hint=false)`); each delegates 1:1 to the Phase-2 `repo::*` fn via `map_err(app_error_to_mcp)`. In `mcp/mod.rs`: `mod worktrees; pub use worktrees::*;`, add `+ Self::tool_router_worktrees()` to the sum (L225-240), **bump the count assertion 74→83** (count ALL 9 new tools incl. task 10's `set_sprint_status`), add the 9 names to the membership loop (L356-451) + the read/idempotent annotation lists (L569-673).
- **Acceptance**: `cargo nextest` tool-count test passes at 83; tools list advertises the 8 new names (+ `set_sprint_status` from task 10); annotations test green.

#### 10. MCP `set_sprint_status` + `create_sprint` widening in `mcp/runs_sprints.rs` [S]
- **Files**: `lumina/src/mcp/runs_sprints.rs`
- **Depends on**: 5
- **Action**: Add `set_sprint_status` to the existing `tool_router_runs_sprints` family and thread the new `create_sprint` params.
- **Detail**: `set_sprint_status` `#[tool]` (`{sprint_id, status: SprintStatus}` → `structured_result`); extend the `create_sprint` Params struct with optional `worktree_id`/`predecessor_sprint_id`. No router-sum change (family already summed); the count bump lives in task 9.
- **Acceptance**: the tool is callable end-to-end; an illegal transition surfaces `invalid_params` (422 mapping); the count test (task 9) passes with this tool present.

#### 11. HTTP mirrors: new `http/worktrees.rs` + sprint-status route in `http/sprints.rs` [M]
- **Files**: `lumina/src/http/worktrees.rs` (new), `lumina/src/http/sprints.rs`, `lumina/src/http/mod.rs`
- **Depends on**: 4, 5, 6
- **Action**: Mirror each new MCP tool as an HTTP route delegating to the same `repo::*`.
- **Detail**: `http/worktrees.rs` — `pub fn router()` with e.g. `POST /sprints/{sprint_id}/worktree` (create), `GET /worktrees/{id}`, `GET /worktrees`, `POST /worktrees/{id}/merge`, `POST /worktrees/{id}/reject`, `PATCH /work-items/{task_id}/checkpoint`, `POST /commits` (record_task_commits), `GET /commits` (list, query params). Mount via `.merge(worktrees::router())` in `http/mod.rs`. `http/sprints.rs` — add `PATCH /sprints/{sprint_id}/status` → `repo::set_sprint_status` and thread the widened create body. app.rs untouched.
- **Acceptance**: `oneshot` request to each new route returns the expected shape; an illegal sprint transition → 422 envelope.

### Phase 4: Tests

#### 12. E2E thread extension in `tests/e2e.rs` [M]
- **Files**: `lumina/tests/e2e.rs`
- **Depends on**: 9, 10, 11
- **Action**: Extend the in-process thread to exercise the full worktree/sprint lifecycle, no socket/sleep.
- **Detail**: create owning sprint S1 → `set_sprint_status` draft→ready→active → `create_worktree(S1, …)` → claim+complete tasks → `set_task_checkpoint` a task, assert the claim freezes while it's in_progress → complete it → `record_task_commits(sha, [tasks])` → S1 active→review → `create_run(review, S1)` + `record_finding_decision(spawn_task)` → create fix sprint S2 (`worktree_id=W1`, `predecessor_sprint_id=S1`) → ready→active → claim+complete the rework → S2→done → `record_worktree_merge(W1)` asserts W1.merged_at set + S1 review→done → HTTP reads of worktree/sprint-status/task_commits → export drain still succeeds (inert worktree/sprint events are NOT rendered). **Update any existing create-then-claim setup in this file to activate the sprint first** (the stricter guard).
- **Acceptance**: e2e passes; the worktree/commit records read back over HTTP; export drain is clean.

#### 13. Lifecycle/guard tests + fix existing claim tests in `tests/sprint_lifecycle.rs` [M]
- **Files**: `lumina/tests/sprint_lifecycle.rs` (new), `lumina/tests/claim_concurrency.rs`
- **Depends on**: 4, 5, 7, 8
- **Action**: A dedicated guard/lifecycle suite, plus repair the layer-1 concurrency test for the stricter guard.
- **Detail**: new file asserts — illegal sprint transitions rejected; claim refuses a non-`active` sprint; checkpoint-freeze yields nothing then resumes; merge-audit is idempotent and flips the owner to `done`; rejection flips to `cancelled`; worktree `effective_status` derives from the owner; a worktree-owning sprint can't terminal-transition via `set_sprint_status`; lumina never touches git (no shell-out anywhere in the worktree path). In `claim_concurrency.rs`, insert a `set_sprint_status(active)` (via draft→ready→active) before the concurrent claims so the N=8 no-double-claim test still holds under the new guard (seeded-past-lease reclaim unchanged — no-sleep rule).
- **Acceptance**: both test files pass deterministically; `claim_concurrency` shows no double-claim and no `SQLITE_BUSY`.

### Phase 5: Docs (parallel — distinct files)

#### 14. lumina/CLAUDE.md surface section [S]
- **Files**: `lumina/CLAUDE.md`
- **Depends on**: 9, 10, 11
- **Action**: Add a `### migration-0016 sprint-lifecycle & worktree substrate` block under § MCP tool surface (the 9 tools, the worktree-owned-by-sprint model, derived status, the lifecycle vocab + stricter guard + checkpoint-freeze, `task_commits`, the inert-`worktree`-event note) + the matching § HTTP routes block. Update the 74→83 references (L23/L110/L306).
- **Acceptance**: counts/catalogue match the implemented surface; rg for `74` in lumina/CLAUDE.md finds only historical migration-0015 mentions.

#### 15. CONTEXT.md glossary + ADR-0005 [M]
- **Files**: `lumina/CONTEXT.md`, `docs/adr/0005-sprint-lifecycle-worktree-ownership.md` (new)
- **Depends on**: —
- **Action**: Resolve the CONTEXT.md "Worktree" flagged-ambiguity to the decided model and capture the refinement in a new ADR.
- **Detail**: CONTEXT.md — update the **Worktree** flagged-ambiguity + **Sprint**/**Merge**/**Checkpoint** entries to state the owned-by-one-sprint / status-derived / follow-ups-target-not-own model and the `draft→ready→active→review→done` lifecycle. ADR-0005 (status accepted) records: the worktree-ownership inversion vs ADR-0002's "worktree:sprint=1:many" framing (it remains 1:many, now with a designated owner), the sprint-status vocab + the runnable⟺`active` guard, the runtime-freeze-only + explicit-`task_commits`-list resolutions of ADR-0003's two open items. Reference ADR-0002/0003. (May be authored before code lands — it's a decision record; no code dependency.)
- **Acceptance**: glossary has no stale free-floating-worktree-status wording; ADR-0005 exists and is linked from ADR-0002/0003's layer-2 references.

#### 16. Root CLAUDE.md count + plugin SKILL.md catalogue [S]
- **Files**: `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`
- **Depends on**: 9, 10
- **Action**: Bump the root tool count 74→83 and catalogue the 9 new tools in the plugin skill.
- **Detail**: `CLAUDE.md` L69 (the `get_session_context`/74 sentence) → 83 + a one-line migration-0016 family summary. SKILL.md — add a worktree/sprint-lifecycle/commit-provenance tool table (the 9 tools, params, when-to-use), following the existing per-family table format.
- **Acceptance**: root count = 83; SKILL.md tables list all 9 tools.

## Dependency Graph

```
Batch 1 (parallel):        1, 2
Batch 2 (after 1,2):       3
Batch 3 (parallel):        4, 5, 6, 7        (distinct repo submodule files)
Batch 4 (after 7):         8
Batch 5 (parallel):        9, 10, 11         (9 owns the count; 10 must land with 9)
Batch 6 (parallel):        12, 13
Batch 7 (parallel):        14, 15, 16        (15 has no code dependency — may start anytime)
```

- Phase 1: 1 ∥ 2 → 3. Phase 2: 4 ∥ 5 ∥ 6 ∥ 7 → 8 (the submodule split removes layer-1's single-file serialisation). Phase 3: 9 ∥ 10 ∥ 11 (9's count assertion includes 10's tool, so the count test only goes green once both land). Phase 4: 12 ∥ 13. Phase 5: 14 ∥ 15 ∥ 16.

## Verification

1. `cargo build --manifest-path lumina/Cargo.toml` — migration applies; columns/tables present.
2. `cargo nextest run --manifest-path lumina/Cargo.toml` — unit + `migration_0016` + `sprint_lifecycle` + repaired `claim_concurrency` + extended `e2e` green; tool-count test at 83.
3. `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — clean.
4. `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` — prints 0 (macro-eradication gate).
5. Manual smoke: create an owning sprint, draft→ready→active, `create_worktree`, claim/complete with a checkpoint task (observe the freeze), `record_task_commits`, active→review, `create_run`+`record_finding_decision` → chained fix sprint on the same worktree, `record_worktree_merge` → confirm the owner flips to `done` and lumina recorded intent without touching git.

## Risks

- **Higher-than-typical file count (~26)** — exceeds the ~15-file guideline. Mitigation: the joyful-singing-crane submodule split keeps every task ≤3 files with no parallel-batch file overlap, and layer-1 (a comparable single-migration ~13-task plan) executed cleanly in one flow. The atomic migration cannot be split across plans without breaking forward-only discipline. If the orchestrator prefers, Phases 1–2 and Phases 3–5 are a clean split point.
- **Stricter guard breaks existing create-then-claim tests** — new sprints start `draft`; claim requires `active`. Mitigation: task 12/13 repair `e2e.rs`/`claim_concurrency.rs` to activate the sprint first; the migration backfills live `'open'`→`'active'`. Any OTHER consumer that created a sprint and claimed immediately must now activate it — call out in the docs (task 14).
- **`get_sprint_quiescence` drift from the claim predicate** — the freeze + active-only rules live in two files (tasks 7 and 8). Mitigation: task 8 mirrors task 7's predicate exactly (layer-1 already requires this byte-consistency); the e2e (task 12) asserts quiescence during a freeze.
- **Worktree/sprint merge audit vs single-mutation invariant** — `record_worktree_merge` updates worktree + owner-sprint in one txn. Mitigation: both are *inert* aggregates (not `work_items`), so this follows the inert-batch precedent (`add_tasks_to_sprint`, 0011 Part-B) — one coarse export-inert `worktree` event; no `+1 work_items` rule is in play.
- **lumina/git divergence** — out-of-band git merges/deletes make records stale. Mitigation: records are audit/intent only; transitions are idempotent; v1 does no git reconcile (Q6).
- **Scope creep into the engine** — the temptation to auto-decide review-before-merge or auto-merge. Mitigation: hold the ADR-0002/0003 line — records + transitions only; the review-before-merge JUDGEMENT and the merge itself stay with the consumer (layer 3).
- **Editing an applied migration** — none: 0016 is new (memory rule). The `'open'→'active'` backfill is a forward-only `UPDATE` in the new migration, never an edit to 0011.
