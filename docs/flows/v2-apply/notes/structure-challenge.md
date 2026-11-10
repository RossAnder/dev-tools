Verdict: no restructure. Run each apply as its own v2 flow that the existing slug-bound engine executes unchanged in shape. Every other piece fits an existing seam: the v1 export/sync bridge is the template for a schedule/reconcile pair, `[[backlog_links]]` is the template for the task-to-finding link, the status-companion catalogue holds `blocked`, premises catch anchor drift, and the engine's step table takes the apply steps.

# Structure challenge: v2 apply

## Why the present structure holds

The work has to put review findings into an execution store, link them both ways, resolve them on a recorded commit, and drop the v1 bridge. Each of those has an existing counterpart in the code, so the change extends seams rather than moving responsibilities. One choice is load-bearing, and a planner already deep in the design might make it the other way: where the apply's tasks live. Generalising the engine to work without a flow would be the restructure. This note argues against it.

### Claim 1: run each apply as its own v2 flow, not in the reviewed flow and not through a scope-addressed engine

- The engine, the Workflow and both stores are addressed only by flow slug. Running a flow-less (`--scope`) review's fixes in place would force edits on all of them:
  - `plan::store::resolve` and `authored_dir` (`tomlctl/src/plan/store.rs:37-54`);
  - `tasks::resolve_store_path(Some(slug), None)` (`tomlctl/src/plan/target.rs:256`);
  - the Workflow's `SLUG` check, `notePath` and the worker prompt's `--slug` commands (`claude/workflows/implement-v2-milestone.js:20,124,136-138`);
  - every command in `claude/skills/implement-v2-engine/references/run-milestone.md` and `drain.md`.

  A flow-less review store has no flow at all (`tomlctl/src/review/store.rs:1058-1077`).
- The reviewed flow cannot host its own fixes in every case:
  - A flow-bound review may be bound to a v1 flow, since the envelope is built with the v1 command names (`claude/skills/review-v2-engine/SKILL.md`, stage 0, step 3). `plan.toml` must never be seeded on a v1 flow (`tomlctl/src/plan/store.rs:9-11`).
  - On a finished v2 flow, adding pending rows moves `stage` from `done` back to `running` (`tomlctl/src/plan/stage.rs:167-175`). It also puts new intent under an approval that froze the old intent (`drain.md:108`).
- A per-apply flow opened from the review store costs one new verb input and nothing structural. `plan open --from` already seeds a brief from backlog ids (`tomlctl/src/cli/types/plan.rs:115-121`). `plan approve` then needs a brief, committed stores and no unanswered decisions (`tomlctl/src/plan/approve.rs:226-250`). The triage's selection stands in for the user's approval.
- **What refutes this:** a design in which the engine must run apply tasks with no `.claude/flows/<slug>/` directory at all, for example a hard requirement that an apply leave no flow on disk.
- **One open check for the planner:** approval upserts a registry entry (`approve.rs:256`, `register` at `approve.rs:450-461`). With an empty `[binding]` the entry has no branch to match. Whether `flow resolve`'s registry-recency step could still pick an apply flow over the user's work flow is unverified. Read the step-3 and step-4 paths in `tomlctl/src/flow/resolve.rs` before settling the slug scheme.

### Claim 2: link tasks to findings on the task side, mirroring `[[backlog_links]]`, with a status and link fields on the finding side mirroring `exported`

- The task store already holds links to another store, keyed by `ref` (`tomlctl/src/tasks/schema.rs:103-105,222-233`). `tasks add` and `add-many` already accept `closes` per row (`tomlctl/src/tasks/add.rs:80,354,380`). Adding a `fixes` list in the same shape, plus the review store it names (slug or scope), gives several findings in one file one task.
- On the finding side, `Exported` with `LINK_FIELDS` (`exported_as`, `export_ledger`) is the existing "handed to another store" shape (`tomlctl/src/review/fields.rs:139-149,230`). A new status with its own companions naming the apply flow and task ref follows the same pattern. Because each status clears the others' companions (`fields.rs:135-137`), a failed or parked task needs no extra rule: the finding simply stays in that status rather than becoming `resolved`.
- The worker view inlines only what reaches the task through `binds` and contracts (`tomlctl/src/plan/view.rs:499-520`). The fix worker therefore sees its findings only if `worker()` gains a "Findings this task fixes" block that reads the link and projects each finding `--for implementer`. Copying the finding's text into the spec instead would create a second copy that can drift from the store.
- **What refutes this:** if the task-side table cannot name a store outside its own flow, then the link moves to the finding side only, and `worker()` has to search review stores to find it.

### Claim 3: `export-v1` and `sync-v1` are the template for two new verbs, `review schedule` and `review reconcile`

- `export.rs` already has every part `schedule` needs:
  - an eligibility gate (`refusal`, `tomlctl/src/review/export.rs:312-336`: advisory, `needs-plan`, `large`, not open);
  - a two-sided write with crash convergence through a back-reference (module doc at `export.rs:1-10`);
  - a per-finding mark (`mark_exported`, `export.rs:877`).

  `schedule` swaps the v1 ledger for the apply flow's `tasks.toml`.
- `sync.rs` reads the other store's outcome back into findings. The new `reconcile` has to differ from the closest relative, `backlog reconcile`, which judges by task status alone (`tomlctl/src/backlog/reconcile.rs:1-4`). Resolution here needs `status = done` and a non-empty `commit` (`tasks/schema.rs:348`). Reusing backlog's rule would resolve a finding on a `done` row whose gate never passed.
- Both verbs go beside `export.rs` and `sync.rs` in `tomlctl/src/review/`. Leave the old two in place unless the plan also rewrites `stats.rs`. `stats.rs:24` imports `sync::{SYNC_REASON_PREFIX, V1Outcome, ledger_paths, linked_rows}`, and `export.rs:25` imports `sync::{map_status, v2_refs}`, so deleting `sync.rs` forces edits in `stats`.
- **What refutes this:** an apply path that writes `resolved` in the drain directly, with no verb, could not re-converge after a crash between the commit and the review write. If the planner shows that `drain.md`'s "Stamping on re-entry" (`drain.md:366-368`) already covers that window, the separate reconcile verb is not needed.

### Claim 4: `blocked` belongs in the status and field catalogue, which already refuses incomplete rows at write

- Statuses own their companion fields (`fields.rs:118-149`). Conditional requirements are `Need` variants checked at `review add` (`fields.rs:303-315`). A blocked claim with its route (decision, evidence or design) and a required `missing` is one more status or `Need` there. A blocked row without `missing` is then refused when written, as the future state requires.
- Today's design route is `fix_shape = needs-plan`, which is defect-only (`fields.rs:263,561-564`). Cleanup must be able to carry a blocked claim too, so this field's kind set changes. `export.rs:329` and the triage presentation (`triage-and-close.md:72`) read `needs-plan` and move with it.
- **What refutes this:** a blocked claim that has to be written by an agent who cannot go through `review add` or `triage`, which would leave no write-time check to put the refusal in.

### Claim 5: anchor drift is a premise, not a new engine step

- Premises run before every launch, and a premise that fails withholds the task and its dependents (`run-milestone.md:76-129`). If each fix task gets a `base` premise that calls a read-only `tomlctl review` anchor check, drift is caught before dispatch with no engine edit. The premise-script rule that a check may only read (`run-milestone.md:88`) holds for a tomlctl read verb.
- `check_old` (`tomlctl/src/review/add.rs:673`) already checks a cleanup's `old` near its line. Defects carry no `old`, so the check needs an anchor rule of its own for them, such as the symbol at the cited file within `matching::LINE_WINDOW` (`tomlctl/src/review/matching.rs:11`).
- **What refutes this:** the planner finding that premises cannot be written at `schedule` time, because the spec is written after approval.

### Claim 6: the engine takes the apply steps through its step table, with the apply rules in one new reference

The table's "Skipped when" column (`claude/skills/implement-v2-engine/SKILL.md:46-58`) is the seam. New rows: settle ask, reproduction check, and resolve findings. Each is skipped when the milestone's rows carry no finding link. The current text forces these edits:

- **Tier rule.** "Never up a tier" and "A false premise or a failed task is replanned, never escalated" (`drain.md:123`, `SKILL.md:20`) contradict the future state, where a fix task's failure goes up a tier first. A worker's `parked` return (`implement-v2-milestone.js:34`) must also read as a failure on a fix task.
- **Settle tasks.** They edit no code, but the Workflow refuses a task with empty `files` (`implement-v2-milestone.js:107`), the launch drops one (`run-milestone.md:147`), and `WORKERS` knows only `lite` and `deep` (`implement-v2-milestone.js:24-27`). Either settle rows name the files they read and get a new executor entry in `WORKERS`, or the empty-files rule gains an exception for them.
- **Commit.** "Commits are exactly two per milestone" and step 10's staging (`SKILL.md:31`, `drain.md:329-335`) do not cover a settle milestone with nothing to stage.
- **Reproduction check.** "Vetting, apply and harvest" is listed out of scope, with "no vet pass over a worker's return" (`SKILL.md:73`). A reproduction check after a fix milestone is such a pass and blocks the commit, so that line changes and the check sits between drain and gate.
- **Settle-to-fix ask.** The milestone loop continues "while nothing in the flow is parked" (`SKILL.md:60`). The single question point between the settle and fix milestones is a new stop condition in that loop.
- **Decision-blocked findings.** Their answers can be recorded as `plan decide` records that bind the fix task, which reach the worker through the existing decision block (`view.rs:499-509`).

Put these deltas in one new `references/findings.md`, linked from the new table rows, rather than writing exceptions into each plan-path paragraph of `drain.md`. Plan runs then keep reading the text they read now. This is a new reference file, not a restructure of the engine.

- **What refutes this:** a reproduction check or settle ask that has to run inside the Workflow rather than between runs. A Workflow has no user hook (`SKILL.md:18`), so this is unlikely for the ask, but the planner should confirm it for the verifier dispatch.

### Claim 7: the close-summary measures need a stats source that does not depend on v1

- `--outcomes` reads only v1 links (`tomlctl/src/review/stats.rs:117-121,636-672`). Once triage stops exporting, it reports nothing for new runs.
- `--plan-outcomes` already derives dispositions from the store alone (`stats.rs:10-12,985`). The apply outcomes, the blocked rate per lens and per agent, and the count of settle tasks that turned out mechanical follow that pattern. They read the finding's status, its verdicts, its cell's agent and the linked apply flow's rows.
- The review engine's stage 0, step 4 (`review sync-v1`) and stage 8, steps 3-4 (export and the `/review-apply` hand-off; `triage-and-close.md:134-147`) are replaced by `reconcile` and `schedule` respectively.
- **What refutes this:** keeping v1 export as an opt-in beside the new path, in which case `outcomes()` needs both sources rather than a replacement.

## Alternatives ruled out

- **A scope-addressed engine and task store.** Ruled out under claim 1 by the slug-bound surfaces it would force edits on, for no gain over a per-apply flow.
- **Fix tasks inside the reviewed flow.** Ruled out under claim 1: v1 flows cannot take `plan.toml`, and a finished v2 flow's frozen intent would have to absorb new work.
- **Tasks stored inside `review-v2.toml`.** This would duplicate the task schema, milestones, premises and run records that `tasks.toml` and `plan.toml` already own, and the engine reads none of it.
- **A separate settle Workflow.** The intent requires the same engine and Workflow, and settle is a milestone kind within it.
- **Reusing `backlog reconcile` for findings.** Its rule judges by status alone and ignores the commit stamp (claim 3).

## Coverage

Files read in full:
- `claude/skills/implement-v2-engine/SKILL.md`
- `claude/skills/implement-v2-engine/references/run-milestone.md`
- `claude/workflows/implement-v2-milestone.js`
- `claude/skills/review-v2-engine/references/triage-and-close.md`
- `tomlctl/src/plan/target.rs`
- `tomlctl/src/review/target.rs`
- `tomlctl/src/review/mod.rs`
- `tomlctl/src/tasks/finding.rs`

Files read in part:
- `claude/skills/implement-v2-engine/references/drain.md` (steps 5-10)
- `tomlctl/src/review/fields.rs` (status enum and field catalogue)
- `tomlctl/src/review/store.rs` (header and target resolution)
- `tomlctl/src/tasks/schema.rs` (store, link, spec and row types)
- `tomlctl/src/plan/approve.rs` (approve, register, activate)
- `tomlctl/src/plan/stage.rs` (assess)
- `tomlctl/src/plan/store.rs` (header and paths)
- `tomlctl/src/plan/view.rs` (`worker`)
- `tomlctl/src/review/export.rs` (header and `refusal`)
- `tomlctl/src/review/stats.rs` (v1 coupling, by grep)
- `tomlctl/src/backlog/reconcile.rs` (header)
- `tomlctl/src/tasks/backlog_refs.rs` (header)
- `tomlctl/src/tasks/add.rs` (link handling, by grep)
- `tomlctl/src/review/add.rs` (anchor checks, by grep)
- `tomlctl/src/flow/active.rs` (upsert)
- `claude/skills/review-v2-engine/SKILL.md` (stages 0-1)
- `claude/workflows/plan-v2-investigate.js` (header)

Not read:
- `drain.md` steps 1-4 and 11
- the `plan-worker` agents and the `review-verifier` agent
- `review-v2-engine/references/plan-flow.md`
- `tomlctl/src/flow/resolve.rs` past its header, which is the open check under claim 1.
