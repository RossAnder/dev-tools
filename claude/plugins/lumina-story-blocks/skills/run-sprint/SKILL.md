---
name: run-sprint
description: Drive an active sprint to a recorded merge — single-agent worker loop over the team-execution work-queue, then the companion-executed merge with lumina recording provenance.
arguments: [sprint_id]
argument-hint: "[sprint_id]"
disable-model-invocation: true
---

# `lumina:run-sprint`

Execution-loop runner: drives ONE sprint from `active` to a recorded merge of
its shared worktree. The runner claims, works, and completes tasks against the
durable team-execution work-queue (migration 0013), honours the migration-0016
sprint-lifecycle + checkpoint-freeze + worktree-ownership rules, and ends by
triggering the merge via `execute_worktree_merge` — the companion now merges
for you. The SERVER stays **record-only** and never shells to git; the git
itself runs in the separate `lumina-companion` process (ADR-0006), which
merges on lumina's behalf and records the audit with the companion's
ground-truth sha. (The agent still runs its own commits in the shared
worktree; manual `git merge` + `record_worktree_merge` remains the
no-companion fallback/audit path — see Step 6.)

The **single-agent worker loop (Steps 2–6) is the CANONICAL path** — the safe
default. An agent-team variant is a clearly-labelled SECONDARY appendix at the
end; do NOT reach for it unless the user explicitly opts into agent teams.

Cites the shared contract at [`../../CONVENTIONS.md`](../../CONVENTIONS.md):
§a (five keys, NOT forked — the runner stays inline so checkpoint/quiescence
decisions are user-visible), §c (one §c rollup at the end; the per-task
`record_task_activity` progress entries are operational, not planning writes —
they carry `origin: "implement"`), §e (Sentry — runner decides loop control;
MCP owns leasing/cascade/lifecycle state).

## MCP tools used directly by this runner

Work-queue (migration 0013):

- `claim_next_task { sprint_id, lane, tier?, agent_id, lease_ttl_secs }` — the
  race-free claim primitive; returns `{ claimed: null }` (NOT an error) when no
  task is ready OR the sprint is not runnable (frozen / not `active`).
- `renew_lease { task_id, agent_id, lease_ttl_secs }` — heartbeat at ~half-TTL while working.
- `record_task_activity` — per-task progress entries (`origin: "implement"`).
- `complete_task { task_id, agent_id }` — transition `done` + clear lease; for an
  `implement`-lane task ALSO spawns the back-linked `review`-lane task.
- `release_task { task_id, agent_id }` — owner-guarded yield; ONLY on a true abandon.
- `get_sprint_quiescence { sprint_id }` — the lead's termination/escalation poll.
- `list_open_questions_for_sprint { sprint_id }` — arbiter surface for blocked questions.

Review cascade:

- `add_finding` — reviewer files critique findings (`kind: "code-review"`).
- `record_finding_decision { finding_id, decision: "spawn_task", ... }` — spawns a
  rework task already stamped `lane='implement'` + `tier=NULL` + bound to the sprint
  (so rework re-enters the implement-lane claim with no manual lane-stamp).

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

The runner ALSO invokes the repo-wide `commit-conventions` skill (NOT a
`/lumina:*` skill — it lives at `claude/skills/commit-conventions/`) to draft
clean git messages. Keep commit↔task cross-refs in lumina (via
`record_task_commits`), NOT in commit-message trailers.

## Body

### Step 1 — pre-flight (sprint active + exactly one shared worktree)

1. Read the sprint. If its status is `draft` or `ready`, raise it to `active`
   via `set_sprint_status({ sprint_id: "$sprint_id", status: "active" })` (the
   claim guard requires `active`). If it is already `active`, no-op. If it is
   terminal (`done` / `cancelled`), ABORT: `"sprint <id> is <status> — cannot
   run a terminal sprint."`
2. Assert EXACTLY ONE shared worktree exists for the sprint:
   `list_worktrees` (or `get_worktree` on a known id) → confirm one worktree
   whose `owning_sprint_id == $sprint_id`. There is ONE shared sprint worktree,
   never a per-worker worktree. Confirm the on-disk worktree exists too —
   compose-sprint minted it earlier via the companion's
   `execute_worktree_create` (or, on the no-companion fallback, the agent ran
   the real `git worktree add` and recorded it via `create_worktree`). If zero
   or more than one, ABORT and ask the user to reconcile (the SERVER does not
   create on-disk worktrees itself; only the companion does, on its behalf).
3. Mint a stable `agent_id` for this run (e.g. `run-sprint-<short-uuid>`) and a
   `lease_ttl_secs` (the default 30 min is generous; pick a shorter value for a
   chatty loop). Hold both for every `claim_next_task` / `renew_lease` /
   `complete_task` / `release_task` call.

### Step 2 — worker loop (single agent — CANONICAL)

Loop, working entirely inside the SHARED worktree on disk:

1. `claimed = claim_next_task({ sprint_id, lane: "implement", agent_id,
   lease_ttl_secs })`. (Drain the implement lane first; sweep the `review` lane
   in the same loop — see Step 3 lane handling. Pass `tier` only if this worker
   is specialised to a tier.)
2. If `claimed == null`, BREAK to Step 5 (quiescence check) — null means no
   ready task OR the sprint is frozen by an in-progress checkpoint task OR the
   sprint is not `active`. Do not treat null as an error.
   **Lane-fix note**: a planned task DEFAULTS to `lane='implement'` at create,
   so the implement-lane claim surfaces planned tasks DIRECTLY — there is no
   pre-stamp step and no finding-spawn detour.
3. If the claimed task carries `checkpoint=1`, jump to Step 4 (checkpoint
   protocol) instead of working it normally.
4. Work the task in the shared worktree. While working, `renew_lease` at
   ~half the TTL so the lease never lapses mid-task (a lapsed lease is lazily
   reclaimed by the next claim — another worker could steal the task). Append
   progress via `record_task_activity({ work_item_id: <task_id>, entry_type:
   "execution", origin: "implement", summary: "...", body:
   "session=${CLAUDE_SESSION_ID}" })` (apply the §c substitution guard —
   `session=unknown` + warn on non-substitution).
5. On completion, `complete_task({ task_id, agent_id })`. Treat completion
   defensively: `complete_task` is idempotent (safe to re-run across the
   two-txn window), so on an AMBIGUOUS failure (network/timeout, unknown
   outcome) RETRY `complete_task` rather than releasing. Call `release_task`
   ONLY on a TRUE abandon (you are deliberately yielding unfinished work back
   to the queue) — never as error-recovery for an uncertain completion.
6. Re-loop to Step 2.1.

### Step 3 — lane handling (implement ⇄ review cascade)

Lanes are first-class (`work_items.lane ∈ implement | review`, NULL = not
team-managed):

- **Completing an `implement`-lane task** spawns a `lane='review'` review task
  (done by `complete_task` itself): a new task under the impl task's story,
  back-linked via `reviews_work_item_id`, with a task→task dep edge so it
  CANNOT be claimed until the impl task is `done`, and bound to the sprint.
  The runner does nothing extra — the cascade is server-side.
- **Claiming the `review` lane**: in the same worker loop, also
  `claim_next_task({ sprint_id, lane: "review", agent_id, lease_ttl_secs })`
  (e.g. when the implement lane returns null but quiescence is not yet
  terminal). The reviewer reads the impl task's diff, files critique via
  `add_finding` (`kind: "code-review"`), and for each actionable finding calls
  `record_finding_decision({ finding_id, decision: "spawn_task", ... })` —
  which stamps a NEW `lane='implement'` rework task (already `tier=NULL`,
  sprint-bound, claimable). That rework re-enters the implement lane with NO
  manual lane-stamp.
- **Completing a `review`-lane task spawns NOTHING** (the cascade stops at one
  hop — prevents an infinite review→review loop). Rework, if any, came from the
  `spawn_task` finding-decision above, not from `complete_task`.

A practical single-agent order: drain implement-lane claims; when implement
returns null, sweep review-lane claims; a review may spawn fresh implement
rework, so re-check the implement lane after each review sweep. Continue until
BOTH lanes return null in the same pass, then fall through to Step 5.

### Step 4 — checkpoint protocol (sprint-wide freeze)

A `checkpoint=1` task (set via `set_task_checkpoint`) FREEZES the WHOLE sprint
while it is `in_progress` — `claim_next_task` returns null sprint-wide (a
runtime freeze, NOT a task→task dep). This is the barrier for shared-file /
consolidated-commit work. When you claim such a task:

1. QUIESCE peers: poll `get_sprint_quiescence` until `in_progress` (excluding
   this checkpoint task) reaches 0 — all other workers have parked. (Single
   agent: you are the only worker, so this is immediate; the poll matters under
   the team appendix.)
2. Make ONE consolidated REAL commit on the shared worktree (use the
   `commit-conventions` skill for the message; NO harness trailers).
3. Record provenance: `record_task_commits({ commit_sha: <the commit>,
   task_ids: [<every task id this batch commit implements>], sprint_id:
   "$sprint_id" })`. Idempotent via `UNIQUE(commit_sha, task_id)`.
4. `complete_task({ task_id: <checkpoint task>, agent_id })` to LIFT the freeze
   — claims resume sprint-wide.

Re-loop to Step 2.

### Step 5 — lead / quiescence (terminate or escalate)

When the worker loop drains (both lanes return null), poll
`get_sprint_quiescence({ sprint_id })` and branch on the verdict:

- **`done`** (`claimable==0 && in_progress==0 && blocked==0`) → proceed to
  Step 6 (finalize / merge).
- **`blocked_on_question > 0`** → resolve via
  `list_open_questions_for_sprint({ sprint_id })`: answer code/convention
  questions directly with `resolve_open_question` (pick the enabling option —
  this unblocks that branch's tasks and cancels the others'), and ESCALATE
  genuine product calls to the human (who answers via
  `POST /open-questions/{id}/resolve`). After resolving, re-loop to Step 2 —
  unblocked tasks are now claimable.
- **`stalled`** (`blocked>0 && claimable==0 && in_progress==0`) → no progress
  is possible without an arbiter. Surface the stall to the user with the open
  questions / blocked task ids; do NOT spin. Resolve or escalate, then re-loop
  to Step 2; if nothing can unblock it, treat the sprint as un-wedge-needed
  (Step 7).

Re-poll after each resolution; only a terminal `done` verdict gates Step 6.

### Step 6 — finalize / merge (companion-executed)

Only AFTER quiescence reports `done`:

1. `set_sprint_status({ sprint_id, status: "review" })` — flip `active→review`.
   (This is the LAST `set_sprint_status` call the runner makes; the terminal
   `review→done` flip is driven by the merge RECORD — `execute_worktree_merge`
   records it for you, or `record_worktree_merge` on the manual fallback — NOT
   by `set_sprint_status`; see the un-wedge note for why.)
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

### Step 7 — un-wedge a stuck sprint (record-only abandon)

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

### Step 8 — §c provenance rollup (ONE post-run entry)

After the run ends (merged, rejected, or aborted), append exactly ONE rollup
to the sprint (or its lead story). Apply the §c substitution guard verbatim.

```
mcp__lumina__record_task_activity {
  work_item_id: "$sprint_id",
  entry_type: "execution",
  origin: "implement",
  summary: "run-sprint: completed=<n> review=<n> rework=<n> checkpoints=<n>; outcome=<merged|rejected|aborted>",
  body: "session=${CLAUDE_SESSION_ID}; worktree=<worktree_id>; merge_ref=<ref-or-none>"
}
```

`origin` is `"implement"` (this is execution, not planning — contrast the §c
default `origin: "plan"` for the planning-block skills). On non-substitution,
write `session=unknown` and warn.

## Sentry-pattern compliance (per §e)

Runner decides: pre-flight order, lane sweep order (implement-first, review
after drain), the checkpoint quiesce→commit→record→complete sequence, the
quiescence poll cadence, and the un-wedge two-step. Runner MUST NOT replicate
server-owned state: leasing + reclaim (`claim_next_task` / `renew_lease`), the
done→review→rework cascade (`complete_task` / `record_finding_decision`), the
sprint-lifecycle transition legality and the terminal-flip guard
(`set_sprint_status` / `execute_worktree_merge` / the fallback
`record_worktree_merge` / `record_worktree_rejection`), and commit-provenance
idempotency (`record_task_commits`) all live in lumina. The runner runs its
own git commits in the shared worktree; the MERGE is executed by the
`lumina-companion` process via `execute_worktree_merge` (the SERVER stays
record-only and never shells to git — ADR-0006), with manual `git merge` + the
fallback `record_worktree_merge` reserved for the no-companion case.

## Appendix — agent-team variant (SECONDARY, opt-in)

> **This is NOT the default.** The single-agent loop above (Steps 2–6) is the
> canonical, safe path. Use agent teams only when the user explicitly opts in
> (e.g. a large, well-decomposed sprint where parallelism pays). The lifecycle,
> checkpoint-freeze, lane-cascade, and merge steps are UNCHANGED; only the
> worker fan-out differs.

Enable with the experimental flag `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`. The
team shares ONE sprint worktree on disk — NEVER a per-worker worktree (the
checkpoint barrier and the single Step-6 merge both assume one shared
worktree).

Team tools used:

- `claim_next_task` (each worker claims with its OWN `agent_id`, a `lane`, and
  optionally a `tier` so lite/deep workers self-select) — the atomic claim is
  exactly the race-free primitive a shared in-process list cannot provide.
- `renew_lease` — each worker heartbeats its own lease at ~half-TTL.
- `complete_task` — each worker completes its own claims (the review cascade is
  identical to the single-agent path).
- `get_sprint_quiescence` — the LEAD (one designated agent) polls for the
  terminal verdict and owns Step 6 finalize; workers do NOT each merge.
- `list_open_questions_for_sprint` — the lead (arbiter) resolves/escalates
  blocked questions; workers park via `release_task` and pull the next task.
- peer `SendMessage` — workers coordinate over `file_overlap_warnings`
  (advisory, never a gate per ADR-0002) and the lead signals the checkpoint
  quiesce. lumina provides NO peer-messaging tool; that is the harness's
  agent-team channel.

Checkpoint under teams: the freeze is sprint-wide, so when the lead (or any
worker) holds an `in_progress` `checkpoint=1` task, every other worker's
`claim_next_task` returns null. The checkpoint holder polls
`get_sprint_quiescence` until peers' `in_progress` reaches 0, makes the ONE
consolidated commit, `record_task_commits`, then `complete_task` to lift the
freeze for the whole team.

## Pointers

- Shared contract: [`../../CONVENTIONS.md`](../../CONVENTIONS.md) §a, §c, §e.
- MCP catalogue: [`../mcp/SKILL.md`](../mcp/SKILL.md) — team-execution work-queue
  (migration 0013) + sprint-lifecycle / worktree / commit-provenance (migration 0016) sections.
- Commit messages: the repo-wide `commit-conventions` skill at `claude/skills/commit-conventions/`.
- ADRs: [`docs/adr/0002-sprint-execution-architecture.md`](../../../../docs/adr/0002-sprint-execution-architecture.md) (advisory file-overlap), [`docs/adr/0003-commit-checkpoint-provenance.md`](../../../../docs/adr/0003-commit-checkpoint-provenance.md), [`docs/adr/0005-sprint-lifecycle-worktree-ownership.md`](../../../../docs/adr/0005-sprint-lifecycle-worktree-ownership.md), [`docs/adr/0006-git-execution-companion.md`](../../../../../docs/adr/0006-git-execution-companion.md) (the companion execution plane behind `execute_worktree_merge`).
