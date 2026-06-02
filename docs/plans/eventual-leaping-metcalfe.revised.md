# Plan: Team-based task execution for lumina (claim/lease queue + review cascade)

**Plan path**: docs/plans/eventual-leaping-metcalfe.md
**Created**: 2026-06-02
**Status**: draft
> Last revised: 2026-06-02

## Context

We are building a new "implement"-style workflow where a **team of Claude Code agents** (via `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`) executes a pre-planned task graph concurrently. By session start every task already exists in lumina with dependencies and tiers defined. The design (settled over discussion) splits responsibilities:

- **Agent teams = the worker runtime** — the lead spawns a pool of long-lived agents (e.g. 3 `deep` + 2 `lite` implementers + N reviewers) that pull work independently.
- **lumina = the durable coordination plane** — a race-free work queue, atomic leasing, dependency/decision gating, file-conflict avoidance, dynamic review-task generation, crash recovery, and termination detection.

lumina is chosen as the coordination plane (not the agent-teams shared task list) because its rows are durable across `/resume` (NOTE: agent-teams in-process teammates are NOT restored on /resume — the lead may message teammates that no longer exist; orphaned leases self-heal only after TTL expiry, so size the TTL with resume-recovery latency in mind), race-free under SQLite's single-writer `BEGIN IMMEDIATE`, and already models tiers, dependencies, findings, runs/sprints, and the open-question decision lifecycle. This plan adds the **missing queue/lease/cascade primitives** on top of that store. Communication stratifies: ephemeral peer coordination over `SendMessage`; blast-radius decisions and human escalation over lumina's existing `open_questions` (a question sets `status='blocked'`, which removes the task from the claim set until resolved).

## Objective

lumina exposes an atomic work-queue so that a team of agents can: claim the next ready task by `(lane, tier)` under a lease; heartbeat/renew or have stale leases reclaimed; `complete_task`, which cascades a review task bound back to the implementation task; reviewers spawn rework via the existing findings→decision path; and the lead detects termination via a quiescence read — all persisted in SQLite, crash-recoverable, and mirrored on the HTTP API.

## Scope

- **In**: one additive migration (`work_items` columns + indexes); new `repo::*` mutations/reads; 6 new MCP tools + their HTTP mirrors; `Lane` domain type + row-mapping extension; `complete_task` cascade + review→rework lane stamping; concurrency + e2e tests; doc/catalogue updates.
- **Out**: the agent-teams runtime itself (lead spawn loop, per-lane teammate prompts) — that lives in the consumer, not lumina. No git-worktree merge machinery (file-lease handles conflicts in v1). No Postgres port. No new `runs.kind` (use a **sprint** as the execution container — `sprints.status` is free TEXT, no `kind` CHECK).
- **Affected areas**: `lumina/migrations/`, `lumina/src/repo.rs`, `lumina/src/mcp.rs`, `lumina/src/domain.rs`, `lumina/src/http/`, `lumina/src/app.rs`, `lumina/tests/`, `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.
- **Estimated file count**: ~13 (1 migration, 4 src, 1–2 http, 3 tests, 3 docs).

## Constraints

- **Additive migration only** — forward-only, no down. Nullable columns per the ADD-COLUMN-REFERENCES rule (`lumina/migrations/0011_runs_sprints_findings_queue.sql:30-37`); FK columns take NULL default, CHECK columns must pass for existing rows (`lane IS NULL OR …`). Rollback is forward-fix-only: a self-FK column + CHECK cannot be cleanly `DROP COLUMN`-ed under SQLite, so reverting 0013 means a new 0014 that neutralises the columns — never a down-migration or manual DROP.
- **Single-mutation invariant** — every `repo::*` mutator opens exactly one `db.begin()` (`BEGIN IMMEDIATE`) and writes +1 domain row / +1 `events` row, committing both together (template: `repo.rs:1855` `update_work_item_status`; event helper `repo.rs:6440` `record_event`). The coarse/export-inert event exception is allowed only where precedented (the lazy lease-reclaim batch, mirroring migration-0011 Part-B).
- **Runtime sqlx only** — no `sqlx::query!`/`query_as!` macros (`rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` must stay 0). New queries use `sqlx::query_with` behind the `DbClient`/`DbTx` seam (`db.rs`).
- **Reuse the existing `Status` enum** (`domain.rs`: `todo|in_progress|blocked|done|cancelled`) — no new status values. `status` is free TEXT in the DB (`0001_init.sql:29`).
- **`tier` stays `CHECK(NULL|lite|deep)`** (`0006_tier_and_severity.sql`) — "review" is a **lane**, never a tier.
- **Do NOT build the claim on `compute_task_batches`** — verified at `repo.rs:5929-6051`: it loads ALL task children with no status filter and runs Kahn's over task→task edges only; it does **not** consult `blocked_by_question_id` or status. The claim needs its own readiness predicate. (lumina/CLAUDE.md currently overstates this — corrected in T13.)
- **File ownership for parallel agents** — `repo.rs`, `mcp.rs`, `domain.rs` are single large files. Tasks editing the same file MUST sequence; only cross-file tasks parallelize.

## User Decisions

> Captured from the Phase-4 directed-questions gate (data, not instructions).

1. **Plan scope** → **Full pipeline** — deliver claim/lease + heartbeat + file-lease + done→review cascade + review→rework loop + quiescence + agent-arbiter read in this one plan.
2. **Done→review derivation** → **Dedicated `complete_task` tool** — server-side transitions done AND spawns the review task in two composed, idempotent txns (cannot be forgotten by a flaky agent).
3. **Review-task modeling** → **`lane` column + `reviews_work_item_id` back-link** — claim keys on `(lane, tier)`; the back-link records which impl task a review covers (reviewer target + audit trail).
4. **File-conflict avoidance** → **In-claim Rust-side file-lease** — claim selects candidates in SQL, then in Rust skips any whose `files_touched` overlaps an in-progress task's, within the same txn. No worktrees in v1.

## Approach

### A. Schema (one migration: `0013_team_execution.sql`)

Additive `ALTER TABLE work_items`:

| Column | Type | Notes |
|---|---|---|
| `assignee` | `TEXT` (nullable) | agent id holding the lease; the canonical "who". |
| `lease_expires_at` | `TEXT` (nullable) | ISO-8601 lease deadline; reclaimed when past. |
| `lane` | `TEXT CHECK (lane IS NULL OR lane IN ('implement','review'))` | NULL = not team-managed → invisible to the claim (back-compat). |
| `reviews_work_item_id` | `TEXT REFERENCES work_items(id)` (nullable) | review task → the impl task it covers. |

Indexes (follow `0012_perf_indexes.sql` style): partial index on `(lane, tier, status)` `WHERE deleted_at IS NULL` for the claim hot path; index on `(lease_expires_at)` `WHERE assignee IS NOT NULL` for lazy reclaim.

### B. `Lane` type + row mapping (`domain.rs`, `repo.rs`)

Add `enum Lane { Implement, Review }` (wire `implement|review`, matching the `Status`/`Tier` serde convention). Extend the `WorkItem` row struct + its `FromRow` + the two `SELECT` column lists at `repo.rs:506` and `repo.rs:609` to carry the four new columns (so they flow into `WorkItemDetail` and the git-export TOML snapshot automatically — export is event-driven off `work_items` rows, no export-code change).

### C. Claim / lease semantics (`repo.rs`)

`claim_next_task(db, sprint_id, lane, tier: Option<Tier>, agent_id, lease_ttl_secs) -> Result<Option<ClaimedTask>, AppError>` — ONE `db.begin()` txn:

1. **Lazy reclaim** (first statement): `UPDATE work_items SET status='todo', assignee=NULL, lease_expires_at=NULL WHERE status='in_progress' AND lease_expires_at < :now` (scoped to the sprint). One coarse `leases.reclaimed` event if any rows hit (export-inert, precedented). Self-heals dead-agent leases without a background reaper.
2. **Candidate select** (SQL, `LIMIT 16`): tasks joined to `sprint_tasks` where `status='todo'` AND `assignee IS NULL` AND `lane=:lane` AND (`:tier IS NULL` OR `tier=:tier`) AND `blocked_by_question_id IS NULL` AND `deleted_at IS NULL` AND `NOT EXISTS (unsatisfied dep)` — i.e. `NOT EXISTS (SELECT 1 FROM task_dependencies d JOIN work_items dep ON dep.id=d.depends_on_id WHERE d.task_id=t.id AND dep.status<>'done')`. Order by `(task_kind sort, created_at, id)` (mirror `compute_task_batches` tie-break).
3. **File-lease (Rust-side)**: load `files_touched` for all `in_progress` tasks in the sprint once. Normalise each entry to a key — bare string `p` → `(primary, p)`; object `{repo,path}` → `(repo, path)` (entries are heterogeneous, repo.rs:251/6149). A candidate conflicts iff its key-set intersects any in-progress key-set. Empty/absent `files_touched` = conservative WILDCARD: treat as conflicting with ALL in-progress tasks (skip it while any task is in progress) to avoid the unbounded-blast-radius collision noted in Risks. Pick the first non-conflicting candidate. Document the rule in T13.
4. **Lease**: `UPDATE … SET status='in_progress', assignee=:agent_id, lease_expires_at=:now+ttl`; `record_event 'work_item.claimed'` (payload carries `assignee`). Return `Some(ClaimedTask{ task_id, lane, tier, files_touched, … })`.
5. **Empty/all-conflicting** → `Ok(None)` (NOT an error → MCP returns `{claimed: null}`).

The SELECT→UPDATE in one `BEGIN IMMEDIATE` txn is race-free under SQLite's single writer — this is the property the agent-teams shared list cannot give. Corollary: all claimers serialise on the writer lock, and the Rust file-overlap scan (step 3, which serde-parses `files_touched` per in-progress task — repo.rs:6154) runs INSIDE that lock, so keep step 3 cheap. The in-progress set is sprint-scoped/small in v1, but a large sprint extends lock-hold time; the deferred `task_files` denormalisation removes the in-lock JSON parse.

`release_task(db, task_id, agent_id)` — clear `assignee`/`lease_expires_at`; set `status='todo'` **only if** current status is `in_progress` (leave `blocked` untouched, so park-after-question works); guarded by `WHERE assignee=:agent_id`. Used for park-and-pull and voluntary yield.

`renew_lease(db, task_id, agent_id, ttl)` — heartbeat: bump `lease_expires_at` where owned + `in_progress`. Minimal (idempotent). Note: `record_task_activity`/`append_activity` (`repo.rs:2487`) can double as a heartbeat carrier; keep `renew_lease` dedicated for a generous default TTL (30 min) so heartbeats are infrequent.

### D. `complete_task` cascade (`repo.rs`)

`complete_task(db, task_id, agent_id) -> Result<{task_id, review_task_id: Option}, AppError>` — two **composed, idempotent** txns (per the "compose, don't trigger" finding at `update_work_item_status`):

- **Txn 1**: first read the task's `lane` (it drives the Txn-2 branch). Transition to `done` via `update_work_item_status(db, task_id, 'done')` (preserves the closure-gate check; note repo.rs:1855 opens its OWN tx and clears NOTHING), then issue a separate owner-guarded `UPDATE work_items SET assignee=NULL, lease_expires_at=NULL WHERE id=:task_id AND assignee=:agent_id`.
- **Txn 2** (only when `lane='implement'`): spawn the review task — `parent_id` = the impl task's `parent_id` (always the story, per the hierarchy trigger, so no ancestor walk and no task-parents-task violation); `kind='task'`, `lane='review'`, `tier=NULL`, `reviews_work_item_id=task_id`, `files_touched` copied from the impl task. Mechanism: `create_work_item_full_tx` takes only `CreateOpts{origin,outcome,shape}` (repo.rs:1365), so stamp `lane='review'`/`reviews_work_item_id`/`files_touched` via a post-create `UPDATE work_items …` in the SAME txn (mirror the `spawned_from_finding_id` stamp at repo.rs:3856). Add a `task_dependencies` edge on the impl task AND an `INSERT INTO sprint_tasks (sprint_id, task_id)` binding the review task to the impl task's sprint — WITHOUT the sprint_tasks row the §C claim JOIN never sees it (sprint_tasks is written only by `add_tasks_to_sprint`, repo.rs:3658) and the cascade never runs. `record_event 'work_item.created'`.
- **Lane-awareness**: when `lane='review'`, complete to `done` only — **no** further review spawn (prevents infinite cascade).
- **Idempotency**: re-running on an already-`done` task with an existing `reviews_work_item_id` child no-ops; with none, spawns (crash recovery between the two txns).

### E. Review → rework loop (existing primitives + small extension)

Reviewer claims `lane='review'`, reads the files via `reviews_work_item_id`, then:
- **Clean** → `complete_task` (lane=review → done, no cascade).
- **Problems** → `add_findings` hosted **on the story** — NOT on the impl task (a task-hosted finding's `spawn_task` would parent a task under a task and fail with the hierarchy-trigger `RAISE(ABORT)` at 0001_init.sql:74/94). Hosting on the story lets `record_finding_decision(spawn_task)` create the rework task legally under the story. Then `record_finding_decision(spawn_task)` → rework task. **Extension**: the SpawnTask path (`repo.rs:3741`) stamps `lane='implement'` on the spawned rework task via a post-create `UPDATE` (create_work_item_full_tx carries no lane/tier — CreateOpts is origin/outcome/shape, repo.rs:1365) plus the same `INSERT INTO sprint_tasks` §D requires. Leave `tier=NULL` (NOT default `deep`): under the `(:tier IS NULL OR tier=:tier)` claim filter a `deep` default makes rework invisible to lite-tier claims; NULL lets either a lite or deep agent re-claim it, and a reviewer can force a tier afterward via `set_task_tier`.
- **Round-cap guard**: `findings.rounds` is NOT auto-incremented today (verified: no `SET rounds` UPDATE exists in repo.rs — it is written only at insert, repo.rs:3250), so reusing it requires a NEW increment step — extend the SpawnTask path to `UPDATE findings SET rounds = COALESCE(rounds,0)+1` on the host finding inside its txn. Then when `rounds >= N` (default 3) the reviewer records `record_finding_decision(defer)` + `add_open_question` (human escalation) instead of spawning another rework — prevents review↔rework loops (mirrors the ledger's chronic-item escalation).

### F. Quiescence + arbiter read (`repo.rs`, read-only)

`get_sprint_quiescence(db, sprint_id) -> SprintQuiescence` — counts across the sprint, all lanes: `claimable` (the §C readiness predicate minus the lease), `in_progress`, `blocked_on_question`, `terminal`. Verdict: `done = (claimable==0 && in_progress==0 && blocked==0)`; `stalled = (blocked>0 && claimable==0 && in_progress==0)` → needs an arbiter. The lead polls this to terminate or escalate.

`list_open_questions_for_sprint(db, sprint_id) -> Vec<OpenQuestionSummary>` — unresolved `open_questions` across the stories owning the sprint's tasks (question id, story, text, options, age). Lets a dedicated **arbiter agent** resolve code/convention questions and escalate product calls to the human (who answers via the existing SPA `POST /open-questions/{id}/resolve`).

### G. MCP + HTTP surface

6 new MCP tools in the `#[tool_router] impl LuminaTools` block (`mcp.rs:1321-2982`): `claim_next_task`, `release_task`, `renew_lease`, `complete_task` (write; `structured_result`); `get_sprint_quiescence`, `list_open_questions_for_sprint` (read; rmcp `#[tool(annotations(read_only_hint = true))]`, return `Json<T>` for structured_content). Each delegates to one `repo::*` call; `Ok(None)` from claim → `{claimed: null}` (no error). **Tool count 67 → 73** — update the count-invariant assertion (`mcp.rs:3245`) and the membership loop (`mcp.rs:3143`). Mirror each as an HTTP route in a new `lumina/src/http/execution.rs`, registered in `http/mod.rs` and mounted under `/api` by `app::build_router` (`app.rs:209`).

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets
smoke: rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests   # must print 0
```

## Tasks

### Phase 1: Schema & types foundation (parallel where cross-file)

#### T1: Add migration `0013_team_execution.sql`
- **Files**: `lumina/migrations/0013_team_execution.sql` (new)
- **Action**: Additive `ALTER TABLE work_items` adding `assignee`, `lease_expires_at`, `lane` (with CHECK), `reviews_work_item_id` (FK) per Approach §A; add the two partial indexes. Follow the 0011 header-comment + 0012 index style.
- **Acceptance**: `cargo nextest run` drives the embedded-migration path (db::init) clean on a fresh DB; a unit test asserts `PRAGMA table_info(work_items)` includes `assignee`/`lease_expires_at`/`lane`/`reviews_work_item_id` and that the `lane` CHECK rejects an out-of-vocab value; `EXPLAIN QUERY PLAN` on the T4 candidate SELECT shows the `(lane, tier, status)` partial index is used (SEARCH, not SCAN). Effort: S
- **Blocked-by**: none

#### T2: Add `Lane` enum + result types
- **Files**: `lumina/src/domain.rs`
- **Action**: `enum Lane { Implement, Review }` (serde wire `implement|review`); add result structs with EXACT fields — `ClaimedTask { task_id, lane: Lane, tier: Option<Tier>, assignee, lease_expires_at, files_touched: Vec<serde_json::Value> }`, `SprintQuiescence { claimable, in_progress, blocked_on_question, terminal, done: bool, stalled: bool }`, `OpenQuestionSummary { question_id, story_id, text, options: Vec<String>, age_secs }` (single source of truth for T4/T9/T10/T12). ALSO extend `domain::WorkItem` (domain.rs:21) with `assignee`/`lease_expires_at`/`lane`/`reviews_work_item_id`, each `Option<String>` using the migration-0003 `skip_serializing_if = Option::is_none` serde convention — without this T3's `work_item_from_row` (repo.rs:141) cannot compile. Mirror the `Tier` enum derive set.
- **Acceptance**: compiles; `Lane` round-trips its wire form in a unit test. Effort: S
- **Blocked-by**: none

#### T3: Extend `WorkItem` row mapping for the new columns
- **Files**: `lumina/src/repo.rs` — the `WorkItemRow` struct (~73), its manual `FromRow::from_row` (~111), the `work_item_from_row` mapper (~141), AND both `SELECT` lists (~502/~605). All four change together or the build breaks; add the new scalar fields BEFORE `created_at` to satisfy the tables-last export-ordering gate
- **Action**: Carry `assignee`/`lease_expires_at`/`lane`/`reviews_work_item_id` through the row struct and both column lists so they reach `WorkItemDetail` and git-export.
- **Acceptance**: `get_work_item_detail` returns the new fields (null on legacy rows); build clean. Effort: S
- **Blocked-by**: T1, T2

### Phase 2: Core queue mutations (sequential — all in repo.rs)

#### T4: Implement `claim_next_task` (+ lazy reclaim, file-lease)
- **Files**: `lumina/src/repo.rs`
- **Action**: Per Approach §C — one `BEGIN IMMEDIATE` txn: (1) lazy-reclaim expired leases as the first statement, emitting one coarse export-inert `leases.reclaimed` event ONLY when rows are reclaimed; (2) SQL candidate select (status/deps/lane/tier/not-blocked); (3) Rust-side file-overlap skip per the normalised-key rule (§C); (4) lease UPDATE + `work_item.claimed` event; `Ok(None)` when none. Reuse `record_event` (`repo.rs:6440`) and the `attributes.files_touched` read path (`repo.rs:6154`). Consider splitting step (1) into a prior task if the L estimate proves too large.
- **Acceptance**: unit test covering (a) deps + in-progress file-conflict skip; (b) empty lane → `None`; (c) an expired-lease task is lazily reclaimed to todo/assignee=NULL and emits exactly one coarse `leases.reclaimed` export-inert event; (d) a legacy `lane IS NULL` task is NEVER returned by the claim (back-compat). Effort: L
- **Blocked-by**: T3

#### T5: Implement `release_task` + `renew_lease`
- **Files**: `lumina/src/repo.rs`
- **Action**: Per §C — owner-guarded (`WHERE assignee=:agent_id`); `release` resets `in_progress`→`todo` but leaves `blocked`; `renew` bumps `lease_expires_at`. One event each.
- **Acceptance**: unit test — release frees a lease; releasing a `blocked` task keeps it blocked; renew extends; non-owner is a no-op/`NotFound`. Effort: M
- **Blocked-by**: T4

#### T6: Implement `complete_task` cascade
- **Files**: `lumina/src/repo.rs`
- **Action**: Per §D — two composed idempotent txns; reuse `update_work_item_status` for the `done` transition (closure-gate preserved); spawn review under the story with `lane='review'` + `reviews_work_item_id` + copied `files_touched` + dep edge; lane-aware (review tasks don't re-spawn).
- **Acceptance**: unit test — completing an `implement` task spawns exactly one review task bound back to it; completing a `review` task spawns none; re-running is idempotent. Effort: L
- **Blocked-by**: T4

#### T7: Implement `get_sprint_quiescence` + `list_open_questions_for_sprint`
- **Files**: `lumina/src/repo.rs`
- **Action**: Per §F — read-only counts + verdict; cross-story unresolved-question list scoped to the sprint.
- **Acceptance**: unit test — quiescence verdict flips `done`/`stalled` correctly across seeded states; question list returns only unresolved, sprint-scoped questions. Effort: M
- **Blocked-by**: T3

#### T8: Extend `record_finding_decision` SpawnTask for rework lane
- **Files**: `lumina/src/repo.rs` (`record_finding_decision` ~3741)
- **Action**: Per §E — stamp `lane='implement'` (+ `tier` via `compute_tier`/default `deep`) on spawned rework tasks; keep `spawned_from_finding_id` provenance.
- **Acceptance**: unit test — a spawn_task from a story-hosted finding yields a claimable `implement`-lane task. Effort: M
- **Blocked-by**: T3 (logical: needs the `lane` column + row mapping). Ordered after T4/T6 only by the repo.rs single-file serialisation rule, not by a data dependency on `complete_task`.

### Phase 3: Surface (parallel — different files)

#### T9: Add 6 MCP tools + update count invariant
- **Files**: `lumina/src/mcp.rs`
- **Action**: Add the 6 `#[tool]` methods + `Parameters` structs per §G; `claim`→`{claimed: null}` on `None`; reads carry `read_only_hint`. Bump assertion 67→73 (`mcp.rs:3245`) and the membership loop (`mcp.rs:3143`); map any new `AppError` via `app_error_to_mcp`.
- **Acceptance**: `cargo nextest` tool-count test passes at 73; tools list advertises the six new names. Effort: M
- **Blocked-by**: T4, T5, T6, T7

#### T10: Add HTTP mirrors
- **Files**: `lumina/src/http/execution.rs` (new), `lumina/src/http/mod.rs` (add `pub mod execution;` + `.merge(execution::router())`). app.rs is NOT touched — it nests the whole `http::router()` at app.rs:209.
- **Action**: Per §G — one handler per tool delegating to the same `repo::*`; register `router()` in `http/mod.rs`.
- **Acceptance**: `oneshot` request to each route returns the expected JSON/shape. Effort: M
- **Blocked-by**: T4, T5, T6, T7

### Phase 4: Tests (parallel — distinct test files)

#### T11: Claim concurrency test (the correctness gate)
- **Files**: `lumina/tests/claim_concurrency.rs` (new)
- **Action**: Mirror `tests/concurrency.rs` — N=8 agents call `claim_next_task` concurrently against an on-disk pool over a sprint of M tasks; assert each task is claimed at most once (distinct assignees, claims = min(N, ready M)), no `SQLITE_BUSY`. Test lazy-reclaim by SEEDING a row whose `lease_expires_at` is already in the past (NOT by elapsing a TTL — that violates the crate's no-sleep rule and is flaky under nextest); ensure `claim_next_task` accepts an injectable `now` or that `lease_ttl_secs=0` yields an immediately-reclaimable lease.
- **Acceptance**: test passes deterministically; no double-claim. Effort: M
- **Blocked-by**: T9

#### T12: E2E thread extension
- **Files**: `lumina/tests/e2e.rs`
- **Action**: Extend the in-process thread: claim → `complete_task` → assert review task spawned + back-linked → claim(review) → `add_findings`+`record_finding_decision(spawn_task)` rework → `get_sprint_quiescence` reflects state → git-export drains the new columns → HTTP read. No socket/sleep (use `oneshot` + direct export drain).
- **Acceptance**: e2e passes; exported TOML carries the new columns. Effort: M
- **Blocked-by**: T9, T10

### Phase 5: Docs (parallel — different files)

#### T13: Update docs & catalogue
- **Files**: `CLAUDE.md` (MCP surface count 67→73), also refresh the stale `58-tool surface` comments at `app.rs:216` + `pty/ask.rs:20` and extend the count-breakdown comment at `mcp.rs:3234-3242` with a +6 team-execution line, `lumina/CLAUDE.md` (document the new tools + **correct** the inaccurate "`compute_task_batches` respects task-on-question blocks" claim), `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` (catalogue the 6 tools + params).
- **Acceptance**: counts/catalogue match the implemented surface; the corrected claim describes the `status='blocked'` mechanism. Effort: S
- **Blocked-by**: T9

## Dependency Graph

```
T1 ─┐
T2 ─┴─► T3 ─► T4 ─► T5
                │   T6 ─► T8
                ├─► T7
                └─► (T4,T5,T6,T7) ─► T9 ──► T11
                                    └─► T10 ─► T12
                                    └─► T13
```

- Phase 1: T1 ∥ T2, then T3.
- Phase 2 (all repo.rs — **sequential**): T4 → {T5, T6, T7} (T6 → T8).
- Phase 3: T9 ∥ T10 (different files).
- Phase 4: T11 ∥ T12. Phase 5: T13 (∥ with Phase 4).

## Verification

1. `cargo build --manifest-path lumina/Cargo.toml` — migration applies; columns present.
2. `cargo nextest run --manifest-path lumina/Cargo.toml` — unit + `claim_concurrency` + e2e all green; tool-count test at 73.
3. `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — clean.
4. `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` — prints 0 (macro-eradication gate).
5. Manual smoke: against a running lumina, create a sprint of dependent tasks, `claim_next_task` from two lanes, `complete_task` one, confirm a review task appears bound to it, `get_sprint_quiescence` reports the right verdict.

## Risks

- **Claim correctness under concurrency** — the core risk. Mitigated by the single `BEGIN IMMEDIATE` SELECT→UPDATE (SQLite single-writer) and T11's concurrency test, which relies on the pool's EXISTING WAL + 5s `busy_timeout` (db.rs:32,79) — the same mechanism `tests/concurrency.rs` uses. Do not split the select and update across txns, and do not weaken the WAL/busy_timeout config without updating T11.
- **File-lease accuracy depends on `files_touched` being populated/honest** — a task with empty/wrong `files_touched` can collide. v1 accepts this; the documented scale path is a denormalized `task_files` table (deferred). Note the limitation in T13.
- **`complete_task` two-txn window** — a crash between txn 1 and txn 2 leaves a done task with no review; mitigated by idempotent re-run (T6). The reviewer/lead should re-issue `complete_task` on resume for done-but-unreviewed tasks.
- **Review↔rework non-termination** — mitigated by the `findings.rounds` cap + human escalation (T8/§E).
- **Lease TTL vs long tasks** — too-short TTL reclaims a live task. Mitigated by a generous default (30 min) + `renew_lease` heartbeat; document the tuning knob.
- **Doc drift** — three docs reference the tool count / `compute_task_batches` behaviour; T13 keeps them honest (the count-invariant test already guards 73).
