Verdict: Out-of-order is safe only while no milestone other than the picked one holds a started, unstamped row (`agent` set, `commit` empty). Today that is guaranteed by store order alone, and every flow-wide exemption relies on it: the clean-tree check (run-milestone.md:37-45), the lane set (tomlctl/src/plan/lane.rs:328) and the gate, which runs over the whole tree (drain.md:295-327). The pick must refuse that case, and must refuse a fix milestone `apply-<n>` while `settle-<n>` is unfinished, because only store position links the two (apply-rows.md:215).

## Answer

Today store order keeps one invariant: when a milestone is picked, every earlier milestone is done and stamped, and no later one has been started. Re-entry stamps or re-gates unstamped milestones before the pick (SKILL.md:111), and the pick refuses to go past an unfinished checkpoint (run-milestone.md:58). So the only started, unstamped rows in the flow belong to the milestone being run. Several mechanisms are written flow-wide and stay correct only because of that invariant. A dependency-ordered pick breaks it as soon as it passes over a milestone X whose rows were started.

What breaks, per mechanism, when Y runs ahead of an earlier-stored X:

| X's state | Clean-tree check | Gate | Milestone commit | Flow-store commit | Stamp | Lane check | Apply rows | Handovers |
|---|---|---|---|---|---|---|---|---|
| Started, uncommitted edits (parked or failed rows, gate stopped after round 2, two timeouts, stop rule) | Passes silently: X's started rows are exempt flow-wide | Runs crate-wide over X's half-edits. It fails on them or passes because of them, a fix round can mint a Y task on X's files, and the held `cargo install` installs X's unfinished code | Stages Y's `files` that are uncommitted. A file Y shares with X takes X's partial edits with it, and a Y fix task on X's files takes them all. Y's commit is not the tree the gate passed | Takes X's run notes and row states before X's code lands (harmless) | No new break | X's started files count as Y's lanes, so a Y worker editing them is never flagged | The reproduce check reads a mixed tree | No new break |
| Done but unstamped (gated or not) | As above (the rows are started) | As above | As above. Also, launch-args counts a `done` need as met without its commit (emit.rs:754-760), so Y can build on X's uncommitted code and Y's commit then lacks it | As above | Re-entry's stamp test is HEAD-only (drain.md:472). A Y commit landing after X's commit and before X's stamp sends X back to a gate rerun and a commit with nothing to stage. Re-entry's settle-before-pick order prevents this, as long as it is kept | As above | As above | No new break |
| Deferred or being elaborated (no rows started) | No break: unstarted rows are not exempt, and elaboration writes only to the flow directories, which are never listed | No break | No break | Takes X's half-elaborated `tasks.toml` and `plan.toml` rows. Those rows cite `docs/flows/<slug>/design/`, which this commit does not stage (drain.md:463). This is the concurrent-elaboration lens's question | No break | The digest covers every section of the store (lane.rs:88-123), so an elaboration write during Y's run reads as a worker write. Only a concurrent session can cause that, so it belongs to the concurrent-elaboration lens | Settle and fix pairing breaks (below) | No break |

What the pick must refuse:

1. **Any milestone other than Y with a started, unstamped row.** This rule alone restores the invariant above. It also covers "done but unstamped", since a worker-done row carries `agent`. Little is lost: every way X ends up started and unstamped is a state in which the flow stops today. The measured case, a deferred or elaborating X, has no started rows and stays runnable.
2. **A row of Y with a need outside Y that is not both `done` and stamped.** Rule 1 already covers this. Check it in `plan emit launch-args` too: `stranded()` accepts a bare `done` need today.
3. **`apply-<n>` while `settle-<n>` holds a row not done and stamped.** No `needs` edge links them: settle rows are minted with `needs = []` (schedule.rs:646), and the settle drain schedules accepted findings into "the checkpoint that follows this one in store order". If `apply-<n>` ran first, the drain would add pending rows to a committed milestone. Its re-run would then re-stamp every row with the new sha (drain.md:445-450), and `tasks update --commit` overwrites without complaint. The pairing could also be written as an edge from fix rows to settle rows instead of a pick refusal.
4. **A milestone that is deferred, or whose elaboration is unproved, picked as Y itself.** This is outside this lens: the elaboration-procedure and concurrent-elaboration lenses own it.

Handovers need no refusal. Nothing between milestones waits on an undone handover, in store order or out of it: the Next rule checks only the commit and parked rows (drain.md:476). Under rule 1, a milestone holding a proposed but uncommitted handover blocks the pick anyway.

## Findings

- **File**: claude/skills/implement-v2-engine/references/run-milestone.md:37-45 and tomlctl/src/plan/lane.rs:320-331 `out_of_lane`
- **Source**: run-milestone.md:37 (a row is started when `agent` is a worker, "whatever its `status`") and :41-43 (the files of every started row are exempt); lane.rs:328 `run.tasks.contains(&row.id) || (WORKER_AGENTS.contains(..) && row.commit.trim().is_empty())`
- **Evidence-grade**: high
- **Finding**: Both the clean-tree exemption and the lane set cover every started, unstamped row in the flow, not just the picked milestone's. Under an out-of-order pick, another milestone's unfinished edits pass both checks unseen. Refuse that state at the pick (rule 1), or scope both to the picked milestone. needs-plan
- **Details**: `plan drift` counts the same rows as the run's own work (drain.md:57). Under store order these sets equal the picked milestone's started rows, because re-entry settles unstamped milestones before the pick (SKILL.md:111) and `--milestone` refuses to pass an unfinished checkpoint (SKILL.md:38). Files: run-milestone.md, SKILL.md, lane.rs, and the pick verb the pick-and-preflight lens names.
- **Counter**: A store-order invariant that already keeps other milestones' started rows out of the tree under any pick. None exists: the invariant comes only from the refusal at SKILL.md:38.
- **Impact on plan / report**: The dependency-ordered pick must carry rule 1. A milestone-scoped clean-tree check without rule 1 would instead report X's dirt as foreign and stop the engine.

- **File**: claude/skills/implement-v2-engine/references/drain.md:295-327 (gate), :373 (fix-task `files`), :431-439 (milestone commit)
- **Source**: drain.md:373 "`files` are the files the failure points at, whichever task last wrote them"; :327 holds `cargo install` back only until the other blocks pass; every milestone gate in `v2-harness-tuning` and `v2-apply` is crate-wide (`cargo nextest run --manifest-path tomlctl/Cargo.toml`, `cargo install --path tomlctl`), read with `tomlctl items list .claude/flows/<slug>/tasks.toml --array milestones --select id,gates`
- **Evidence-grade**: high
- **Finding**: The gate checks the working tree, not the commit. With another milestone's uncommitted edits present, Y's gate tests a mixed tree. Its fix round can mint a Y task on X's files, so Y's path-staged commit sweeps X's partial work in, and the held `cargo install` installs a binary built from X's unfinished code.
- **Details**: The gates in this repo's corpus build whole crates, so disjoint `files` across milestones do not isolate one milestone's gate from another's edits. The milestone commit stages Y's `files` that are uncommitted (drain.md:431), so it is not the tree the gate passed whenever X's edits stay out of it. Rule 1 at the pick removes the case. Nothing in the store describes a gate's scope, so the scope cannot be compared against X's files instead.
- **Counter**: A gate that reads only the files it stages, such as one run over a `git stash --keep-index` snapshot. The engine has none, and stashing is barred to it.
- **Impact on plan / report**: Use rule 1 rather than a file-overlap test. File overlap is the weaker guard, and cross-milestone sharing without a dependency path is only a `dag/unreachable-claim` warning (tomlctl/src/tasks/check.rs:639-660).

- **File**: tomlctl/src/plan/emit.rs:748-767 `stranded`
- **Source**: emit.rs:756 `if status == Some(Status::Done) || candidates.contains_key(&need) { continue; }`
- **Evidence-grade**: high
- **Finding**: Launch-args treats a need in another milestone as met once it is `done`, even if it is unstamped. A dependency-ordered pick needs "done and committed", or Y builds on code that its own commit leaves out.
- **Details**: Under store order a `done` need in an earlier milestone is always stamped by the time it is read (SKILL.md:111), so the `commit` test was never needed. Add a `commit` test for needs outside `--milestone`, with a new dropped reason or with `need-not-done` extended. Files: emit.rs, run-milestone.md (dropped table, :171), and tomlctl/tests/plan_emit.rs.
- **Counter**: A pick-side rule that guarantees no out-of-milestone `done` need is unstamped. Rule 1 does guarantee it, so this edit is defence in depth rather than the guard itself.
- **Impact on plan / report**: Optional once rule 1 is in place. It is cheap and keeps the verb correct when used on its own.

- **File**: claude/skills/implement-v2-engine/references/apply-rows.md:215 and claude/skills/apply-v2-engine/SKILL.md:167,174
- **Source**: apply-rows.md:215 "The fix milestone is the checkpoint that follows this one in store order"; tomlctl/src/review/schedule.rs:646 settle row `needs = []`; schedule.rs:773 fix rows take only their own findings' `needs`
- **Evidence-grade**: high
- **Finding**: `settle-<n>` comes before `apply-<n>` only by store position. A dependency-ordered pick would run a non-empty `apply-<n>` first, then the settle drain would schedule new rows into a committed milestone, whose re-run re-stamps every row (drain.md:445-450).
- **Details**: `review schedule --checkpoint` makes no check on whether the target is already committed (schedule.rs: no `commit` test on the checkpoint). `tasks update --commit` overwrites a stamp without complaint (tomlctl/src/tasks/update.rs:1236 `replace_str`). Fix: either refuse `apply-<n>` while `settle-<n>` is unfinished, or have `review schedule --settle-checkpoint` add each fix row's need on the settle rows. Files: apply-rows.md, apply-v2-engine SKILL.md, and schedule.rs if the edge is chosen.
- **Counter**: `apply-<n>` being always empty until its settle drain runs. That holds only when every selected finding is blocked on evidence (apply-v2-engine SKILL.md:174). Mixed selections put unblocked fix rows into `apply-<n>` straight away.
- **Impact on plan / report**: Rule 3 belongs in the pick, or in a new edge minted by `review schedule`.

- **File**: claude/skills/implement-v2-engine/references/drain.md:472 (Stamping on re-entry)
- **Source**: drain.md:472 stamps only when `git show --name-only HEAD` lists the rows' files
- **Evidence-grade**: medium
- **Finding**: The re-entry stamp test is HEAD-only. Any commit landing after a milestone's commit and before its stamp, including Y's under an out-of-order pick, sends the milestone back to a gate rerun and to step 10 with nothing to stage.
- **Details**: SKILL.md:111 settles unstamped milestones before the pick, so the out-of-order case arises only if a new pick bypasses that ordering. Keep it. A user commit can trigger the same flaw under store order. `package-design-tools` in the reportdesignkit repo shows the pattern: B's rows sat done and uncommitted from its last run on 2026-11-12, a user "deps bump" commit (c8098327) landed, and B's commit f9662889 followed on 2026-11-13. B's commit had not been made yet, so the gate-rerun path was the right one there.
- **Counter**: A re-entry that finds the milestone's commit by searching history for the rows' files rather than reading HEAD. The current text does not.
- **Impact on plan / report**: Keep re-entry's settle-unstamped-before-pick order in the new pick. The HEAD-only test is a pre-existing fragility and is noted only.

Coverage: read in full claude/skills/implement-v2-engine/SKILL.md and references/run-milestone.md; read drain.md steps 1-7, 10 and the Next rule (lines 1-484); read apply-rows.md "Settle drain", "Failure on an apply row" and "Reconcile"; read lane.rs `store_digest`, `lane_check` and `out_of_lane`; read emit.rs `load_milestone_store`, `launch_args`, `stranded` and `drop_dependents`; read check.rs `cut_findings` and `overlap_findings`; read schedule.rs settle and fix row minting; the docs/flows/milestones-by-dependency brief. Checks run: grep of apply-v2-engine for settle and fix ordering, of plan stage and the plan.md reference for handover gating (none between milestones), of update.rs for commit overwrite; `tomlctl` reads of the gates and row files of `v2-harness-tuning` and `v2-apply`, and of the rows, checkpoints and runs of `package-design-tools`, plus `git log` there. Ruled out: handovers (nothing waits on them between milestones); `checkpoint/invalid-cut` (a valid store order stays a topological order whatever is picked); the gate-round count and `plan run gate` (keyed by milestone id); review-store reconcile (`in_flight` for another milestone's row is documented as expected, apply-rows.md:268). Left to sibling lenses: pre-flight scoping, the parked-anywhere Next rule, and the `--milestone` refusal (pick-and-preflight-sites); digest and flow-store-commit behaviour under a concurrent elaborating session (concurrent-elaboration).

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = []

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 37
claim = "Started rows are exempt from the clean-tree check flow-wide, whatever their milestone"

[[source]]
path = "claude/skills/implement-v2-engine/references/run-milestone.md"
line = 58
claim = "The pick takes the first checkpoint in store order with a row not done"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 111
claim = "Unstamped milestones are settled before the pick"

[[source]]
path = "claude/skills/implement-v2-engine/SKILL.md"
line = 38
claim = "--milestone is refused past an unfinished earlier checkpoint"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 328
claim = "The lane set includes every started unstamped row in the flow"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 88
claim = "The store digest covers every section of the task store"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 373
claim = "Gate fix tasks take the files the failure points at, whichever task wrote them"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 327
claim = "cargo install is held only until the other gate blocks pass"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 431
claim = "The milestone commit stages the milestone's row files that are uncommitted"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 445
claim = "The stamp writes the sha on every row of the milestone"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 463
claim = "The flow-store commit stages .claude/flows/<slug>/ and notes/run only"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 472
claim = "The re-entry stamp test reads HEAD only"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 476
claim = "The Next rule checks only the commit and parked rows, not handovers"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 756
claim = "launch-args treats a done need as met without checking its commit"

[[source]]
path = "tomlctl/src/tasks/check.rs"
line = 639
claim = "Files shared across tasks with no dependency path are only a warning"

[[source]]
path = "tomlctl/src/tasks/check.rs"
line = 665
claim = "invalid-cut requires store order to be prefix-closed over needs"

[[source]]
path = "claude/skills/implement-v2-engine/references/apply-rows.md"
line = 215
claim = "The fix milestone is the checkpoint after the settle milestone in store order"

[[source]]
path = "tomlctl/src/review/schedule.rs"
line = 646
claim = "Settle rows are minted with no needs, so no edge links fix rows to them"

[[source]]
path = "claude/skills/apply-v2-engine/SKILL.md"
line = 174
claim = "apply-<n> is empty until the settle drain only when every selected finding is blocked"

[[source]]
path = "tomlctl/src/tasks/update.rs"
line = 1236
claim = "tasks update --commit overwrites an existing stamp"

[[source]]
path = ".claude/flows/v2-harness-tuning/tasks.toml"
claim = "Every milestone gate in the corpus flow is crate-wide, including cargo install"
