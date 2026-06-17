# 0002 — Sprint execution: pull-queue substrate, advisory file scope, worktree-as-merge-unit (three-layer split)

**Status:** accepted (2026-06-02)

## Decision

Lumina's team-based task execution is split into **three independently-built layers**, and four cross-cutting semantics are fixed so the substrate cannot contradict the (deferred) sprint-composer vision.

**The three layers:**

1. **Execution substrate** — plan `eventual-leaping-metcalfe`. A pull-based claim/lease queue: long-lived agents in a **Team** *draw* the next ready **Task** from their **Sprint** by `(lane, tier)` under an atomic lease (`BEGIN IMMEDIATE`), with a per-task review→rework cascade and lazy lease reclaim. **Worktree-agnostic.**
2. **Sprint-lifecycle substrate** — a future plan. A **first-class `worktrees` entity** (path, base ref, merge details, review-before-merge disposition), `sprints.worktree_id` + a sprint **status** lifecycle, and run-chaining. Built on layer 1.
3. **Composer / overseer engine** — deferred to the user's timing. The *intelligence*: compose sprints (manual→smart), gate the review-before-merge judgement, perform worktree merges, feed follow-up tasks. The substrate must **not** stub this.

**The four cross-cutting semantics:**

- **Pull, not push.** Agents self-assign by pulling ready work; the former "dispatch" splits into *composition* (selecting tasks into a sprint, up front) + the queue's atomic claim. `compute_task_batches` / `get_task_dispatch_plan` are **advisory previews** (a composer quick-add aid + UI batch view), never the runtime authority — the claim computes readiness live and must not depend on precomputed batches.
- **File scope is advisory, never a gate.** `files_touched` is best-effort (implementation routinely touches files beyond the declared set). The claim **never skips** a task on overlap; it returns a prima-facie overlap *caution* so the team can coordinate (peer `SendMessage`) or proceed with care. Task-claim race-freeness (no double-claim) is unaffected; only *file*-collision avoidance is advisory.
- **Worktree = inter-sprint isolation AND the merge unit.** Each Team runs in a git worktree; a worktree hosts **one or more sequential sprints** (implementation → optional review/fix) and merges to base **once** (`worktree : sprint = 1 : many`). Worktree creation/merge is consumer/overseer work; lumina **records** the lifecycle as an audit/intent log and **never polices git** (git is the source of truth for actual merge state).
- **Composition is up-front and human/agent-driven.** A sprint is composed — sized to taste, a slice of a plan or the whole thing — *before* it is queued; multiple sprints run concurrently, each isolated by its worktree.

## Considered options

- **Push, batch-synchronous dispatch** (a `/lumina:run-batch` driver reads `compute_task_batches` and pushes each wave onto freshly-spawned agents, barrier between waves) — **rejected**: lower utilization, a central dispatcher to crash-recover, and it duplicates what atomic leases give for free. Pull + leases is crash-recoverable and self-balancing. `compute_task_batches` survives as an advisory preview, not the executor.
- **Hard file-lease** (claim skips any candidate whose `files_touched` overlaps an in-progress task's) — **rejected**: `files_touched` cannot be trusted complete, so a hard lock would limit effectiveness for a guarantee it can't back, and it put a JSON parse inside the writer lock. Advisory overlap + worktree isolation + agent coordination is chosen instead.
- **Composition-enforced serialization** (wire a dependency edge between every file-overlapping pair so they never run concurrently) — **rejected** for the same best-effort reason: composition is itself best-effort and would over-serialize.
- **One mega-plan** (queue + lifecycle + composer together) — **rejected**: layer 1 is a prerequisite for layer 2, each is independently reviewable, and a mega-plan risks building the composer as a stub. Three sequential plans instead.
- **lumina polices git worktree/merge state** — **rejected**: lumina records intent + outcome as audit; git is the source of truth. A worktree merged or deleted out-of-band must not corrupt lumina.
- **Explicit sprint↔work-item link table** (relate a sprint to stories/focuses/epics) — **rejected for now**: inferable via the hierarchy (a sprint's tasks → their story/focus/epic ancestors). Add only if a real consumer needs a non-derivable relation.

## Consequences

- **Plan `eventual-leaping-metcalfe` (layer 1) is revised**: the §C file-lease becomes an advisory overlap *report* — the claim no longer skips on overlap; `ClaimedTask` gains an advisory overlap field; the overlap computation moves **out** of the write transaction. This removes prior review finding **P7** (no disjoint-set rule left to define) and **fully resolves P10** (no `files_touched` parse inside the writer lock). The claim gains a sprint-**status** guard so it pulls only from a runnable sprint.
- **A new layer-2 plan** introduces the first-class `worktrees` entity, `sprints.worktree_id`, the sprint status lifecycle vocabulary, merge-record columns, and run-chaining (a review `Run` over a sprint → an optional fix `Sprint` on the same worktree before merge). Additive, forward-only. **Refined by [ADR-0005](0005-sprint-lifecycle-worktree-ownership.md)**: the `worktree : sprint = 1 : many` framing above remains, now with a designated **owner** (`worktrees.owning_sprint_id` UNIQUE FK), a status **wholly derived** from that owner (no `worktrees.status` column), and the typed `SprintStatus::{Draft,Ready,Active,Review,Done,Cancelled}` lifecycle with the runnable⟺`active` claim guard.
- The composer/overseer **engine (layer 3)** stays deferred to the user's timing; this ADR is the guard that the substrate layers don't pre-empt it.
- Supersedes the framing in the `lumina-relevance-and-sprint-composer` note that called the composer "the execution trigger": the *execution trigger* is the pull queue; the composer is the *composition* layer above it.

## Amendment (2026-06-17) — run-sprint team-default topology + Kahn-boundary commit cadence

The layer-3 execution-loop runner (`/lumina:run-sprint`) now **defaults to the agent-team worker fan-out** with **automatic single-agent degrade**. The lead spawns a bounded pool (3–5 teammates per upstream guidance) that drains the layer-1 claim queue in parallel; it falls back to the single-agent worker loop when agent teams are unavailable (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` unset) or lumina MCP is not configured at PROJECT/USER scope — a teammate subagent does NOT inherit a per-subagent `mcpServers` block (it is dropped upstream), so without project/user-scope MCP a teammate could not reach `claim_next_task`. The degrade is automatic; the lifecycle, lane cascade, checkpoint freeze, quiescence, and the single companion-executed merge are IDENTICAL across both topologies — only the worker fan-out differs. This realises (does not change) the layer-1 "pull, not push" + "worktree = inter-sprint isolation" semantics: the team shares the sprint's ONE worktree, and the atomic lease is exactly the race-free primitive a shared in-process list cannot give.

The runner commits at **Kahn phase-batch boundaries**: it reads `compute_task_batches` **advisorily** (still NEVER the runtime claim authority — consistent with "Pull, not push"), commits at each phase boundary with `record_task_commits` provenance per commit, makes a consolidated commit at each `checkpoint=1` freeze, and a final close commit before the merge. Because teammates work concurrently and the boundaries are not hard barriers, an occasional commit may capture a dirty snapshot — **accepted**: under team concurrency per-commit bisectability is best-effort, reconciled by the checkpoint and close commits.

Checkpoints are surfaced server-side by a new read, `get_checkpoint_suggestions{story_id|sprint_id}` (MCP + HTTP), which returns candidate checkpoint tasks from cross-task `files_touched` OVERLAP — the third cross-cutting semantic ("file scope is advisory, never a gate"), now read pairwise over the first-class `task_files` EXPECTED set. `/lumina:compose-sprint` calls it and lets the operator finalise the set via `set_task_checkpoint` BEFORE the sprint runs. It stays **advisory** — it suggests consolidated-commit barriers; it does not gate the claim.

Glossary: `lumina/CONTEXT.md` (Sprint, Composition, Team, File scope, Run, Merge; refined Worktree flagged-ambiguity). Layer-1 plan: `docs/plans/eventual-leaping-metcalfe.md`. Layer-1 review findings: `.claude/flows/eventual-leaping-metcalfe/plan-review-findings.toml`.
