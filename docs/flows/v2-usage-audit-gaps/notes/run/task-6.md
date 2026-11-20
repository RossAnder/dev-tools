Status: done — milestones store baseline gates, baseline proofs and the ui call on task-store schema 5, wired into plan milestone add and update

```toml run-note
files_touched = [
  "tomlctl/src/tasks/schema.rs",
  "tomlctl/src/tasks/milestones.rs",
  "tomlctl/src/plan/milestone.rs",
  "tomlctl/src/plan/lane.rs",
  "tomlctl/tests/plan_milestone.rs",
]

[[deviations]]
what = "`--baseline-proof` is wired into `plan milestone update` only, not `add`"
why = "The CLI args struct (tomlctl/src/cli/types/plan.rs, another task's file) declares `baseline_proof` on PlanMilestoneUpdateArgs only; a proof on add could not name a null-control task row in any case, since the milestone has no rows yet"
means = "detail: wire --baseline-gate and --baseline-proof into plan milestone add and update"

[[deviations]]
what = "Fixed the stale assertion in tasks::schema::tests::an_absent_run_by_takes_its_owner_from_writes_build_and_writes_no_key, which still expected a worker-run building check to be refused"
why = "An earlier task in this milestone lifted that refusal from Accept::owner_problem in the same file but left this unit test asserting it, so the milestone gate would have failed on it; the file is this task's, and the new assertion follows the recorded decision on building reproducers"
means = "none (outside the detail)"

[[deviations]]
what = "When `--gate` or `--clear-gate` replaces the gate list without `--baseline-gate`, baseline gates no longer in the gates are dropped, and proofs whose gate is no longer a baseline gate are dropped with them"
why = "Matches how external_gates marks are pruned on a gate replacement in patch_milestone; the alternative, refusing the gate update until the baseline list is fixed, would add a step no other milestone list requires"
means = "follow how external_gates and gate_probes were added"

[[tangential]]
kind = "bug"
area = "tomlctl/src/cli/types/plan.rs"
summary = "The help text for `--baseline-gate` says 'rerun at the milestone's base commit when it fails', and the help for `--baseline-proof` says 'the flags given replace the whole list', but the records design and this verb store a proof per gate by upsert and describe the baseline gate as one proved on the unchanged tree"
why = "`--help` and the skill flag reference generated from it would misdescribe the stored behaviour"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "cargo clippy --all-targets (isolated copy of tomlctl, sibling pattern gaps patched in the copy only)"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check (the five edited files)"
result = "pass"

[[checks]]
command = "grep -q 'baseline_gates' tomlctl/src/tasks/schema.rs && grep -q 'baseline_proofs' tomlctl/src/plan/milestone.rs"
result = "pass"

[[checks]]
command = "cargo nextest run -E 'test(/^plan_milestone::/) | test(/tasks::schema::/) | test(/plan::milestone::/) | test(/tasks::milestones::/) | test(/plan::lane::/) | test(/^tasks_/)' (isolated copy)"
result = "pass"
```

## What changed

- `tomlctl/src/tasks/schema.rs`: `SCHEMA_VERSION` is now 5, with `V4_SCHEMA_VERSION = 4` for the external-effect rung. `MilestoneSpec` gains `baseline_gates`, `baseline_proofs` (a new `BaselineProof { gate, verdict, at, task }`) and `ui: Option<bool>`, plus the `BASELINE_PASS`, `BASELINE_FAIL` and `BASELINE_NULL_CONTROL` verdict constants. `holds_v5_keys` stamps 5 when any milestone holds one of the three; otherwise the stamp ladder is as before. The reader is strict on a non-boolean `ui`, a non-table proof and a non-integer proof `task`. The writer omits all three keys while they are empty or absent, so a milestone without them keeps its bytes and stamp. `at` is a TOML datetime. Unit tests: round-trip and stamp 5 for each new key, and refusal of a malformed `ui` or proof. The two version-4 tests now set `V4_SCHEMA_VERSION`. The building-worker assertion is updated as described above.
- `tomlctl/src/plan/milestone.rs`:
  - `--baseline-gate` on add and update must name one of the milestone's gates (external gates included), or the call is refused with the command named. On update the list replaces the whole baseline list.
  - `--baseline-proof '<gate> :: <verdict>'` on update needs a baseline gate and a verdict of `pass` or `fail`. `<gate> :: null-control :: <task id>` stores `task`, refused unless the id names a row of this milestone whose spec holds at least one accept entry. A third part after `pass` or `fail` is refused.
  - Proofs upsert by gate and stamp `at` with the current time.
  - `--ui yes|no` on add and update stores the call.
  - Every new field is audited on an audited store.
- `tomlctl/src/tasks/milestones.rs`, `tomlctl/src/plan/lane.rs`: the new fields are initialised empty at each test-module `MilestoneSpec` literal. Nothing else changed.
- `tomlctl/tests/plan_milestone.rs`: five new tests over a new fixture holding a milestone with two gates, a row with an accept entry, a row without one and a row outside the milestone:
  - add and update with a baseline gate and the ui call, plus a bad `--ui`.
  - A baseline gate naming no gate is refused on add and on update, writing nothing.
  - A proof upserts by gate, and a null-control proof stores its task.
  - Each bad proof is refused, writing nothing: a bad verdict, a missing separator, a third part on pass, null-control without a task, a non-numeric task, a task with no accept entry, a task outside the milestone, and a gate that is not a baseline gate.
  - A store without a baseline gate keeps its stamp of 4. Adding one moves it to 5, and dropping it restores the original bytes.

## Checks

The in-tree clippy failed only on sibling files: `plan/approve.rs`, `plan/emit.rs` and `plan/run_gate.rs` do not yet bind the CLI fields another task added, and `plan/records.rs` has a test calling `upsert_handover` with the old arity. To compile my changes, I copied the crate to the session scratchpad with its own target dir, patched those sibling gaps in the copy only, and ran clippy and the narrow tests there. clippy was clean for my files, and 314 tests passed. The copy was then deleted. The `plan_milestone::` nextest accept check belongs to the gate.

Glimpse embeds tomlctl's task-store reader and refuses schema 5. The plan's existing H1 relink handover already covers this, so this note proposes no new handover.
