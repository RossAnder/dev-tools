Verdict: restructure: move milestone order out of "store order is the schedule" (a prose walk in run-milestone.md plus four other store-order readers) into one tomlctl milestone-schedule module and a `plan emit` leaf; scope the lane check and the pre-flight's store checks to the milestone being run

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 58
claim = "The pick is the first checkpoint in store order with a non-done row; --milestone is refused when an earlier checkpoint holds a non-done row"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 67
claim = "A deferred milestone stops the engine instead of being passed over"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 19
claim = "Pre-flight runs plan check and tasks check over the whole store and stops on any error-class finding"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 38
claim = "--milestone refusal and deferred stop restated in the engine contract"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 98
claim = "An open run recorded under another session is asked about, and the engine stops when it may still be running"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 135
claim = "Every trail row written while the run was open is attributed to a worker"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 431
claim = "The milestone commit stages the uncommitted paths among its rows' files"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 463
claim = "The flow-store commit stages all of .claude/flows/<slug>/"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 476
claim = "Continuation to the next milestone re-enters the store-order pick"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 215
claim = "The settle drain finds the fix milestone as the checkpoint after this one in store order"

[[source]]
path = "claude/skills/apply-v2-engine/SKILL.md"
line = 109
claim = "apply-v2 asks the user because the engine takes the first non-done milestone and refuses --milestone past it"

[[source]]
path = "claude/skills/apply-v2-engine/SKILL.md"
line = 167
claim = "settle-<n> must run before apply-<n>, an order carried by store adjacency rather than task needs"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 53
claim = "plan-v2's milestone-cutting rule and the deferral rule"

[[source]]
path = "claude/skills/plan-v2-engine/references/write-and-prove.md"
line = 236
claim = "Prove's cold read covers non-deferred milestones only; deferred ones are read when elaborated"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 23
claim = "The fan-out check diffs the whole tree minus notes and the flow store, and HEAD"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 88
claim = "The lane digest hashes rows, specs, milestones and changes of the whole store"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 290
claim = "The check side recomputes the digest over the store as it stands at check time"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 360
claim = "trail_writes lists every [[changes]] row in the run window regardless of which milestone it touches"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 748
claim = "launch-args treats a need as satisfied when done, without reading commit"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 1
claim = "Precedent: what the engine passes on is computed from the stores, not assembled from row reads"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 428
claim = "plan/task-no-spec is an error for any row in any milestone"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 440
claim = "Only accept-no-check is relaxed for deferred milestones; other spec errors are store-wide"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 467
claim = "plan/spec-orphan carries no ids, so it cannot be scoped to a milestone"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 729
claim = "milestone_width_findings already walks store order and skips deferred milestones"

[[source]]
path = "tomlctl/src/tasks/check.rs"
line = 665
claim = "checkpoint/invalid-cut keeps store order a valid topological order over row dependencies"

[[source]]
path = "tomlctl/src/tasks/check.rs"
line = 639
claim = "A file shared by two rows with no dependency path is only a warning"

[[source]]
path = "tomlctl/src/tasks/graph.rs"
line = 31
claim = "Node carries no commit field; graph.rs is the task-level seam"

[[source]]
path = "tomlctl/src/tasks/graph.rs"
line = 364
claim = "groups() computes per-checkpoint closures over a given order"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 369
claim = "MilestoneSpec has id, gates, elaboration, defer_reason, gate_probes and no ordering field"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 143
claim = "milestone add always appends to the end of checkpoints and milestones"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 90
claim = "Every open run becomes a blocker and the next hint, whatever its step"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 656
claim = "A Run already records step, session, run_id and started, so an elaboration can be an owned run"

[[source]]
path = "tomlctl/src/tasks/ready.rs"
line = 27
claim = "tasks ready --checkpoint gates on done only and answers one checkpoint per call"
```

## What the change does to the code as it stands

Today one implicit rule, "store order is the schedule", sits in five places. Each of them forces an edit:

1. **The pick.** `run-milestone.md:58-65` walks `[[checkpoints]]` in store order and picks the first one with a non-done row. `run-milestone.md:67-73` and `SKILL.md:77` stop on a deferred milestone. `SKILL.md:38` refuses `--milestone` when an *earlier* checkpoint still holds a non-done row. The new rule asks, for each milestone, whether every row it needs outside itself is done and committed, whether an explicit `after` holds, whether it is deferred or being elaborated, and (see 5) whether it would sweep in another milestone's started edits. The orchestrator gets none of these from today's reads. `tasks ready --checkpoint` (`ready.rs:27`) gates on `done` and never reads `commit`, answers one checkpoint per call, and knows nothing of milestone `after` edges. Done in prose, this becomes `tasks closure --checkpoint` plus a `tasks list` for status and commit plus an elaboration read, repeated for every milestone. That is exactly what `emit.rs:1-4` says the engine should not assemble by hand.
   *Refuted if* one existing verb already returns, per milestone, its outside needs together with their status and `commit`.

2. **The launch args.** `emit.rs:748-767` (`stranded`) counts a need as met once it is `done`. While the pick ran in store order, every milestone ahead of the one being run was finished, and Re-entry stamped any unstamped one before the pick (`SKILL.md:111`). Once milestones can run out of order, a need sitting in a milestone that is done but has not passed its gate would launch dependents on work that has not been gated, unless the emit also requires `commit` for needs outside the milestone.
   *Refuted if* every route into `launch-args` is guaranteed to run after the stamping settle, for every milestone and not only the one in front.

3. **The lane check, with a second session elaborating.** `lane.rs:88-131` hashes the whole store's `rows`, `specs`, `milestones` and `changes`. `lane.rs:290` recomputes that hash at check time. `lane.rs:360-383` lists every trail row inside the window. An audited post-approval elaboration in another session writes spec, row and `milestone:<id>` trail rows. `drain.md:135` then reads those as "a worker wrote it" and sends them to `needs_human`. So the digest and the trail have to leave out what another milestone under elaboration owns. Because the digest is recomputed at check time, the set to leave out must be fixed at launch. Recording it on the `Run` works. Deriving it from the current store does not, because the elaboration's final `--elaborated` flip would change the answer mid-run.
   *Refuted if* elaboration writes leave no trail row and no digest change, which they cannot do on an audited store (`milestone.rs:322-327`).

4. **The pre-flight's store checks.** `run-milestone.md:19-26` stops on any error from `plan check` or `tasks check`, and both run over the whole store. Only `accept-no-check` is relaxed for deferred milestones (`check.rs:440-451`, `520`). A half-written elaborated row raises `plan/task-no-spec` (`check.rs:428`), `spec-no-outcome` or `spec-no-accept` (`504-517`), `design-anchor-dead` and `premise-task-unknown`, and any of these refuses every milestone. The fix is to scope the exit code to the picked milestone plus its dependency closure. `Finding.ids` makes that possible for most classes. `plan/spec-orphan` carries no ids (`check.rs:467-471`), so the procedure has to write row and spec together or that class stays store-wide.
   *Refuted if* the elaboration procedure can always write a milestone's rows in an order that never leaves an error-class finding between two writes. That includes `tasks add-many` and spec updates landing in one call.

5. **The commits.** `drain.md:431` stages the uncommitted paths among the milestone's rows' `files`. A file shared by two rows with no dependency path between them is only a warning (`tasks/check.rs:639-654`). Under store-order picking, an unfinished started row could only belong to the milestone being run. Once milestones run out of order, a parked or failed started row in milestone A, holding a file it shares with an independent milestone B, gets its partial edits swept into B's milestone commit. The pick has to treat that overlap as not runnable, or the commit has to refuse it. Separately, `drain.md:463` stages all of `.claude/flows/<slug>/`, so another session's elaboration writes ride along in this milestone's flow-store commit. That is safe only while the milestone stays `deferred` until the final flip.
   *Refuted if* `tasks check` turns an overlap between rows of different milestones into an error.

Smaller forced edits:
- `stage.rs:90-101` turns every open run into a blocker and a `relaunch_or_abandon` hint. Re-entry (`SKILL.md:98`) stops the engine on another session's live run, so an `elaborate:<id>` run would refuse the whole flow unless runs are told apart by step kind.
- `fanout-check.sh:23-40` diffs the whole tree and `HEAD`. A cold read for an elaboration running beside `/implement-v2` would report the workers' edits and the milestone commit as cold-reader writes. The procedure either places the cold read when no `implement:` run is open, or the script takes extra excludes and tolerates `HEAD` moving.
- `apply-v2-engine/SKILL.md:109` asks the user on a premise that becomes false. `SKILL.md:167` and `apply-rows.md:215` rely on store adjacency for settle→apply. That is precisely an order "no task dependency records", so it is the first consumer of an explicit milestone `after`.

## Drafted structure

**A. One milestone-schedule module in tomlctl (new, e.g. `tomlctl/src/tasks/milestones.rs`).** It owns:
- The milestone DAG. Its edges are N→M wherever a row of M needs or couples a row of N, found through `Graph::closure_up` (`graph.rs:260`), plus explicit `after` edges.
- Each milestone's state: `done` (every row done and stamped), `unstamped`, `runnable`, `waiting{on}`, `deferred`, `elaborating` (an open `elaborate:<id>` run, supplied by the plan side), `parked`, and `blocked-by-overlap{row,file}`.
- The pick: the first `runnable` milestone in store order.

It sits above `graph.rs`, not inside it. `Node` (`graph.rs:31-38`) carries no `commit`, and `graph.rs:76-80` is deliberately the task-level seam. `groups()` (`graph.rs:364`) stays as it is.

**B. The schema gains `after: Vec<String>` on `MilestoneSpec`** (`schema.rs:369`), with its read and write and the snapshot export. It is set through `plan milestone add/update --after <id>` in `milestone.rs`'s patch path, with an audit row like the other fields. Independence the plan does not qualify stays unordered.

**C. Checks.**
- `tasks/check.rs` `cut_findings` keeps store order a valid topological order. Keeping it is what makes "first runnable in store order" deterministic and lets `--milestone` validation become "every milestone it depends on is done", with no special case. It is extended so an `after` edge that points forward in store order, or names an unknown id, is an error.
- `plan/check.rs` gains a warning for a deferred milestone sitting ahead of a milestone that does not depend on it. It sits beside `milestone_width_findings` (`check.rs:729`), which already walks store order and skips deferred milestones. This is the "warns" half of the last future-state bullet. `milestone add` can only append (`milestone.rs:143`), so placing a milestone needs a position flag, or plan-v2 has to add deferred milestones last.
- Both checks take a milestone scope for the exit code, as described in claim 4.

**D. A new `plan emit` leaf (e.g. `plan emit schedule`)** prints module A's states and the pick, overlaying open `elaborate:` runs from `plan.toml`. `run-milestone.md` §2, `SKILL.md`'s `--milestone` rule, `drain.md`'s Next rule and `apply-v2-engine` step 2 then read this leaf instead of walking store order. A deferred or elaborating milestone is reported, not stopped on.

**E. Lane scoping in `lane.rs`.** The milestones excluded from the digest and the trail are fixed at `plan run start` and stored on the `Run` (`records.rs:656-670`). `store_digest` and `trail_writes` drop records owned by those milestones. `launch-args` `stranded` requires `commit` for needs outside the milestone.

**F. Elaboration as a run step.** Elaboration becomes `elaborate:<id>` under `plan run start/finish`, which already records `session`, `run_id` and `started`. The milestone stays `deferred` through every write, the cold read and the probes (`plan probes` already scopes by `--milestone`, per `probes.rs:8`). It becomes runnable only at the final `plan milestone update --elaborated`. That ordering makes every existing `is_deferred()` reader (`check.rs:443/632/749`, `approve.rs:96`, `view.rs:404/466`) correct without a fourth `elaboration` value. `stage.rs` and Re-entry tell `elaborate:` runs apart from `implement:` and `reproduce:` runs.

What stays as it is: the Workflow and `launch-args`' membership logic beyond `stranded`, `graph.rs`, `tasks ready`, the probe verbs, one task store per flow, and store order as the tie-break.

## Alternatives ruled out

- **Keep the pick in prose and add calls.** It would need `tasks closure --checkpoint` plus `tasks list` plus an elaboration read per milestone, the overlap test by hand, and `after` edges read by hand. That is many calls per invocation, built from row reads, which `emit.rs:1-4` rules out.
- **A fourth `elaboration` value, `elaborating`.** It carries no session or start time, so there is no ownership. Every `is_deferred()` caller would have to classify it, and a crashed elaborator would leave the value stuck with no abandon path. A run row already has abandon semantics (`records.rs:717`).
- **Split the task store per milestone** so that concurrent writes never meet. The cut check, the ready frontier, the graph and the lane digest all read one store. That is far more churn than scoping the digest inside `lane.rs`.
- **Put the milestone DAG in `graph.rs`.** It would need `commit`, elaboration and run state on `Node`, which breaks that module's task-only seam.
- **Drop `checkpoint/invalid-cut`.** Without it, store order stops being a topological order. The pick then loses its deterministic tie-break and `--milestone` needs its own reachability walk.

Not decided here, for the planner: whether `drain.md:476` ("continue only while nothing in the flow is parked") should become "while a runnable milestone remains". The intent's list of holds does not name parking.

Coverage: read in full `implement-v2-engine/SKILL.md`, `references/run-milestone.md`, `tomlctl/src/plan/{milestone,stage,check}.rs`, `tomlctl/src/tasks/closure.rs` and `plan-v2-engine/scripts/fanout-check.sh`. Read in part `drain.md` (steps 3, 5, 6, 10 and Next), `apply-rows.md` (settle drain), `apply-v2-engine/SKILL.md` (steps 2 and 5, hand-over), `write-and-prove.md` (milestones and cold read), `tomlctl/src/plan/{emit,lane,records,approve,probes}.rs` (launch-args, digest, run start, deferred list, scoping header), `tomlctl/src/tasks/{graph,ready,check,schema}.rs` (Node, frontier, groups, cut and overlap findings, MilestoneSpec) and `tomlctl/src/plan/store.rs` (open runs, require_open). Not read: `plan-v2-engine/SKILL.md`, `approve.md`, `open-and-challenge.md`, `view.rs`, the `implement-v2-milestone` Workflow script. Nothing was built or run.
