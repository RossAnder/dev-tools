Verdict: a worker could carry out milestone A as written; one point needs settling (the dispatch-tests subset that six tasks must run fails between tasks 6 and 13), there are two minor false premises (tasks 13 and 16), and two accept checks already pass at base.

```toml sources
read_at = "5aca3e7be8f169ba2b524edbb4e87b955e5b67da"
dirty = ["docs/flows/milestones-by-dependency/design/schedule.md"]

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 454
claim = "documented_plan_classes_match_the_source compares the plan classes in src/plan with plan.md's table, so it fails once task 6 adds classes and until task 13 documents them"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 329
claim = "the plan check section has a command block and a prose output shape, but no flag table"

[[source]]
path = "claude/skills/tomlctl/references/plan.md"
line = 542
claim = "the plan/milestone-width row describes the target as the milestone after the latest need, which task 6 changes and no task updates"

[[source]]
path = "claude/skills/implement-v2-engine/references/drain.md"
line = 61
claim = "the lane check is run in step 3 (Drift); step 5 at line 131 reads its store_writes"

[[source]]
path = "tomlctl/tests/output_options.rs"
line = 988
claim = "\"schedule\" already appears in output_options.rs as the review schedule case"

[[source]]
path = "claude/skills/plan-v2-engine/scripts/fanout-check.sh"
line = 40
claim = "--exclude-standard already matches the grep for --exclude"

[[source]]
path = "tomlctl/src/plan/milestone.rs"
line = 147
claim = "MilestoneSpec is built in add (147) and patch_milestone (270), as task 1 says"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 369
claim = "MilestoneSpec ends with gate_probes; milestone_to_toml writes gate_probes only when it is not empty (1366)"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 94
claim = "milestone_width_findings runs only before approval; findings() is shared with plan stage"

[[source]]
path = "tomlctl/src/plan/check.rs"
line = 428
claim = "plan/task-no-spec fires for rows of a deferred milestone as well, so task 7's test fixture is sound"

[[source]]
path = "tomlctl/src/plan/emit.rs"
line = 748
claim = "stranded accepts a done need without reading commit"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 88
claim = "store_digest projects and hashes sections; trail_writes at line 360 filters by the run window"

[[source]]
path = "tomlctl/src/plan/stage.rs"
line = 90
claim = "with_open_runs splices every open run into blockers and points next at the oldest"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 261
claim = "approval sets audit = true, so after approval an --elaborated flip always leaves the trail row that task 10's exclusion relies on"

[[source]]
path = "docs/flows/milestones-by-dependency/design/schedule.md"
claim = "the states, pick and output that tasks 3, 4 and 5 follow are fully specified"
```

## Unactionable points

1. **Tasks 14, 16, 17, 18, 19 and 20: the required dispatch-tests subset will fail for a reason that is not theirs.** Each task says "run the cli::dispatch::tests subset after editing". Task 6 adds three `plan/*` classes to `tomlctl/src/plan/check.rs` and says itself that `documented_plan_classes_match_the_source` fails until a later task documents them. That later task is 13. None of tasks 14, 16, 17, 18, 19 or 20 needs 13, so any of them can run while 6 is done and 13 is not. Task 17 needs only 2, so it can even land before 6. In that window the subset fails on `tomlctl/src/cli/dispatch/tests/finding_classes.rs:454`, naming `src/plan` and `plan.md`, which these workers do not own. The plan does not say whether the worker should treat that one failure as expected and pass, or park. CLAUDE.md's rule to note a failure in a file you do not own and return covers it only loosely. Either make those tasks need 13, or name the expected failure in their constraints as task 6 does.

## False premises

1. **Task 13 says "a `--milestone <ID>` row in the `plan check` table".** At `claude/skills/tomlctl/references/plan.md:329-341`, the `plan check` section has a command block and a prose description of the output, but no flag table. The worker has to create the table, not add a row to one. It can still act.
2. **Task 16 says "drain.md, step 5 (lane check)".** The lane check is run in step 3, Drift, under "### The lane check" (`drain.md:61-69`). Step 5, "Store changes during the run" (`drain.md:131`), reads the check's `store_writes` and `digest`. Either place could take the sentence. The worker has to choose, but it is not blocked.

## Gaps (not counted)

- Task 6 changes the `plan/milestone-width` target so it skips deferred milestones. The documented row at `plan.md:542` still describes the target as "the one after its latest need, or the first when it has none". Task 13 updates no part of that row, so the row goes stale. `documented_plan_classes_match_the_source` checks only class names and severities, so no gate catches this.
- Task 1 adds `after` to the snapshot's milestone object. The snapshot field list at `claude/skills/tomlctl/references/tasks-store.md:256` is not named in task 13, which covers only the `[[milestones]]` key.

## Check commands

I ran every worker-owned accept check against the present tree (HEAD 5aca3e7b). The forward checks should fail (exit 1) before the change.

- T1 `grep -qF 'pub(crate) after: Vec<String>' …schema.rs` exited 1; `grep -qF '"after": ' …snapshot.rs` exited 1.
- T2 `long = "after"` and `long = "clear-after"` both exited 1.
- T3 module exists and is registered: exited 1. T4 both checks exited 1. T5 `"schedule"` in plan_emit.rs exited 1.
- **T5 `grep -qF '"schedule"' tomlctl/tests/output_options.rs` exited 0 at base, so it cannot tell whether the change was made.** It matches the existing `review schedule` case at `output_options.rs:988`. Grep for `"emit",` next to `"schedule"`, or for `plan emit schedule`, instead.
- T6, T7, T8, T9 and T10 checks all exited 1.
- **T11 `grep -q -- '--exclude' …fanout-check.sh` exited 0 at base, so it cannot tell whether the change was made.** It matches `--exclude-standard` at `fanout-check.sh:40`. Use `grep -qF -- '--exclude)'`, `excludes.txt` or similar. The gate's `bash …/fanout-check.test.sh` does discriminate, since the file does not exist yet.
- T12, T13, T14 (all three), T15 (both), T16 a1 and a2, T17, T18, T19 (both), T20 and T21 all exited 1, as expected.
- T16 guard `test $(wc -l < drain.md) -lt 600` exited 0. drain.md has 586 lines, so task 16 has 13 lines of headroom.
- I spot-checked the premise probes and they hold: `gate_probes` in schema.rs and snapshot.rs, `clear-gate`, `dependency_closure`, `PlanEmitOp::GateList(args) => gate_list(args)`, `fn open_runs`, `"lane-check",`, `if plan.approvals.is_empty()`, `let mut found = check(&tasks, &[]);`, the `stranded` line, `fn with_open_runs`, the lane.rs sections tuple, `specs+=(":(exclude)$note")`, `"plan_emit_lane_check",`, and the run-milestone, implement-v2 SKILL, drain, apply-v2, open-and-challenge, write-and-prove and CLAUDE.md sentences.
- I did not run the gate-owned `cargo nextest` checks. They are whole-suite runs and belong to the orchestrator.

## Coverage

I read the worker view and the `files` of all 21 tasks. Code checked against the tasks:

- `tomlctl/src/tasks/schema.rs`, `snapshot.rs`, `closure.rs`, `graph.rs` and `mod.rs`
- `tomlctl/src/plan/milestone.rs`, `check.rs`, `emit.rs`, `lane.rs`, `stage.rs`, `store.rs`, `records.rs` (run_start) and `mod.rs`
- `tomlctl/src/cli/types/plan.rs`
- `tomlctl/tests/plan_emit.rs` (helpers) and `output_options.rs`
- `tomlctl/src/cli/dispatch/tests/finding_classes.rs`
- `fanout-check.sh` and `plan-v2-prove.js` (fan-out ownership)
- `run-milestone.md`, `drain.md` and `apply-rows.md`
- the apply-v2 SKILL, the plan-v2 SKILL, `open-and-challenge.md` and `write-and-prove.md`
- `plan.md`, `plan-engine.md` and `tasks-store.md`
- `claude/commands/implement-v2.md` and `plan-v2.md`
- `CLAUDE.md`
- `design/schedule.md`

Tasks 3 to 5 follow design/schedule.md, which is internally consistent and settles every state and the pick. I did not read the precedent note `notes/elaboration-procedure.md` (task 18), to keep this read cold. I did not compile or run any tests.
