# 0005 — Sprint lifecycle & worktree ownership: owned-by-one-sprint, status-derived, merge-once

**Status:** accepted (2026-06-08)

## Decision

Layer 2 of the sprint-execution stack ([ADR-0002](0002-sprint-execution-architecture.md)) lands the first-class `worktrees` entity, a typed **sprint lifecycle**, and run-chaining — and in doing so fixes the worktree↔sprint ownership model and resolves the two open items ADR-0003 left for "Plan B". lumina stays **record-only** throughout: it records worktree/merge/commit provenance and **never shells out to git** (git is the source of truth for actual merge state). This is migration 0016.

### Worktree ownership — inversion to a designated owner

ADR-0002 framed `worktree : sprint = 1 : many` (a worktree hosts a chain of sprints — implementation → optional review/fix — and merges once). That framing **remains true**, now sharpened with a **designated owner**:

- A worktree is **owned by EXACTLY ONE sprint** — `worktrees.owning_sprint_id` is a **UNIQUE FK → `sprints(id)`**.
- Its status is **WHOLLY DERIVED** from the owning sprint: there is **NO `worktrees.status` column**. `get_worktree` returns an `effective_status` by JOINing the owner. This removes the earlier implicit "free-floating worktree status" — a worktree has no independent lifecycle of its own.
- **Follow-up sprints TARGET the same worktree** (they share `sprints.worktree_id`) **but do NOT own it**. So one worktree hosts a **chain** of sprints and **merges ONCE**, minimising churn on a merged worktree.
- The worktree carries only **audit-terminal** fields, stamped at merge/rejection time: `merged_at`, `merge_ref`, `outcome ∈ merged|rejected`.

This is an *inversion* of where status lives (it was previously imagined as a worktree-local lifecycle column), not a change to the cardinality: `worktree : sprint = 1 : many` still holds, with the owner being the row where `worktrees.owning_sprint_id = sprint.id`.

### Sprint lifecycle vocabulary + the runnable guard

A typed `SprintStatus::{Draft, Ready, Active, Review, Done, Cancelled}` is enforced at the **repo layer**. `sprints.status` stays **free TEXT with NO DB CHECK** — SQLite cannot `ALTER TABLE ADD CONSTRAINT`, so the guard lives in Rust, mirroring `work_items.status`.

- **Legal transitions**: `draft→ready`; `ready→{active,cancelled}`; `active→{review,done,cancelled}`; `review→{done,cancelled}`; `done`/`cancelled` terminal.
- **Runnable ⟺ `status='active'`.** `claim_next_task` pulls only from an `active` sprint — tightening ADR-0002 layer-1's "any non-terminal status (incl. legacy `'open'`) is runnable". The migration **backfills legacy `'open'`→`'active'`**, and `create_sprint` now defaults to `'draft'`.
- **A worktree-owning sprint cannot terminal-transition via `set_sprint_status`.** It stays in `review` until the worktree is **merged or rejected**; `set_sprint_status` REJECTS a bare `review→done|cancelled` flip on a worktree-owning sprint — those must route through `record_worktree_merge` / `record_worktree_rejection`, so the merge audit is never skipped.

### Resolving ADR-0003's two open items

ADR-0003 left two questions "for Plan B fleshing"; both are resolved here:

1. **Checkpoint ordering = RUNTIME-FREEZE ONLY.** The checkpoint barrier is the `work_items.checkpoint` flag plus a **sprint-wide claim-freeze** while any checkpoint task is `in_progress` (`claim_next_task` returns `Ok(None)`). It is **NOT** auto-wired as a task→task dependency edge — smart DAG-explicit ordering is layer-3's (the composer/overseer's) concern. `get_sprint_quiescence` mirrors the freeze (a frozen-but-incomplete sprint does not falsely report `done`).
2. **`task_commits` coverage = EXPLICIT task-id LIST.** The committing lead passes the covered task-ids to `record_task_commits(commit_sha, task_ids[], sprint_id?)` — coverage is an explicit list, **not** derived from completion timestamps. Idempotent via `UNIQUE(commit_sha, task_id) ON CONFLICT DO NOTHING`; pure audit (story-level context inferred via the task hierarchy, no explicit story link).

### Run-chaining + pure-audit merge

- **Run-chaining** is an explicit nullable `sprints.predecessor_sprint_id` (the widened `create_sprint` / `NewSprint`). A review `Run` over a completed sprint can motivate a follow-up review/fix `Sprint` that targets the same worktree, chained to its predecessor.
- **Merge is PURE AUDIT (v1).** No git reconcile, no squash/rebase policing — lumina records the worktree/merge lifecycle as a durable audit/intent log and **never shells out to git**. `record_worktree_merge` validates the owner is in `review`, stamps the terminal fields, and transitions the owner `review→done`; `record_worktree_rejection` stamps `outcome='rejected'` and transitions the owner `review→cancelled`.

## Considered options

- **A `worktrees.status` column with its own lifecycle** — **rejected**: it would duplicate (and risk diverging from) the owning sprint's status. Deriving `effective_status` by JOIN keeps a single source of truth and matches the record-only posture (a worktree is provenance, not an independent state machine).
- **A worktree owned jointly / referenced symmetrically by all its sprints** — **rejected**: without a designated owner there is no unambiguous "who merges this / whose status is authoritative" row. A UNIQUE `owning_sprint_id` plus shared `worktree_id` for followers gives a clear owner while still letting the chain target one worktree.
- **Checkpoint as a task→task dependency edge (DAG-explicit ordering)** — **deferred to layer 3**: wiring the checkpoint as a hard predecessor of the next chunk is composer intelligence; the substrate ships the runtime freeze only (ADR-0003's lighter of the two options).
- **`task_commits` coverage derived from completion timestamps** — **rejected**: timestamp inference is fragile across drains and re-runs. An explicit task-id list from the committer is unambiguous and audit-faithful.
- **lumina performs / polices the git merge** — **rejected** (consistent with ADR-0002 and ADR-0003): lumina records merge intent + outcome; git stays the source of truth. A worktree merged or deleted out-of-band must not corrupt lumina.
- **A DB CHECK on `sprints.status`** — **rejected as infeasible**: SQLite cannot `ALTER TABLE ADD CONSTRAINT`, so the typed-transition guard lives at the repo layer (mirroring `work_items.status`).

## Consequences

- **Schema (migration `0016_sprint_lifecycle_worktree.sql`)**: a new `worktrees` table (`owning_sprint_id` UNIQUE FK, audit-terminal `merged_at`/`merge_ref`/`outcome`, no `status` column); `sprints.worktree_id` + `sprints.predecessor_sprint_id` (both nullable); `work_items.checkpoint`; a new `task_commits` table; supporting indexes; and the `'open'`→`'active'` sprint-status backfill. Additive, forward-only. (Next migration = 0017.)
- **Nine new tools** (surface 74 → 83): the worktree/checkpoint/commit family (`create_worktree`, `get_worktree`, `list_worktrees`, `record_worktree_merge`, `record_worktree_rejection`, `set_task_checkpoint`, `record_task_commits`, `list_task_commits`) plus `set_sprint_status`, with HTTP mirrors. A new **`worktree`** export-inert event aggregate (never git-exported).
- **The claim guard tightens** (a behaviour change to migration-0013's `claim_next_task`): runnable ⟺ `status='active'`, plus the checkpoint freeze. `get_sprint_quiescence` mirrors both.
- **Layer 3 (composer / overseer engine) stays deferred** — smart checkpoint ordering, the merge judgement, and worktree merge execution remain its concern; this layer records the lifecycle, it does not drive it.

Glossary: `lumina/CONTEXT.md` (Worktree flagged-ambiguity resolved to the owned-by-one-sprint / status-derived / follow-ups-target-not-own / merge-once model; Sprint, Merge, Checkpoint, Run refined). Builds on and refines [ADR-0002](0002-sprint-execution-architecture.md) (the layer-2 entity it promised) and resolves the two open items in [ADR-0003](0003-commit-checkpoint-provenance.md). Layer-2 plan: `docs/plans/sprint-lifecycle-worktree-substrate.md`.
