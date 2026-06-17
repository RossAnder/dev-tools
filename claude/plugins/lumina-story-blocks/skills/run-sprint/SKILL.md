---
name: run-sprint
description: Drive an active sprint to a recorded merge — agent-team worker fan-out by DEFAULT (auto-degrading to a single-agent loop when CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS or project/user-level lumina MCP is unavailable) over the team-execution work-queue, then the companion-executed merge with lumina recording provenance.
arguments: [sprint_id]
argument-hint: "[sprint_id]"
disable-model-invocation: true
---

# `lumina:run-sprint`

Execution-loop runner: drives ONE sprint from `active` to a recorded merge of
its shared worktree. The runner claims, works, and completes tasks against the
durable team-execution work-queue (migration 0013), honours the migration-0016
sprint-lifecycle + checkpoint-freeze + worktree-ownership rules, and ends by
triggering the merge via `execute_worktree_merge` — the companion merges for
you. The SERVER stays **record-only** and never shells to git; the git itself
runs in the separate `lumina-companion` process (ADR-0006), which merges on
lumina's behalf and records the audit with the companion's ground-truth sha.
(Workers still run their own commits in the shared worktree; manual `git merge`
+ `record_worktree_merge` remains the no-companion fallback/audit path — see
Step 7.)

The **agent-team worker fan-out (Step 2) is the DEFAULT path** — a bounded pool
of teammate workers drains the work-queue in parallel under one lead. When
agent teams are unavailable the runner **auto-degrades** to the single-agent
worker loop, which is the explicitly-labelled fallback (`## Auto-degrade —
single-agent worker loop` near the end). The degrade is automatic and silent on
the happy path: the runner DETECTS team availability at pre-flight (Step 1.3)
and picks the topology for you. Everything ELSE — pre-flight, the review-as-state
lifecycle, commit cadence, checkpoint freeze, quiescence, and the merge — is
IDENTICAL across both topologies; only the worker fan-out differs.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (five keys, NOT forked — the runner stays inline so topology-selection,
checkpoint, and quiescence decisions are user-visible), §c (one §c rollup at
the end; the per-task `record_task_activity` progress entries are operational,
not planning writes — they carry `origin: "implement"`), §e (Sentry — runner
decides loop control + fan-out + drift monitoring; MCP owns
leasing / the review-as-state lifecycle / sprint-lifecycle state).

## MCP tools used directly by this runner

Work-queue (migration 0013):

- `claim_next_task { sprint_id, lane, tier?, agent_id, lease_ttl_secs }` — the
  race-free claim primitive; returns `{ claimed: null }` (NOT an error) when no
  task is ready OR the sprint is not runnable (frozen / not `active`). Under a
  team, each worker claims with its OWN `agent_id` (and optionally a `tier` so
  lite/deep workers self-select) — the atomic claim is exactly the race-free
  primitive a shared in-process list cannot provide.
- `renew_lease { task_id, agent_id, lease_ttl_secs }` — heartbeat at ~half-TTL while working.
- `record_task_activity` — per-task progress entries (`origin: "implement"`).
- `complete_task { task_id, agent_id }` — close out a claimed task on its OWN row,
  routed by tier (1B-F9 review-as-state): a **deep** task (or one already flagged
  `lane='review'`) moves to the NON-terminal `review` STATE + `lane='review'` on
  the SAME row (NOT `done`, NOT reconciled — it re-enters the queue for a reviewer
  to claim); a **lite / un-flagged** task transitions straight to `done`. NO
  separate review task is ever spawned (`review_task_id` is always null — the old
  done→review SPAWN cascade is RETIRED). `complete_task` on a `lane='review'` task
  routes it BACK to review (not done), so it is NOT the review→done close path —
  the reviewer uses `transition_status`/`update_work_item_status → done` for that.
- `transition_status { id, status }` — the reviewer's clean-review close path:
  flips the claimed `review`-state row `review → done` on the SAME row (the
  files_touched reconcile fires HERE). Rejects a `done → review` flip — `done` is
  terminal, so re-reviewing completed work needs a brand-NEW task.
- `release_task { task_id, agent_id }` — owner-guarded yield; ONLY on a true abandon.
- `get_sprint_quiescence { sprint_id }` — the lead's termination/escalation + drift
  poll. Now carries an `in_review` bucket: UNCLAIMED `review`-state tasks are
  NON-terminal (they keep the sprint not-`done`); a no-claimer review surfaces as
  `stalled` (needs a reviewer) rather than hanging the sprint invisibly.
- `list_open_questions_for_sprint { sprint_id }` — arbiter surface for blocked questions.
- `compute_task_batches { story_id }` — Kahn phase batches over the task subtree; the
  lead reads these to drive the phase-batch-boundary commit cadence (Step 4).

Review findings → rework (NEW implement tasks, never a review-task spawn):

- `add_finding` — reviewer files critique findings (`kind: "code-review"`).
- `record_finding_decision { finding_id, decision: "spawn_task", ... }` — spawns a
  rework task already stamped `lane='implement'` + `tier=NULL` + bound to the sprint
  (so rework re-enters the implement-lane claim with no manual lane-stamp). A
  task-hosted finding's rework nests under the host task's parent STORY (1B-F9 MF),
  so it is hierarchy-legal AND inherits the story's sprint membership.

Sprint lifecycle / worktree / commit provenance (migration 0016):

- `set_sprint_status { sprint_id, status }` — `draft|ready→active` pre-flight; `active→review` finalize.
- `get_worktree` / `list_worktrees` — assert the one shared worktree.
- `set_task_checkpoint { task_id, on }` — (read at claim time) a `checkpoint=1` task freezes the sprint.
- `record_task_commits { commit_sha, task_ids, sprint_id }` — bind a real commit to the tasks it implements.
- `execute_worktree_merge { worktree_id }` — EXECUTE the merge via the connected
  `lumina-companion` (ADR-0006); on success it records the audit itself (through
  the same `record_worktree_merge` mutation, with the companion's ground-truth
  sha) and drives the owner `review→done`. The PRIMARY merge path.
- `record_worktree_merge { worktree_id, merge_ref }` — FALLBACK/audit path only:
  record a merge the agent performed manually with real `git merge` (no
  companion connected); drives the owner `review→done`.
- `record_worktree_rejection { worktree_id, reason }` — the reject counterpart; drives the owner `review→cancelled`.

Team coordination is the HARNESS's job, not lumina's: teammate fan-out and peer
`SendMessage` (advisory `file_overlap_warnings`, lead nudges) ride the agent-team
channel — lumina provides NO peer-messaging tool. The runner ALSO invokes the
repo-wide `commit-conventions` skill (NOT a `/lumina:*` skill — it lives at
`claude/skills/commit-conventions/`) to draft clean git messages. Keep
commit↔task cross-refs in lumina (via `record_task_commits`), NOT in
commit-message trailers.

## Body

### Step 1 — pre-flight (sprint active + one shared worktree + topology selection)

1. Read the sprint. If its status is `draft` or `ready`, raise it to `active`
   via `set_sprint_status({ sprint_id: "$sprint_id", status: "active" })` (the
   claim guard requires `active`). If it is already `active`, no-op. If it is
   terminal (`done` / `cancelled`), ABORT: `"sprint <id> is <status> — cannot
   run a terminal sprint."`
2. Assert EXACTLY ONE shared worktree exists for the sprint:
   `list_worktrees` (or `get_worktree` on a known id) → confirm one worktree
   whose `owning_sprint_id == $sprint_id`. There is ONE shared sprint worktree,
   never a per-worker worktree — the checkpoint barrier and the single Step-7
   merge both assume it. Confirm the on-disk worktree exists too — compose-sprint
   minted it earlier via the companion's `execute_worktree_create` (or, on the
   no-companion fallback, the agent ran the real `git worktree add` and recorded
   it via `create_worktree`). If zero or more than one, ABORT and ask the user to
   reconcile (the SERVER does not create on-disk worktrees itself; only the
   companion does, on its behalf).
3. **SELECT TOPOLOGY — team (default) vs single-agent (auto-degrade).** Run the
   team topology UNLESS one of these degrade gates fires; if either fires,
   auto-degrade to the single-agent fallback and state which gate fired:
   - **(a) Agent-teams availability**: agent teams require the experimental flag
     `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` in the environment. If it is
     UNSET/not `1`, you cannot spawn teammates → DEGRADE to single-agent.
   - **(b) lumina MCP at project/user scope**: a teammate subagent does NOT
     inherit a per-subagent `mcpServers` frontmatter block — that is DROPPED
     upstream; teammates inherit MCP servers ONLY from PROJECT (`.mcp.json`) or
     USER settings. So BEFORE spawning teammates, confirm the `lumina` server is
     configured at project or user scope (e.g. `.mcp.json` carries a `lumina`
     HTTP entry). If it is NOT, teammates would be unable to call
     `claim_next_task` / `complete_task` → DEGRADE to single-agent (the lead, in
     the main session, keeps its lumina MCP either way).

     A handy assertion: the lead itself reaching `mcp__lumina__*` does not prove
     project/user scope (the lead may have it via a session/frontmatter source a
     teammate won't see) — verify the `.mcp.json` / user-settings entry exists,
     don't infer it from your own access.
4. Mint a stable `agent_id` for the lead (e.g. `run-sprint-<short-uuid>`); each
   teammate gets its OWN derived `agent_id` (e.g. `run-sprint-<uuid>-w<n>`). Pick
   a `lease_ttl_secs` with a **floor of 600s (10 min)** — short enough that a
   stalled worker's task is reclaimable promptly, long enough that a deep task's
   work between heartbeats never lapses; the default 1800s (30 min) is a fine
   ceiling. Hold the id + TTL for every `claim_next_task` / `renew_lease` /
   `complete_task` / `release_task` call.

### Step 2 — worker fan-out: agent-team topology (DEFAULT)

The team shares ONE sprint worktree on disk (Step 1.2). One agent is the
**LEAD** (the invoking session): it owns topology selection, fan-out, the commit
cadence (Step 4), checkpoint quiesce (Step 5), quiescence/drift monitoring
(Step 6), and the single Step-7 finalize. The lead spawns a bounded pool of
**teammate workers**; each teammate runs the worker loop below against the
shared queue.

**Fan-out cap — 3 to 5 concurrent teammates.** Spawn a BOUNDED pool (3–5 per the
upstream agent-teams guidance), NOT one teammate per task: the pool drains the
queue by re-claiming as tasks complete. More than ~5 thrashes the shared
worktree + cargo build lock and yields diminishing returns; fewer than 3
underuses parallelism on a well-decomposed sprint. Size to the sprint's claimable
breadth (a 3-task sprint needs ~2–3 workers, not 5). The lead MAY also work the
queue itself between monitoring duties.

Each teammate loops, working entirely inside the SHARED worktree on disk:

1. `claimed = claim_next_task({ sprint_id, lane: "implement", agent_id: <own>,
   lease_ttl_secs })`. (Drain the implement lane first; sweep the `review` lane
   in the same loop — see Step 3. Pass `tier` only if this worker is specialised
   to a tier, so lite/deep workers self-select.)
2. If `claimed == null`, the implement lane is (for now) dry — sweep the review
   lane (Step 3); if BOTH return null, park and report to the lead. Null means no
   ready task OR the sprint is frozen by an in-progress checkpoint task OR the
   sprint is not `active` — never an error.
   **Lane-fix note**: a planned task DEFAULTS to `lane='implement'` at create, so
   the implement-lane claim surfaces planned tasks DIRECTLY — there is no
   pre-stamp step and no finding-spawn detour.
3. If the claimed task carries `checkpoint=1`, hand control to the checkpoint
   protocol (Step 5) instead of working it normally — a checkpoint freezes the
   whole sprint, so it is a lead-coordinated barrier, not ordinary work.
4. Work the task in the shared worktree. **Renew at ~half the TTL**
   (`renew_lease`) so the lease never lapses mid-task (a lapsed lease is lazily
   reclaimed by the next claim — a peer could steal the task). Append progress via
   `record_task_activity({ work_item_id: <task_id>, entry_type: "execution",
   origin: "implement", summary: "...", body: "session=${CLAUDE_SESSION_ID}" })`
   (apply the §c substitution guard — `session=unknown` + warn on
   non-substitution). The `file_overlap_warnings` on the claim are ADVISORY
   (never a gate, per ADR-0002) — coordinate over them with peers via
   `SendMessage`, do not block on them.
5. On completion, `complete_task({ task_id, agent_id: <own> })`. Treat completion
   defensively: `complete_task` is idempotent (safe to re-run across the two-txn
   window), so on an AMBIGUOUS failure (network/timeout, unknown outcome) RETRY
   `complete_task` rather than releasing. Call `release_task` ONLY on a TRUE
   abandon (deliberately yielding unfinished work back to the queue) — never as
   error-recovery for an uncertain completion.
6. Re-loop to 2.1.

**Lead responsibilities run concurrently with the pool** — fan-out sizing, the
Step-4 commit cadence, the Step-5 checkpoint quiesce, and the Step-6
quiescence/drift monitoring (the lead nudges teammates that stop on an error or
finish work without marking it complete). The lead owns the single Step-7
finalize; teammates do NOT each merge.

### Step 3 — review is a LANE/STATE on the SAME task (review-as-state) [shared]

**1B-F9 redesign: review is NOT a spawned task — it is a non-terminal STATE the
implemented task itself enters.** There is no separate review work-item, no
`reviews_work_item_id` back-link, no copied `files_touched`, and no done→review
SPAWN cascade. A task carries its OWN row through implement → review → done.
Lanes stay first-class (`work_items.lane ∈ implement | review`, NULL = not
team-managed); the lifecycle is IDENTICAL under teams and single-agent:

- **Completing a DEEP (or already-flagged) implement task** routes the SAME row
  to the NON-terminal `review` STATE + `lane='review'` (done by `complete_task`
  itself — `review_task_id` is always null). The row leaves the implement claim
  set and enters the review claim set; the files_touched reconcile is DEFERRED
  until the row later reaches `done`. A **lite / un-flagged** task instead
  completes straight to `done` (no review state) — trivial mechanical work is not
  re-reviewed by default; flag it for review by stamping `lane='review'`
  (`set_task_lane`) before completing if a reviewer should still see it.
- **Dedicated review agent(s) claim the `review` lane CONTINUOUSLY**, decoupled
  from the checkpoint/commit cadence: `claim_next_task({ sprint_id, lane:
  "review", agent_id, lease_ttl_secs })` atomically claims a review-state row
  (M2 widened the claim's readiness predicate to admit `status='review' AND
  lane='review'`). The claim flips it to `in_progress` on the review lane — the
  SAME atomic primitive the implement lane uses, so two reviewers never
  double-claim the same row. Reviewing is NOT gated on a quiesce-before-review
  barrier (that would serialise the whole sprint and defeat continuous review) —
  reviewers run alongside implementers throughout.
- **A clean review → `done`** via `transition_status`/`update_work_item_status`
  on the SAME row (the files_touched reconcile fires HERE). Do NOT use
  `complete_task` to close a review: on a `lane='review'` row it routes BACK to
  the review state, so it is not the close path. The row is NEVER reopened —
  `done` is terminal, and a `done → review` flip (or flagging a `done` task into
  the review lane) is rejected; re-reviewing completed work needs a brand-NEW task.
- **Review findings spawn NEW implement tasks, not rework reviews.** The reviewer
  files critique via `add_finding` (`kind: "code-review"`) and, for each
  actionable finding, `record_finding_decision({ finding_id, decision:
  "spawn_task", ... })` — which stamps a NEW `lane='implement'` task (already
  `tier=NULL`, sprint-bound, claimable) under the finding's parent STORY (a
  task-hosted finding lifts to its parent story). That rework re-enters the
  implement lane with no manual lane-stamp.

**Isolation contract (the server is RECORD-ONLY — it ships no git isolation
primitive, so review scopes by the task's own footprint, NOT a whole-tree diff):**

- Review the task's `files_touched`-scoped diff — the paths the task's
  first-class `task_files` set names — never the whole working tree (a shared
  worktree carries every concurrent task's edits; a whole-tree diff would
  attribute peers' work to this task).
- For work ALREADY swept into a checkpoint commit, the working tree is empty for
  those paths — diff the task's recorded `task_commits` SHAs
  (`list_task_commits`) instead of the now-empty working tree, scoped to the
  task's `task_files`.
- For UNCOMMITTED work, diff the working tree scoped to the task's `task_files`.

A practical drain order (per worker): drain implement-lane claims; when implement
returns null, sweep review-lane claims (the review-state rows DEEP completions
left); a review's findings may spawn fresh implement rework, so the pool
re-checks the implement lane after each review sweep. Continue until BOTH lanes
return null sprint-wide (and `get_sprint_quiescence` reports `in_review=0`), then
the lead falls through to Step 6.

### Step 4 — commit cadence (Kahn phase-batch boundaries) [shared]

The lead owns committing; workers do their edits in the shared worktree and the
lead lands them. Commit at **Kahn phase-batch boundaries**, plus checkpoints,
plus a final close:

1. Read the phase batches with `compute_task_batches({ story_id })` (Vec of
   phases, each a set of tasks dispatchable once its predecessors are done).
   Dispatch and complete a phase's tasks, then make ONE commit at the boundary
   before the next phase's tasks land their edits.
2. For each boundary commit: draft the message with the `commit-conventions`
   skill (NO harness trailers), then record provenance with
   `record_task_commits({ commit_sha, task_ids: [<every task id whose edits this
   commit captures>], sprint_id: "$sprint_id" })` (idempotent via
   `UNIQUE(commit_sha, task_id)`).
3. **Accept an occasional dirty snapshot.** Because teammates work concurrently
   and the Kahn boundaries are NOT hard barriers (tasks pipeline across phases as
   their deps clear), a boundary commit MAY capture a partially-edited file from
   an already-started next-phase task. That is ACCEPTED — under team concurrency
   bisectability + per-commit provenance are best-effort, not a guarantee; the
   checkpoint consolidated commits (Step 5) and the final close commit before the
   merge (Step 7) reconcile the remainder. Do NOT stall the pool waiting for a
   perfectly clean tree at every boundary.
4. A `checkpoint=1` task forces a consolidated commit at a sprint-wide freeze —
   that is the Step-5 special case of this cadence.

### Step 5 — checkpoint protocol (sprint-wide freeze) [shared]

A `checkpoint=1` task (set via `set_task_checkpoint`, typically by compose-sprint
from cross-task `files_touched` overlap) FREEZES the WHOLE sprint while it is
`in_progress` — `claim_next_task` returns null sprint-wide (a runtime freeze, NOT
a task→task dep). This is the barrier for shared-file / consolidated-commit work.
When the checkpoint task is claimed:

1. QUIESCE peers: poll `get_sprint_quiescence` until `in_progress` (excluding
   this checkpoint task) reaches 0 — all other workers have parked (their claims
   now return null sprint-wide). Under a team this poll is load-bearing; for a
   single agent it is immediate (you are the only worker).
2. Make ONE consolidated REAL commit on the shared worktree (use the
   `commit-conventions` skill for the message; NO harness trailers).
3. Record provenance: `record_task_commits({ commit_sha: <the commit>,
   task_ids: [<every task id this batch commit implements>], sprint_id:
   "$sprint_id" })`. Idempotent via `UNIQUE(commit_sha, task_id)`.
4. `complete_task({ task_id: <checkpoint task>, agent_id })` to LIFT the freeze
   — claims resume sprint-wide for the whole pool.

Re-loop to Step 2.

### Step 6 — lead / quiescence + drift monitoring (terminate or escalate) [shared]

The lead polls `get_sprint_quiescence({ sprint_id })` on a steady cadence — both
to detect termination AND to monitor teammate drift:

**Drift / stuck-task monitoring (lead contract).** A teammate may STOP on an
error (crash, refusal, context exhaustion) or FINISH work without marking it
complete (no `complete_task`). The lead watches for a task that stays
`in_progress` with a lease nearing expiry and no fresh `record_task_activity`:
- NUDGE the owning teammate via `SendMessage` ("renew or complete task <id>").
- If the teammate is unresponsive, let the lease LAPSE — the next
  `claim_next_task` lazily reclaims it (or the lead `release_task`s it if it
  owns the lease) so a peer re-runs it. The lead must NOT let a stuck task
  silently wedge the sprint: nudge, then rely on lease-reclaim.
- A worker that exits its loop with a task still claimed is the same case —
  reclaim via lease lapse.

**Termination verdict** — branch on the quiescence roll-up:

- **`done`** (`claimable==0 && in_progress==0 && blocked==0`) → proceed to
  Step 7 (finalize / merge).
- **`blocked_on_question > 0`** → resolve via
  `list_open_questions_for_sprint({ sprint_id })`: answer code/convention
  questions directly with `resolve_open_question` (pick the enabling option —
  this unblocks that branch's tasks and cancels the others'), and ESCALATE
  genuine product calls to the human (who answers via
  `POST /open-questions/{id}/resolve`). After resolving, the pool re-claims —
  unblocked tasks are now claimable.
- **`stalled`** (`blocked>0 && claimable==0 && in_progress==0`) → no progress
  is possible without an arbiter. Surface the stall to the user with the open
  questions / blocked task ids; do NOT spin. Resolve or escalate, then re-loop;
  if nothing can unblock it, treat the sprint as un-wedge-needed (Step 8).

Re-poll after each resolution; only a terminal `done` verdict gates Step 7.

### Step 7 — finalize / merge (companion-executed) [shared]

Only AFTER quiescence reports `done`. The LEAD owns this (workers do NOT each
merge):

1. Land the final close commit if the tree carries un-committed worker edits
   (Step 4), then `set_sprint_status({ sprint_id, status: "review" })` — flip
   `active→review`. (This is the LAST `set_sprint_status` call the runner makes;
   the terminal `review→done` flip is driven by the merge RECORD —
   `execute_worktree_merge` records it for you, or `record_worktree_merge` on the
   manual fallback — NOT by `set_sprint_status`; see the un-wedge note for why.)
2. `execute_worktree_merge({ worktree_id })` — the PRIMARY merge path. The
   connected `lumina-companion` performs the merge in a DETACHED integration
   worktree, so a checked-out target does NOT block it (no "move your primary
   checkout off the target branch" dance — that constraint is obsolete). On
   `outcome ∈ merged | already_up_to_date`, lumina records the audit itself
   with the companion's ground-truth sha and drives the owning sprint
   `review→done` — do NOT call `record_worktree_merge` afterwards. Branch on
   the response:
   - **`merged` / `already_up_to_date`** — done; the sprint is `done` with a
     recorded merge (`already_up_to_date` makes re-runs idempotent).
   - **`conflicted`** — the companion already ABORTED the merge and restored
     its worktree; NO DB write happened (`recorded: false`). Record the
     structured conflict `paths` as a finding (`add_finding`) or an open
     question and STOP — re-run only after the conflict is resolved.
   - **lease rejection ("merge already in flight" on the target)** — the
     per-target merge lease REJECTS, it does NOT queue. Retry same-target
     merges in dependency order, one at a time.
   - **`TargetMoved`** (the target branch moved or was deleted between
     tip-resolve and the atomic ref advance) — simply RE-RUN; the next attempt
     resolves the new tip. No rollback is needed.
   - **stale-primary hint** — when the response carries a `target_checkout`
     field and a human `hint` string (the target branch was checked out in
     another worktree, e.g. the operator's primary), surface the `hint` string
     to the operator VERBATIM: the stale checkout shows spurious
     "undo-the-merge" diffs until refreshed with `git reset --keep
     <merge_sha>`, and committing there would revert the merge.
3. **No-companion FALLBACK/audit path only**: if no companion is connected
   (`execute_worktree_merge` rejects with "no companion connected"), the AGENT
   performs the merge manually with real `git merge`, then records it via
   `record_worktree_merge({ worktree_id, merge_ref: <the merge commit
   sha/ref> })` — stamps `merged_at` / `merge_ref` / `outcome` AND drives the
   OWNING sprint `review→done`. Never reach for this while a companion is
   connected.

Draft any merge / commit message with the `commit-conventions` skill. Keep
commit↔task cross-refs in lumina (`record_task_commits`), NOT in
commit-message trailers.

The sprint is now `done` with a recorded merge.

### Step 8 — un-wedge a stuck sprint (record-only abandon) [shared]

A sprint stuck `active` with an OWNED worktree CANNOT be cancelled directly:
`set_sprint_status` rejects the terminal `active→cancelled` flip on a
worktree-owning sprint, and `record_worktree_rejection` needs a `review`-status
owner. Recover in TWO steps:

1. `set_sprint_status({ sprint_id, status: "review" })` — move `active→review`
   (this transition IS allowed).
2. `record_worktree_rejection({ worktree_id, reason: "<why abandoned>" })` —
   records the audit fields AND drives the owner `review→cancelled`.

State this to the user explicitly: you cannot jump `active→cancelled` on an
owned worktree; you go through `review` first.

### Step 9 — §c provenance rollup (ONE post-run entry) [shared]

After the run ends (merged, rejected, or aborted), append exactly ONE rollup
to the sprint (or its lead story). Apply the §c substitution guard verbatim.

```
mcp__lumina__record_task_activity {
  work_item_id: "$sprint_id",
  entry_type: "execution",
  origin: "implement",
  summary: "run-sprint: topology=<team|single-agent> completed=<n> review=<n> rework=<n> checkpoints=<n>; outcome=<merged|rejected|aborted>",
  body: "session=${CLAUDE_SESSION_ID}; worktree=<worktree_id>; merge_ref=<ref-or-none>"
}
```

`origin` is `"implement"` (this is execution, not planning — contrast the §c
default `origin: "plan"` for the planning-block skills). On non-substitution,
write `session=unknown` and warn.

## Auto-degrade — single-agent worker loop (fallback)

> **This is the fallback, not the default.** It runs ONLY when a Step-1.3 degrade
> gate fires — agent teams are unavailable (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`
> unset) OR lumina MCP is not configured at project/user scope (so teammates
> could not reach the work-queue). State which gate fired, then run this loop. The
> lifecycle (Step 1 pre-flight, Step 3 review-as-state, Step 4 commit cadence,
> Step 5 checkpoint freeze, Step 6 quiescence, Step 7 merge, Step 8 un-wedge,
> Step 9 rollup) is UNCHANGED; only the worker fan-out collapses to one agent.

One agent is BOTH lead and sole worker. Loop, working entirely inside the SHARED
worktree on disk:

1. `claimed = claim_next_task({ sprint_id, lane: "implement", agent_id,
   lease_ttl_secs })`. (Drain the implement lane first; sweep the `review` lane
   in the same loop — Step 3. Pass `tier` only if this worker is specialised to a
   tier.)
2. If `claimed == null`, BREAK to Step 6 (quiescence check) — null means no ready
   task OR the sprint is frozen by an in-progress checkpoint task OR the sprint is
   not `active`. Do not treat null as an error.
3. If the claimed task carries `checkpoint=1`, jump to Step 5 (checkpoint
   protocol) instead of working it normally. Single-agent: the quiesce poll is
   immediate — you are the only worker.
4. Work the task in the shared worktree. `renew_lease` at ~half the TTL so the
   lease never lapses mid-task. Append progress via `record_task_activity({
   work_item_id: <task_id>, entry_type: "execution", origin: "implement",
   summary: "...", body: "session=${CLAUDE_SESSION_ID}" })` (apply the §c
   substitution guard — `session=unknown` + warn on non-substitution).
5. On completion, `complete_task({ task_id, agent_id })`. Per Step 3 (review-as-
   state): a deep/flagged task routes to the non-terminal `review` state on its
   OWN row (re-claim it on the review lane below to review + close it `review →
   done` via `transition_status`); a lite/un-flagged task goes straight to `done`.
   `complete_task` is idempotent — on an AMBIGUOUS failure RETRY it rather than
   releasing. Call `release_task` ONLY on a TRUE abandon, never as error-recovery
   for an uncertain completion.
6. Re-loop to 1. A practical order: drain implement-lane claims; when implement
   returns null, sweep review-lane claims (the `review`-state rows deep
   completions left — claim, review the task_files-scoped diff, close `review →
   done`, or `spawn_task` rework); a review's findings may spawn fresh implement
   rework, so re-check the implement lane after each review sweep; continue until
   BOTH lanes return null in the same pass (and quiescence `in_review=0`), then
   fall through to Step 6.

There is no fan-out, no drift monitoring, and no peer `SendMessage` in this
fallback — the single agent cannot drift away from itself. Everything else
(commit cadence, checkpoint quiesce, quiescence, finalize, un-wedge, rollup) is
exactly as the shared steps describe.

## Sentry-pattern compliance (per §e)

Runner decides: pre-flight order, topology selection (team default vs
single-agent auto-degrade) + the degrade gates, fan-out sizing (3–5), the lane
sweep order (implement-first, review after drain), the Kahn phase-batch commit
cadence + dirty-snapshot tolerance, the checkpoint quiesce→commit→record→complete
sequence, the quiescence poll cadence + teammate-drift nudge/reclaim, and the
un-wedge two-step. Runner MUST NOT replicate server-owned state: leasing + reclaim
(`claim_next_task` / `renew_lease`), the tier-routed completion + same-row review
state + the review→done close (`complete_task` / `transition_status`) and the
findings→implement-rework spawn (`record_finding_decision`), the sprint-lifecycle transition
legality and the terminal-flip guard (`set_sprint_status` /
`execute_worktree_merge` / the fallback `record_worktree_merge` /
`record_worktree_rejection`), and commit-provenance idempotency
(`record_task_commits`) all live in lumina. Workers run their own git commits in
the shared worktree; the MERGE is executed by the `lumina-companion` process via
`execute_worktree_merge` (the SERVER stays record-only and never shells to git —
ADR-0006), with manual `git merge` + the fallback `record_worktree_merge`
reserved for the no-companion case. Team fan-out + peer `SendMessage` are the
HARNESS's agent-team channel, not lumina tools.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §c, §e.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — team-execution work-queue
  (migration 0013) + sprint-lifecycle / worktree / commit-provenance (migration 0016) sections.
- Checkpoint suggestion (compose-sprint): [`../compose-sprint/SKILL.md`](../compose-sprint/SKILL.md) stamps `checkpoint=1` from cross-task `files_touched` overlap.
- Commit messages: the repo-wide `commit-conventions` skill at `claude/skills/commit-conventions/`.
- ADRs: [`docs/adr/0002-sprint-execution-architecture.md`](../../../../../docs/adr/0002-sprint-execution-architecture.md) (team-default topology + advisory file-overlap), [`docs/adr/0003-commit-checkpoint-provenance.md`](../../../../../docs/adr/0003-commit-checkpoint-provenance.md), [`docs/adr/0005-sprint-lifecycle-worktree-ownership.md`](../../../../../docs/adr/0005-sprint-lifecycle-worktree-ownership.md), [`docs/adr/0006-git-execution-companion.md`](../../../../../docs/adr/0006-git-execution-companion.md) (the companion execution plane behind `execute_worktree_merge`).
