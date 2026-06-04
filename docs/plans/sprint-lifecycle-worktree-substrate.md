# Plan: Sprint-lifecycle & worktree substrate (layer 2)

**Plan path**: docs/plans/sprint-lifecycle-worktree-substrate.md
**Created**: 2026-06-02
**Status**: skeleton (seed — resolve the Open Design Questions and flesh the tasks before `/review-plan` → `/implement`)
**Architecture**: layer 2 of [ADR-0002](../adr/0002-sprint-execution-architecture.md); commit/checkpoint provenance per [ADR-0003](../adr/0003-commit-checkpoint-provenance.md). Builds on layer 1 — the execution substrate (`docs/plans/eventual-leaping-metcalfe.md`). The composer/overseer **engine** (layer 3) stays deferred; this plan builds none of it.
> Last revised: 2026-06-04
> Paths updated 2026-06-04 for the joyful-singing-crane refactor: `repo.rs` / `mcp.rs` / `domain.rs` are now submodule directories. Worktree mutators land in a NEW `repo/worktrees.rs`; sprint-lifecycle + run-chaining extend `repo/runs_sprints.rs`; the claim-guard tightening + checkpoint-freeze edit `repo/team_execution.rs` (where `claim_next_task` now lives). Domain: `WorktreeStatus`/sprint-status enums in `domain/enums.rs` (beside `Lane`), the `Worktree` struct in `domain/planning.rs` (beside `ClaimedTask`/`NewSprint`). New MCP tools form a `mcp/worktrees.rs` family (constructor router-sum + count-invariant in `mcp/mod.rs`); HTTP in a new `http/worktrees.rs`.

## Objective

Make the **Sprint** a fully-tracked lifecycle entity and introduce a first-class **Worktree** as the inter-sprint isolation + merge unit, so one worktree can host a *chain* of sprints (implementation → optional review/fix) and be merged once — with lumina recording the worktree/merge lifecycle as a durable **audit/intent log** (git stays the source of truth). This is the substrate the deferred composer/overseer engine will later drive.

## Constraints

- **Additive, forward-only** migration(s) — mirror layer-1 / 0011 conventions; ADD-COLUMN-REFERENCES → NULL default; a self-FK + CHECK ⇒ rollback is forward-fix-only (a later migration, never a down/DROP).
- **Single-mutation invariant** — every `repo::*` mutator = +1 domain row / +1 `events` row in one `BEGIN IMMEDIATE` txn (coarse export-inert events only where precedented).
- **Runtime sqlx only** — no `query!`/`query_as!` macros (`rg` gate stays 0); `sqlx::query_with` behind the `DbClient`/`DbTx` seam.
- **Reuse, don't duplicate** existing primitives: `runs` (review pass over a sprint), `findings`, `record_finding_decision(spawn_task)`, `sprint_tasks`, `create_sprint` / `add_tasks_to_sprint`.
- **lumina records merge intent/outcome only — it must NEVER police git state.** A worktree merged or deleted out-of-band must not corrupt lumina; any reconciliation is best-effort and idempotent.
- **Sprint↔higher-item (story/focus/epic) relations are inferred via the task hierarchy** — NO explicit sprint-link table (settled in the reconciliation grilling).
- **Build no layer-3 engine** — provide records + transitions, not the intelligence/automation (composition, the review-before-merge decision, the actual merge).

## Scope

- **In**: a `worktrees` first-class entity (path, base_ref, branch, status, `requires_review` disposition, merge audit); `sprints.worktree_id` + a formal sprint status lifecycle + the *stricter* sprint-status guard the layer-1 claim left as a seam; the **checkpoint barrier** (`work_items.checkpoint` flag + a claim-freeze clause: yield no candidate while a checkpoint task is `in_progress` in the sprint — per [ADR-0003](../adr/0003-commit-checkpoint-provenance.md)); a `task_commits` cross-reference record (checkpoint commit SHA ↔ covered tasks) + its reads; sprint run-chaining provenance (fix sprint ↔ predecessor impl sprint, same worktree); MCP tools + HTTP mirrors for the new surface; e2e + lifecycle tests; doc/glossary/catalogue updates.
- **Out**: actually creating or merging git worktrees (consumer/overseer); the **commit choreography** (staging, message-drafting via the `commit-conventions` skill, committing — the lead's job at a checkpoint barrier, per ADR-0003); the composer/overseer engine; any *automatic* review-before-merge decision (a human/agent judgement — lumina records the disposition + outcome only).
- **Affected areas**: `lumina/migrations/`, `lumina/src/repo/worktrees.rs` (new — worktree + `task_commits` mutators), `lumina/src/repo/runs_sprints.rs` (sprint lifecycle + run-chaining), `lumina/src/repo/team_execution.rs` (claim-guard tightening + checkpoint-freeze), `lumina/src/repo/mod.rs` (re-export the new submodule), `lumina/src/mcp/worktrees.rs` (+ `mcp/runs_sprints.rs`; constructor router-sum + count-invariant in `mcp/mod.rs`), `lumina/src/domain/enums.rs` + `lumina/src/domain/planning.rs`, `lumina/src/http/worktrees.rs` (new) + `lumina/src/http/sprints.rs` (sprint-status routes) + `lumina/src/http/mod.rs` (mount), `lumina/tests/`, `lumina/CLAUDE.md`, `CLAUDE.md`, `lumina/CONTEXT.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.

## Open Design Questions (resolve before fleshing tasks)

1. **Worktree status vocabulary** — proposed `open → active → review_pending → reviewed → merged` (+ `abandoned`). Confirm the set + legal transitions.
2. **Sprint status vocabulary** — proposed `composed → queued → running → review → done`; decide whether `merged` is a sprint state or rolled up from the worktree, and where the layer-1 claim's "runnable" line sits (likely `running`).
3. **Run-chaining shape** — explicit `sprints.predecessor_sprint_id` (provenance link) vs deriving the chain from shared `worktree_id` + `runs`. Recommend the explicit nullable link.
4. **review-before-merge gate strength** — soft (record `requires_review` + warn) vs hard (block a `merged` transition while `requires_review` ∧ not `reviewed`). The reconciliation framed it as a judgement call ⇒ lean soft, but a hard guard on the *record* transition may be wanted. Confirm.
5. **Does lumina open worktrees or only record them?** — recommend record-only (the consumer creates the git worktree, then calls `create_worktree` with path/branch). Confirm lumina never shells out to git.
6. **Merge reconciliation** — is `record_worktree_merge` pure audit (`merged_at`, `merge_ref`), or is an idempotent "already-merged" reconcile in scope? Recommend pure audit for v1.
7. **Checkpoint ordering** — rely purely on the runtime claim-freeze (global, sprint-wide) for "subsequent tasks wait," or *also* wire the checkpoint as an explicit dependency of the next chunk (DAG-explicit)? ADR-0003 leaves this open; recommend freeze-primary, optional explicit deps.
8. **`task_commits` coverage derivation** — does the committing agent pass the covered task-id list explicitly, or does lumina derive it from `done`-timestamps since the previous checkpoint commit? Recommend explicit list (robust against clock/ordering ambiguity).

## Tasks (skeleton — phased outline; flesh after the Open Design Questions resolve)

### Phase 1: Schema & domain
- **T1**: migration `0014_sprint_lifecycle_worktrees.sql` — `worktrees` table + `sprints.worktree_id` (FK, nullable) + (per Q3) `sprints.predecessor_sprint_id` + a `work_items.checkpoint` flag (nullable bool) + a `task_commits` table (checkpoint commit SHA ↔ task_id); indexes in 0012 style.
- **T2**: domain types — `WorktreeStatus` + sprint-status enums in `domain/enums.rs` (mirror the layer-1 `Lane` pattern, also in `enums.rs`); the `Worktree` struct in `domain/planning.rs` (beside `ClaimedTask`/`NewSprint`); row mapping in the owning repo submodule.

### Phase 2: Worktree + sprint-lifecycle mutations (`repo/` submodules)
- **T3**: `create_worktree` / `get_worktree` / `list_worktrees` in a new `repo/worktrees.rs` (declare `mod worktrees; pub use worktrees::*;` in `repo/mod.rs`).
- **T4**: `record_worktree_merge` (audit; review gate per Q4) + `set_worktree_status` — `repo/worktrees.rs`.
- **T5**: `set_sprint_status` (formal transitions) + attach `worktree_id` to a sprint (extend `create_sprint` or a new `set_sprint_worktree`) — `repo/runs_sprints.rs`.
- **T6**: run-chaining (`repo/runs_sprints.rs`, where `record_finding_decision` already lives) — compose a fix sprint on a predecessor's worktree (provenance link) from a review run's findings; wire `record_finding_decision(spawn_task)` so rework lands in the fix sprint + its sprint_tasks.
- **T6b**: `record_task_commits` mutation in `repo/worktrees.rs` — map a checkpoint commit SHA ↔ the tasks it covered (per ADR-0003) + read APIs (task→commits, commit→tasks; story→commits via the hierarchy). Pure audit; never polices git.
- **T7**: tighten the layer-1 claim's **sprint-status guard** to the full lifecycle (the seam Plan A left at `eventual-leaping-metcalfe` §C step 5) — this edits `claim_next_task` in `repo/team_execution.rs` — AND add the **checkpoint-freeze clause** there too: yield no candidate while a checkpoint task is `in_progress` in the sprint (ADR-0003).

### Phase 3: Surface
- **T8**: MCP tools for the above — a new `mcp/worktrees.rs` `#[tool_router(router = tool_router_worktrees, vis = "pub(crate)")]` family (+ sprint-status tools extending `mcp/runs_sprints.rs`); add the family to the constructor router-sum and bump the count-invariant + annotations tests in `mcp/mod.rs`; `app_error_to_mcp` (in `mcp/mod.rs`) mapping.
- **T9**: HTTP mirrors — a new `http/worktrees.rs` + sprint-status routes in `http/sprints.rs`, both mounted via `.merge(...)` in `http/mod.rs`.

### Phase 4: Tests
- **T10**: e2e — impl sprint on W1 → review `Run` → fix sprint on W1 → `record_worktree_merge` → exported audit → HTTP read (no socket/sleep).
- **T11**: lifecycle/guard unit tests — illegal transitions rejected; the claim refuses a non-runnable sprint; merge-audit idempotent; lumina never polices git.

### Phase 5: Docs
- **T12**: `lumina/CLAUDE.md` (worktree/sprint-lifecycle surface), `lumina/CONTEXT.md` (any new terms), the mcp `SKILL.md` catalogue, `CLAUDE.md` tool count.

## Verification

- `cargo build` / `cargo nextest run` / `cargo clippy` (lumina manifest); `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` = 0.
- e2e: the impl→review→fix→merge-audit thread passes; exported TOML carries the worktree/merge records.
- Manual smoke: create a worktree, attach a sprint, run it, review-run it, chain a fix sprint on the *same* worktree, record the merge — confirm lumina records intent without policing git.

## Risks (seed)

- **Scope creep into the engine** — the temptation to auto-decide review-before-merge or auto-merge. Hold the line: records + transitions only (layer 3 owns decisions).
- **lumina/git divergence** — out-of-band git operations make lumina's merge records stale. Mitigate by treating them as audit/intent, never authority; keep transitions idempotent.
- **Sprint-status guard coupling** — tightening the layer-1 claim (T7) edits `claim_next_task` in `repo/team_execution.rs` (layer-1-owned); sequence after layer 1 lands to avoid churn. The refactor's submodule split means T7's edits are now confined to `repo/team_execution.rs` and no longer collide with this plan's worktree/sprint mutators in `repo/worktrees.rs` + `repo/runs_sprints.rs` — so once the new submodule + its `repo/mod.rs` re-export exist, those three can proceed concurrently.
