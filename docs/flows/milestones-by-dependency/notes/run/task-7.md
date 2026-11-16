Status: done — plan check takes --milestone and keeps only findings on that milestone, its upstream milestones, no task, or an absent task

```toml run-note
files_touched = ["tomlctl/src/cli/types/plan.rs", "tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs"]

[[deviations]]
what = "When tasks::milestones::upstream refuses the store (a cycle, a dangling edge or an after naming no milestone), --milestone keeps every finding instead of refusing or scoping"
why = "The detail does not say what to do when the milestone graph cannot be formed; refusing would hide the dag/* and plan/milestone-after-unknown findings that explain why, while keeping everything leaves the scoped check at least as strict as the flow-wide one"
means = "detail: compute the row set from crate::tasks::milestones::upstream"

[[deviations]]
what = "The --milestone id is checked against [[checkpoints]] only, by loading the task store a second time in run, before findings() runs"
why = "The detail says refuse an id naming no checkpoint and leave findings() unchanged; plan emit and plan probes also accept a [[milestones]]-only id, but a milestone with no checkpoint holds no rows to scope to"
means = "detail: refuse an id naming no checkpoint"

[[tangential]]
kind = "debt"
area = "claude/skills/tomlctl/references/plan.md"
summary = "The plan check flag table does not list --milestone yet"
why = "That file belongs to a sibling task in this run; whichever task documents plan check should add the flag so the reference and the help text agree"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/check.rs tomlctl/src/cli/types/plan.rs tomlctl/tests/plan_check.rs"
result = "pass (after applying rustfmt to check.rs)"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_check::/)'"
result = "pass (59 tests)"

[[checks]]
command = "grep -qF 'Report only findings on this milestone' tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "grep -qF 'milestones::upstream(' tomlctl/src/plan/check.rs"
result = "pass"
```

## What changed

- `tomlctl/src/cli/types/plan.rs`: `PlanCheckArgs` gains `--milestone <ID>`, with the help text the task's detail gives.
- `tomlctl/src/plan/check.rs`: `run` now builds a `MilestoneScope` when `--milestone` is given. It refuses an id that names no checkpoint (`kind=validation`). It then collects the rows of the milestone and of every milestone upstream of it, at any distance, by following `milestones::upstream` transitively, so `after` chains count too. A finding is kept when its ids are empty, when one of them is a scoped row or an id no row carries, or when its class is `dag/*`, `checkpoint/invalid-cut`, `plan/handover-incomplete` or `plan/handover-task-unknown`. `ok` and the exit status come from the kept findings only. `findings()` is unchanged, so `plan stage` and `plan approve` see the full set. Without `--milestone`, plan check behaves exactly as before.
- `tomlctl/tests/plan_check.rs`: added a `run_with` helper and four tests:
  - a deferred milestone's spec-less row fails the flow-wide check but not `--milestone` of A or B, and it does fail `--milestone B` once B needs that row;
  - a contract naming a removed task fails the scope of an independent milestone;
  - an incomplete handover sourced in a done milestone A fails `--milestone B`;
  - an unknown id is refused.

The nextest accept check is the gate's to run and was not run here. Only the `plan_check::` tests were run.
