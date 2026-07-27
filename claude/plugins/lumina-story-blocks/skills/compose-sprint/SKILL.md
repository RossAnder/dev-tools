---
name: compose-sprint
description: Compose a sprint from a planned, decomposed story and ladder it draft→ready→active.
arguments: [story_id]
argument-hint: "[story_id]"
---

# `lumina:compose-sprint`

Composer: turns a planned, decomposed story into a runnable sprint and
activates it. Reads the story's readiness + tiered dispatch plan, gates the
task-set selection and worktree choice through two `AskUserQuestion` prompts,
mints the sprint, attaches the selected tasks, mints the worktree via the
companion (`execute_worktree_create` — the SERVER stays record-only; the git
runs in the separate `lumina-companion` process per ADR-0006, with manual
`git worktree add` + record-only `create_worktree` as the no-companion
fallback), and ladders the sprint `draft→ready→active`. It STOPS at `active`;
execution (`run-sprint`) and the terminal merge/rejection flip are a SEPARATE
skill's job.

A high-blast-radius MUTATOR (mints a sprint, attaches tasks, records a worktree,
drives status). Runs INLINE — the gates are user-mediated and benefit from the
parent context. Follows [CONVENTIONS.md](../../CONVENTIONS.md) §a/§c/§e, with §n
as the on-point contract for the lifecycle category: safety rests on §n.1
TXN-idempotency plus deliberate scheduler/operator invocation, NOT on a
supersession prompt. §j and §k are load-bearing: the composer CONSUMES
server-computed batches and tiers and never pre-batches or re-derives them.

## MCP tools used by this composer

- `mcp__lumina__get_work_item` — Step 1 kind precondition (`detail.kind`).
- `mcp__lumina__get_story_readiness` — Step 2 readiness gate (`ready_for_decomposition`).
- `mcp__lumina__get_task_dispatch_plan` — Step 2 the batched, tiered task waves.
- `mcp__lumina__create_sprint` — Step 4 mint the sprint (defaults `draft`).
- `mcp__lumina__add_tasks_to_sprint` — Step 5 attach the selected task ids.
- `mcp__lumina__get_checkpoint_suggestions` — Step 6 surface checkpoint candidates from cross-task `files_touched` overlap (sprint-scoped, over the attached tasks).
- `mcp__lumina__set_task_checkpoint` — Step 6 stamp the operator-finalised checkpoint set (`checkpoint=1`).
- `mcp__lumina__execute_worktree_create` — Step 7 mint a NEW worktree via the connected `lumina-companion` (PRIMARY — runs the real `git worktree add -b` AND records in one call).
- `mcp__lumina__create_worktree` — Step 7 record-only FALLBACK when no companion is connected (after a manual `git worktree add`).
- `mcp__lumina__set_task_lane` — referenced in Step 5 ONLY as the review-lane / clear path (NOT used to make planned tasks claimable — they already default `lane="implement"`).
- `mcp__lumina__set_sprint_status` — Step 8 ladder `draft→ready→active`.
- `mcp__lumina__record_task_activity` — §c provenance after each write.

## Body

### Step 1 — kind precondition

`detail = mcp__lumina__get_work_item({ id: "$story_id" })` (or
`mcp__lumina__get_tree` if you need the task subtree in one read). If
`detail.kind != "story"`, ABORT: `"compose-sprint requires a story work item;
got kind=<kind> for id=<id>."` This local check is the §e-blessed exception.

### Step 2 — readiness + dispatch-plan read

```
readiness = mcp__lumina__get_story_readiness({ story_id: "$story_id" })
```

If `readiness.ready_for_decomposition != true`, ABORT with a one-line reason
(`"story not ready_for_decomposition — run /lumina:plan-story first."`) — a
sprint must compose from a planned, decomposed story, not a bare one.

```
plan = mcp__lumina__get_task_dispatch_plan({ story_id: "$story_id" })
```

`plan.batches` is `Vec<Vec<BatchEntry>>` — the batched, tiered task waves
(`compute_task_batches` + per-task `compute_tier` server-side per §j/§k). If
`plan.batches` is empty, ABORT: `"story has no dispatchable tasks — run
/lumina:decompose-tasks + /lumina:set-task-spec first."` Surface the plan to the
user as phases (`Phase 1: T1[lite], T2[deep] | Phase 2: T3[lite] | …`) so they
can see the wave/tier shape before selecting.

### Step 3 — task-set selection (AUQ gate)

Gate the task set with this `AskUserQuestion`, VERBATIM:

> **Header**: `Task set`
> **Body**: `Include all ready tasks from the dispatch plan, or trim to a subset?`
> **Options** (exactly 2):
> - `All` — Include every task across `plan.batches` (flatten all waves).
> - `Trim` — Pick a subset; the composer then asks which task ids to include.

On `All`, `selected_task_ids` = every `task_id` flattened across
`plan.batches`. On `Trim`, prompt the operator for the subset (by task id /
title) and set `selected_task_ids` to their picks; the batches/tiers are
unchanged — trimming only narrows membership, never re-derives tiers.

### Step 4 — mint the sprint

```
{ sprint_id } = mcp__lumina__create_sprint({ title: "<story title> sprint" })
```

`create_sprint` defaults `status="draft"` (per the migration-0016 contract).
Then per §c append one activity row to the story:

```
mcp__lumina__record_task_activity {
  work_item_id: "$story_id",
  entry_type: "execution",
  origin: "plan",
  summary: "compose-sprint: created sprint <sprint_id> (draft)",
  body: "session=${CLAUDE_SESSION_ID}"
}
```

Apply the §c substitution guard verbatim (verify `${CLAUDE_SESSION_ID}`
resolved; on non-substitution write `session=unknown` and warn).

### Step 5 — attach the selected tasks

```
{ added } = mcp__lumina__add_tasks_to_sprint({ sprint_id, task_ids: selected_task_ids })
```

Idempotent at the junction (already-attached pairs collapse, not counted in
`added`). Then per §c append one activity row summarising the attach
(`summary: "compose-sprint: attached <added> tasks to sprint <sprint_id>"`).

**Lane note (no lane-stamping step needed)**: a planned task created via
`create_work_item` ALREADY defaults to `lane="implement"`, so once the sprint is
`active` (Step 8) every attached task is immediately claimable by
`claim_next_task` — there is NO manual lane-stamp here. Use
`mcp__lumina__set_task_lane` ONLY if you ever need to move a task to the
`review` lane or clear it (`lane=null`); it is never part of the compose path.

### Step 6 — checkpoint suggestions (AUQ gate)

Surface server-computed checkpoint candidates and let the operator finalise the
set BEFORE the ladder — a `checkpoint=1` task only FREEZES the sprint once it is
`active` (Step 8), so the marking must be in place before that.

```
suggestions = mcp__lumina__get_checkpoint_suggestions({ sprint_id })
```

`get_checkpoint_suggestions` returns the attached tasks whose first-class
EXPECTED `files_touched` sets INTERSECT another attached task's — each candidate
carries its overlapping peer task ids + the shared paths. (Sprint-scoped, so it
sees exactly the tasks attached at Step 5; the same read is also available
story-scoped via `{ story_id }` before a sprint exists.)

If `suggestions` is EMPTY, no two attached tasks plan to touch the same file —
skip checkpoints entirely (note `"compose-sprint: no file-overlap checkpoints"`)
and go to Step 7.

Otherwise present the candidates to the operator (task id + title + the shared
paths + overlapping peers) and gate the FINAL set with this `AskUserQuestion`,
VERBATIM:

> **Header**: `Checkpoints`
> **Body**: `<N> attached task(s) overlap on shared files. Mark the suggested candidates as checkpoints (consolidated-commit barriers), or override the set?`
> **Options** (exactly 2):
> - `Accept` — Stamp every suggested candidate as a checkpoint.
> - `Override` — Operator names the final checkpoint set (add a task the suggestion missed, drop one that does not need a barrier — including the empty set to mark none).

Stamp the FINAL set (the accepted suggestions, or the operator's override) one
task at a time:

```
mcp__lumina__set_task_checkpoint({ task_id: "<chosen task>", on: true })
```

A `checkpoint=1` task freezes the WHOLE sprint while it is `in_progress` (a
sprint-wide barrier for shared-file / consolidated-commit work — NOT a task→task
dep), so mark ONLY tasks that genuinely need the team to quiesce around a shared
commit. Per §c append one activity row recording the final set
(`summary: "compose-sprint: marked <n> checkpoint task(s) on sprint <sprint_id>"`).

### Step 7 — worktree (AUQ gate)

Gate the worktree decision with this `AskUserQuestion`, VERBATIM:

> **Header**: `Worktree`
> **Body**: `Create a new worktree for this sprint, or target an existing one?`
> **Options** (exactly 2):
> - `New` — This sprint OWNS a fresh worktree (minted via the companion's `execute_worktree_create`).
> - `Target-existing` — Share an existing sprint's worktree (target, do NOT own).

**On `New`** — mint via the companion (PRIMARY path):

1. `mcp__lumina__execute_worktree_create({ sprint_id, branch: "<branch>", base_ref: "<base_ref>" })`
   — the connected `lumina-companion` runs the real `git worktree add -b
   <branch> <path> <resolved-base>` on the server's behalf (the SERVER never
   shells to git; `base_ref` is any committish, e.g. `"main"`, resolved
   companion-side), and the server records the worktree in the same call.
   Success returns `{ worktree_id, path, head }`; the created worktree is
   ATTACHED to the new branch at the base tip, and this sprint OWNS it
   (`owning_sprint_id` is UNIQUE).
2. **Branch-name constraint (migration 0018)**: at most one LIVE
   (non-terminal) worktree may record a given branch — pick a branch name that
   does not collide with any live worktree's branch (terminal merged/rejected
   worktrees free theirs; a collision is rejected as Validation/422). Avoid
   POST-SANITISATION collisions too: distinct branches can sanitise to the
   same directory name (e.g. `feature/auth` → `feature-auth`), so keep the
   sanitised forms distinct as well.
3. **No-companion FALLBACK only**: if no companion is connected, the agent
   runs the real `git worktree add <path> -b <branch>` itself, THEN records it
   via `mcp__lumina__create_worktree({ owning_sprint_id: sprint_id, path:
   "<path>", branch: "<branch>", base_ref: "<base_ref>" })` — record-only;
   `path` is provenance TEXT, lumina does not touch the on-disk tree.

**On `Target-existing`** — do NOT call `create_worktree` (that would mint a
SECOND owner). Resolve the existing sprint's `worktree_id` (via
`mcp__lumina__list_worktrees` / `get_worktree`) and share it onto this sprint
(`sprints.worktree_id`) as a TARGET — follow-up sprints target a worktree but
never own it.

Per §c append one activity row recording the worktree choice
(`summary: "compose-sprint: <new worktree <id> | targeted worktree <id>> for sprint <sprint_id>"`).

### Step 8 — ladder draft→ready→active

Drive the sprint up the lifecycle with `set_sprint_status`, in order, STOPPING
at `active`:

```
mcp__lumina__set_sprint_status({ sprint_id, status: "ready" })   # draft → ready
mcp__lumina__set_sprint_status({ sprint_id, status: "active" })  # ready → active
```

`active` makes the attached `implement`-lane tasks claimable. STOP HERE —
compose-sprint does NOT execute the sprint. Per §c append one activity row
(`summary: "compose-sprint: laddered sprint <sprint_id> draft→ready→active"`).

### Worktree-owner terminal guard

compose-sprint MUST NEVER call `set_sprint_status` to a TERMINAL status
(`done` / `cancelled`) on a worktree-OWNING sprint. `set_sprint_status` itself
REJECTS a terminal `review→done|cancelled` flip on a worktree-owning sprint;
those terminal transitions go through `mcp__lumina__record_worktree_merge`
(drives the owning sprint `review→done`) or
`mcp__lumina__record_worktree_rejection` (drives `review→cancelled`), so the
merge/rejection audit is recorded atomically with the owner transition. That is
`run-sprint`'s job, NOT compose's — this composer's ladder stops at `active`.

### Final summary

```
compose-sprint: sprint <sprint_id> active; <added> tasks attached
  (<all|trimmed N-of-M>); worktree <new <id> | targeting <id>>;
  next: /lumina:run-sprint <sprint_id>.
```

## What the composer must NOT do

Compute readiness or tiers client-side (always `get_story_readiness` /
`get_task_dispatch_plan`); pre-batch tasks (§j — batching is server-side);
lane-stamp planned tasks (they default `implement`); shell to git for the
worktree mint while a companion is connected (Step 7's `execute_worktree_create`
is primary, manual `git worktree add` + `create_worktree` the fallback); or drive
a worktree-owning sprint to a terminal status (terminal guard above).

## Pointers

- Chained runner: [`../plan-story/SKILL.md`](../plan-story/SKILL.md) (populates the story this composer consumes).
- Advisor: [`../next-block/SKILL.md`](../next-block/SKILL.md); MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) (sprint-lifecycle / worktree tools, migration 0016).
