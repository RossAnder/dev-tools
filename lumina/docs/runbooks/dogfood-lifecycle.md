# Runbook — dogfood lifecycle (create → plan → decompose → compose → execute → merge)

> Canonical end-to-end runbook for driving one slice of work through lumina's
> MCP surface. The lumina SERVER is **record-only for git**: it never shells to
> git. Git executes either in the `lumina-companion` process (PREFERRED — the
> `execute_worktree_create` / `execute_worktree_merge` tools create/merge AND
> record in one call, per [ADR-0006](../../../docs/adr/0006-git-execution-companion.md))
> or in the agent's own shell (the no-companion FALLBACK — real git + the
> record-only provenance verbs `create_worktree`, `record_task_commits`,
> `record_worktree_merge`).
>
> Tool names below are cited from the authoritative catalogue at
> [`../../../claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`](../../../claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md).
> The orchestration skills referenced by slug — `/lumina:create-project`,
> `/lumina:compose-sprint`, `/lumina:run-sprint` — drive the phases below; the
> thin read-only advisor `/lumina:lifecycle` tells you where you are and which
> gate is next.
>
> **Agent execution model**: an agent cannot *type* a slash command, and the
> chained runners (`/lumina:create-project`, `/lumina:plan-story`) cannot
> `Skill()`-dispatch their `disable-model-invocation` block siblings (the
> dispatch is refused). Where a leg says "via `/lumina:<block>`", the runner
> executes by READING that block's `SKILL.md` and replicating its steps inline
> via raw MCP — see CONVENTIONS §l.4. The slash forms name the block to run;
> they are the human entry point, not the agent's dispatch mechanism.

The lifecycle is project → epic → focus → story → task → compose-sprint →
worktree → execute → merge. Each section A–H below is one leg; the
**ORDERING-GATE CHECKLIST** at the end is the single source of truth for the
eleven gates the legs must satisfy, in order.

---

## A. Create the hierarchy (top-down) — `/lumina:create-project`

Build the tree top-down so every child has a legal parent. Use
`create_work_item` for each level (the `/lumina:create-project` orchestrator
composes the per-block field skills around these creates):

```
create_work_item { kind: "project", title: "…" }                                  → { id: <project> }   # NULL parent — gate (1)
create_work_item { kind: "epic",    parent_id: <project>, title: "…", outcome: "…" } → { id: <epic> }    # outcome MANDATORY — gate (2)
add_acceptance_criterion { work_item_id: <epic>, text: "…" }                       # ≥1 close-criterion BEFORE any story — gate (3)
create_work_item { kind: "focus",   parent_id: <epic>, title: "…", shape: "vertical-slice" } → { id: <focus> }  # shape MANDATORY — gate (4)
create_work_item { kind: "story",   parent_id: <focus>, title: "…" }              → { id: <story> }
```

- **Gate (1) — project has NULL parent.** A `project` is the root: it carries no
  `parent_id`. Every other kind requires a legal parent of the level above.
- **Gate (2) — epic requires a non-empty outcome.** `create_work_item` rejects a
  `kind:"epic"` create that omits `outcome` as `invalid_params`. The outcome is
  the value the epic delivers, stated as an end-state.
- **Gate (3) — epic needs ≥1 acceptance (close-)criterion BEFORE any story
  (R3 hard gate).** A story CANNOT be created under an epic that carries zero
  close-criteria. Write each via `add_acceptance_criterion { work_item_id:
  <epic>, … }` and verify `get_work_item(<epic>).acceptance_criteria.length >= 1`
  before the first `create_work_item { kind: "story" }`.
- **Gate (4) — focus requires a shape.** `create_work_item` rejects a
  `kind:"focus"` create that omits `shape ∈ {vertical-slice, cross-cutting,
  foundational}` as `invalid_params`.

## B. Plan the story — `/lumina:plan-story`

Walk the story through the six-phase canonical sequence (frame → explore →
decide → verify-design → decompose → closure). `/lumina:plan-story` is the
chained runner — an agent walks it by inline-replicating each block's
`SKILL.md` via raw MCP (CONVENTIONS §l.4), not by `Skill()`-dispatch;
`/lumina:next-block` advises the next single block. This leg
fills `problem_statement`, accepted research notes, approach, acceptance
criteria, verification commands, risks, etc. — the inputs the closure and
sprint legs depend on.

The closure gate is SET here (leg B) and ENFORCED later (leg G):

- **Gate (5) — the task→done CLOSURE gate.** `/lumina:closure-gate` sets a
  story's `closure_gate ∈ {hard, soft}` via `set_closure_gate`. A story with
  `closure_gate='hard'` BLOCKS any child task's `task → done` transition while
  any acceptance criterion on that task is unchecked. `check_acceptance_criterion`
  ticks each off (appending a `verification` activity internally); only when all
  are checked can the hard-gated task reach `done`. A `soft` gate records the
  unchecked criteria but does not block.

## C. Decompose into tasks — `/lumina:decompose-tasks` + `/lumina:set-task-spec`

Decompose the story into `task` children (`create_work_item { kind: "task",
parent_id: <story>, … }`), then populate each task's spec
(`set_task_spec` — `execution_detail` / `files_touched` / `outcome` / `tier`)
and wire dependency edges (`/lumina:wire-task-deps` → `block_task_on_task`;
`compute_task_batches` later derives the phase batches).

**Lane defaults (lane fix).** A task created via `create_work_item` DEFAULTS to
`lane='implement'`, so it is immediately claimable by `claim_next_task` once its
sprint is `active` — there is NO manual lane-stamping step. Non-task kinds stay
`lane=NULL` (invisible to the claim). Use `set_task_lane(task_id, lane)` (or
`PATCH /work-items/{id}/lane`) only to RE-stamp or clear a lane; planned tasks
need no setter call to become claimable.

## D. Compose the sprint — `/lumina:compose-sprint`

Compose a sprint over the planned, spec'd tasks:

```
create_sprint { title?: "…" }                         → { sprint_id }   # status defaults to "draft"
add_tasks_to_sprint { sprint_id, task_ids: [ … ] }    → { added }       # idempotent at the junction
```

`create_sprint` mints the sprint in `draft`. `add_tasks_to_sprint` attaches the
chosen task ids under one transaction (an already-attached pair is collapsed and
not re-counted). Composition is a COMPOSITION step, NOT an execution trigger —
the sprint does not start running just because tasks are attached.

Between attach and the worktree/ladder, compose-sprint surfaces **checkpoint
suggestions** from cross-task `files_touched` overlap and lets the operator
finalise the set BEFORE the sprint runs (a `checkpoint=1` task only freezes the
sprint once it is `active`):

```
get_checkpoint_suggestions { sprint_id }              → [ { task_id, overlaps: [ { task_id, shared_paths } ] }, … ]
set_task_checkpoint        { task_id, on: true }      # stamp each operator-finalised checkpoint
```

`get_checkpoint_suggestions` returns the attached tasks whose first-class
EXPECTED `task_files` set intersects another attached task's (story- or
sprint-scoped). It is **advisory** — it suggests consolidated-commit barriers
(gate 8) for shared-file work; the operator accepts, adds, or drops candidates,
then stamps the final set with `set_task_checkpoint`. Marking happens here, not
later, because the freeze only bites once the sprint is `active`.

## E. Create the worktree — `execute_worktree_create` (companion), record-only fallback

A sprint OWNS exactly one worktree. PREFER the companion execute path — one
call creates the on-disk worktree AND records it:

```
execute_worktree_create { sprint_id: <sprint>, branch: "…", base_ref: "…" } → { worktree_id, path, head }
```

The companion runs `git worktree add -b <branch> <path> <base>` (`base_ref` is
any committish, resolved companion-side; the new worktree comes up ATTACHED to
the new branch at the base tip) and the server records the row via the same
`repo::create_worktree` mutation the record-only tool uses. Pre-flights: the
sprint must exist (404), be non-terminal and not already own a live worktree
(422), `branch`/`base_ref` must be non-empty (422), and a companion must be
connected (502 otherwise).

- **Unique-live-branch constraint (migrations 0018 + 0019).** At most one LIVE
  worktree (`outcome IS NULL AND deleted_at IS NULL`) may record a given branch
  PER REPOSITORY (the 0019 rebuild scopes the index by the worktree's
  `repo_link_id`; unstamped legacy rows share one bucket); a merged/rejected —
  or soft-deleted — worktree frees the branch. In the execute path a same-branch duplicate usually fails
  earlier in git itself (`worktree add -b` on an existing branch → 502
  `BranchInUse`); the index catches record-layer races and record-only
  collisions ("a live worktree already records branch …" → 422).
- **No-companion fallback (record-only).** The AGENT creates the on-disk
  worktree with real git and lumina only RECORDS it:

  ```
  git worktree add <path> -b <branch> <base_ref>        # agent-run git
  create_worktree { owning_sprint_id: <sprint>, path: "…", base_ref?: "…", branch?: "…" } → { worktree_id }
  ```

- **Gate (11) — the server never runs git.** `create_worktree` records the
  worktree row; it does NOT create the on-disk worktree (that is the companion's
  or the agent's job). Likewise the merge and commit-provenance record verbs
  (legs G/H) RECORD facts that git produced. A worktree is owned by EXACTLY ONE
  sprint (`worktrees.owning_sprint_id` UNIQUE); follow-up sprints TARGET a
  worktree (sharing `sprints.worktree_id`) but never own it, and its
  `effective_status` is JOIN-derived from the owning sprint (there is no
  `worktrees.status` column).

## F. Drive the sprint ladder + execute — `/lumina:run-sprint`

`/lumina:run-sprint` **defaults to the agent-team worker fan-out** (a bounded
pool of 3–5 teammates draining the claim queue in parallel under one lead) and
**auto-degrades to a single-agent worker loop** when agent teams are unavailable
(`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` unset) or lumina MCP is not configured at
project/user scope (teammates inherit MCP only from project/user settings, never
a per-subagent `mcpServers` block). Both topologies share the SAME sprint
worktree and the SAME lifecycle below; only the worker fan-out differs. The
runner commits at Kahn phase-batch boundaries (`compute_task_batches` read
advisorily) with `record_task_commits` provenance, plus a consolidated commit at
each checkpoint freeze and a final close commit before the merge — see
[ADR-0002](../../../docs/adr/0002-sprint-execution-architecture.md) (the
2026-06-17 team-default amendment).

A sprint walks the lifecycle ladder before tasks can be claimed:

```
set_sprint_status { sprint_id, status: "ready" }    # draft → ready
set_sprint_status { sprint_id, status: "active" }   # ready → active   ← CLAIM needs "active"
```

- **Gate (7) — the sprint ladder.** `SprintStatus` is `draft → ready → active →
  review → {done | cancelled}`. Legal transitions: `draft→ready`;
  `ready→{active, cancelled}`; `active→{review, done, cancelled}`;
  `review→{done, cancelled}`; `done`/`cancelled` are terminal. **`claim_next_task`
  requires the sprint to be `active`** — it returns `{ claimed: null }` (not an
  error) when the sprint is not runnable.

Execute by claiming, leasing, and completing tasks:

```
claim_next_task { sprint_id, lane: "implement", tier?, agent_id, lease_ttl_secs } → { claimed }
renew_lease     { task_id, agent_id, lease_ttl_secs }                            # heartbeat
complete_task   { task_id, agent_id }                                            → { task_id, review_task_id? }
```

Because tasks default to `lane='implement'` at create (leg C), a planned task is
claimable the moment the sprint is `active` — no lane-stamping detour.

- **Gate (8) — the checkpoint freeze.** A task with `checkpoint=1` (set via
  `set_task_checkpoint { task_id, on: true }`) that is `in_progress` FREEZES the
  WHOLE sprint: `claim_next_task` returns nothing until the checkpoint task
  completes. This is a runtime freeze over the entire sprint, NOT a task→task
  dependency edge — use it to serialise barrier work (e.g. a shared-file
  migration) that must not race with other claims.

- **Gate (10) — the review-lane cascade.** `complete_task` on an `implement`-lane
  task spawns a `lane='review'` task under the same story, back-linked via
  `reviews_work_item_id`, with copied `files_touched` and a task→task dep edge.
  A reviewer claims it (`claim_next_task { lane: "review" }`); reviewer findings
  spawn rework via `record_finding_decision { decision: "spawn_task" }`, which
  stamps `lane='implement'` + `tier=NULL` on the rework task and binds it to the
  sprint — so the rework is itself claimable. A `review`-lane (or NULL-lane)
  completion spawns nothing (prevents an infinite cascade).

Poll quiescence to know when to stop or escalate:

```
get_sprint_quiescence { sprint_id } → { claimable, in_progress, blocked_on_question, terminal, done, stalled }
```

`done = (claimable==0 && in_progress==0 && blocked==0)`; `stalled = (blocked>0 &&
claimable==0 && in_progress==0)` needs an arbiter (resolve via
`list_open_questions_for_sprint` + the open-question resolve path).

## G. Close out the work — commit provenance + the closure gate

The agent commits with real git; lumina RECORDS which tasks each commit
implements:

```
record_task_commits { commit_sha: "<sha>", task_ids: [ <task>, … ], sprint_id?: <sprint> } → { recorded }
```

Idempotent via `UNIQUE(commit_sha, task_id)`; records one inert `worktree`
aggregate event (never git-exported). Read it back with `list_task_commits`.

- **Gate (5, enforced).** With a `closure_gate='hard'` story, each task's
  `transition_status { id: <task>, status: "done" }` (or `complete_task`) is
  BLOCKED while any of its acceptance criteria are unchecked. Tick them with
  `check_acceptance_criterion` before completing the task.
- **Gate (6) — the epic→done gate.** An epic is `done` only when ALL of its
  close-criteria are checked AND ALL descendant stories are terminal
  (`done`/`cancelled`). The close-criteria are the epic's own deliverable gate;
  the descendant-story rollup is the structural gate. Both must hold — there is
  no `hard`/`soft` mode at the epic level (the done-rule is unconditional).

## H. Merge (or reject) the worktree — `execute_worktree_merge` (companion)

Move the sprint into review, then PREFER the companion execute path — one call
performs the merge AND records it, driving the owning sprint `review → done`
with the companion's ground-truth SHA:

```
set_sprint_status { sprint_id, status: "review" }                  # active → review
execute_worktree_merge { worktree_id, target_branch?, no_ff? }     # companion merges; server records review → done
# — rejection, or the no-companion fallback after an agent-run git merge —
record_worktree_merge     { worktree_id, merge_ref?: "<ref>" }     # record-only fallback / audit verb; drives owning sprint review → done
record_worktree_rejection { worktree_id, reason?: "…" }            # stamps audit fields; drives owning sprint review → cancelled
```

The companion merges inside a DETACHED integration worktree (it never checks a
branch out) and advances the target branch with an atomic compare-and-swap
(`git update-ref … <new> <expected_old>`); the reachability gate — every
recorded task commit must remain reachable — runs BEFORE any ref move. Outcomes
to handle:

- **Merged** → recorded (`review → done`) with the ground-truth `merge_sha`.
  The response may carry a `target_checkout {path, dirty}` field + human
  `hint`: the target branch is checked out in another worktree (typically the
  operator's stale primary), which now shows spurious "undo-the-merge" diffs —
  refresh it with `git reset --keep <merge_sha>` as the hint says; committing
  on the stale checkout would revert the merge.
- **Conflicted** → a structured SUCCESS payload (the conflicting paths) with
  **NO DB write** — the companion has already aborted the merge and restored
  the worktree. Surface it as an open question / finding and STOP; do not
  retry blindly.
- **AlreadyUpToDate** → early return, no ref move; re-runs are idempotent
  (the crash/disconnect recovery path — git already holds the merge, the
  record catches up).
- **TargetMoved** → the target branch moved (or was deleted) between
  tip-resolve and the CAS advance. Nothing was rolled back and nothing needs
  to be — re-run, and the next attempt resolves the new tip (a deleted ref
  then surfaces as NotFound).
- **"merge already in flight"** → the per-target in-memory lease rejected the
  call; it does NOT queue. Retry same-target merges one at a time, in
  dependency order.

**Lifecycle invariants:**

- **A worktree merges EXACTLY ONCE.** The pre-flight requires the owning
  sprint in `review`, and the merge records it `done` (terminal). Follow-up
  work after a merge = a NEW sprint + a NEW worktree off the updated base —
  never reuse the merged worktree.
- **The "never check out the integration/target branch" rule is RELAXED for
  merges.** The detached integration worktree means the companion never needs
  the target branch checked out anywhere, and the ref-CAS catches a concurrent
  move — so a checked-out target is now safe. The stale-primary refresh remedy
  above still applies.

- **Gate (9) — the worktree-owner terminal guard.** A worktree-OWNING sprint may
  reach a terminal state (`done`/`cancelled`) ONLY via `record_worktree_merge` /
  `record_worktree_rejection`, from ANY source. `set_sprint_status` REJECTS a
  direct terminal `review→done|cancelled` flip on a worktree-owning sprint
  (`invalid_params`) — this guarantees the merge/rejection audit is recorded
  atomically with the owner transition. **Un-wedge path:** if a sprint is stuck,
  drive it `active→review` (legal via `set_sprint_status`) and then
  `record_worktree_rejection` to reach `cancelled` cleanly.

---

## ORDERING-GATE CHECKLIST (the eleven gates)

Walk these in order; each must hold before the leg it gates proceeds. This is
the canonical list — the legs A–H above cite it.

1. **Project has NULL parent.** A `project` is the root; it carries no
   `parent_id`. Every other kind needs a legal parent of the level above.
   *(Leg A.)*
2. **Epic requires a non-empty outcome.** `create_work_item { kind:"epic" }`
   rejects a missing/empty `outcome` as `invalid_params`. *(Leg A.)*
3. **Epic needs ≥1 acceptance (close-)criterion BEFORE any story (R3 hard
   gate).** A story cannot be created under an epic with zero close-criteria;
   write ≥1 via `add_acceptance_criterion(<epic>)` and verify before the first
   `create_work_item { kind:"story" }`. *(Leg A.)*
4. **Focus requires a shape.** `create_work_item { kind:"focus" }` rejects a
   missing `shape ∈ {vertical-slice, cross-cutting, foundational}` as
   `invalid_params`. *(Leg A.)*
5. **The task→done CLOSURE gate.** A `closure_gate='hard'` story blocks any child
   task's `task → done` while any of that task's acceptance criteria are
   unchecked. Set via `set_closure_gate` (leg B); enforced at `transition_status`
   / `complete_task` (leg G); tick criteria with `check_acceptance_criterion`.
6. **The epic→done gate.** An epic is `done` only when ALL close-criteria are
   checked AND ALL descendant stories are terminal (`done`/`cancelled`). Both
   the deliverable gate and the structural rollup must hold; the epic done-rule
   is unconditional (no `hard`/`soft`). *(Leg G.)*
7. **The sprint ladder draft→ready→active→review→{done|cancelled}.** Legal
   transitions only; `done`/`cancelled` are terminal. **`claim_next_task`
   requires the sprint to be `active`** (it returns `{ claimed: null }`
   otherwise). *(Legs D, F.)*
8. **The checkpoint freeze.** An `in_progress` task with `checkpoint=1` (set via
   `set_task_checkpoint`) FREEZES the whole sprint's claims until it completes —
   a runtime sprint-wide freeze, NOT a task→task dependency. *(Leg F.)*
9. **The worktree-owner terminal guard.** A worktree-OWNING sprint reaches a
   terminal state ONLY via `record_worktree_merge` / `record_worktree_rejection`
   (from any source); `set_sprint_status` rejects a direct terminal
   `review→done|cancelled` on it. **Un-wedge path:** drive `active→review` via
   `set_sprint_status`, then `record_worktree_rejection` to cancel cleanly.
   *(Leg H.)*
10. **The review-lane cascade.** `complete_task` on an `implement`-lane task
    spawns a `lane='review'` task (back-linked via `reviews_work_item_id`);
    reviewer findings spawn rework via `record_finding_decision { decision:
    "spawn_task" }` (stamps `lane='implement'`, binds to the sprint → claimable).
    A `review`/NULL-lane completion spawns nothing. *(Leg F.)*
11. **The lumina server never runs git.** Git executes in the `lumina-companion`
    process (preferred — `execute_worktree_create` / `execute_worktree_merge`
    create/merge AND record in one call) or in the agent's own shell (the
    no-companion fallback), with provenance recorded via `create_worktree`,
    `record_task_commits`, and `record_worktree_merge` / `record_worktree_rejection`.
    *(Legs E, G, H.)*

> **Lane-default note (lane fix).** Tasks default to `lane='implement'` on
> creation (leg C), so planned tasks are claimable the moment the sprint is
> `active` (gate 7) — there is no manual lane-stamping step. `set_task_lane`
> exists only to re-stamp or clear a lane.

---

## Pointers

- Orchestration skills: `/lumina:create-project` (leg A), `/lumina:plan-story`
  (leg B), `/lumina:compose-sprint` (leg D), `/lumina:run-sprint` (legs F–H).
- Advisor: `/lumina:lifecycle` — "you are HERE; the next gate is X; run Y".
- MCP tool catalogue:
  [`../../../claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`](../../../claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md).
- Plugin conventions:
  [`../../../claude/plugins/lumina-story-blocks/CONVENTIONS.md`](../../../claude/plugins/lumina-story-blocks/CONVENTIONS.md).
- Architecture: [ADR-0006 git-execution companion](../../../docs/adr/0006-git-execution-companion.md)
  (incl. the 2026-06-10 detached-integration + ref-CAS amendment),
  [ADR-0005 sprint-lifecycle & worktree-ownership](../../../docs/adr/0005-sprint-lifecycle-worktree-ownership.md),
  building on [ADR-0003 commit-checkpoint-provenance](../../../docs/adr/0003-commit-checkpoint-provenance.md).
