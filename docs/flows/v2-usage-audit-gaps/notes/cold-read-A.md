Verdict: Milestone A can mostly be carried out as written. Three points stop it: task 1 and task 6 cannot compile inside their own file sets, and task 7's narrowing rule cannot be computed from the digest that task 3 stores. No false premises.

```toml sources
read_at = "6b6c579c7af3863440ee59552c3f91cb9481b3cb"
dirty = []

[[source]]
path = "tomlctl/src/plan/schema.rs"
line = 344
claim = "Approval, Handover, Attempt, GateRound and GateCommand are plain structs with no Default derive (lines 344-512), so a new field must be added at every literal site"

[[source]]
path = "tomlctl/src/plan/approve.rs"
line = 281
claim = "Approval struct literal outside task 1's files"

[[source]]
path = "tomlctl/src/plan/records.rs"
line = 597
claim = "Handover struct literal outside task 1's files; HANDOVER_SET_KEYS at line 60 and upsert_handover at line 550 exist as task 8 says"

[[source]]
path = "tomlctl/src/plan/run_gate.rs"
line = 176
claim = "GateRound literal at 176 and GateCommand literals at 273 and 355 (test module) sit outside task 1's files"

[[source]]
path = "tomlctl/src/plan/run_notes.rs"
line = 108
claim = "Attempt struct literal outside task 1's files"

[[source]]
path = "tomlctl/src/plan/lane.rs"
line = 745
claim = "a MilestoneSpec literal in lane.rs's test module, which no task in milestone A owns"

[[source]]
path = "tomlctl/src/tasks/schema.rs"
line = 430
claim = "MilestoneSpec has no Default derive; the run_by worker plus writes_build refusal that task 5 removes is at line 377; task store SCHEMA_VERSION is 4"

[[source]]
path = "tomlctl/src/cli/types/plan.rs"
line = 437
claim = "the gate_rows ArgGroup (ndjson, blocks) is required; PlanRunVisualArgs at 503 is the --accept/--json precedent; none of the new long flags exist yet"

[[source]]
path = "tomlctl/src/plan/dispatch.rs"
line = 41
claim = "verbs receive their whole clap args struct, so new flags need no dispatch edit; plan gate is approve::gate"

[[source]]
path = "tomlctl/tests/capabilities.rs"
line = 1922
claim = "the capabilities test also pins version 0.26.0, which task 15 does not name"

[[source]]
path = "tomlctl/src/ui_capture.rs"
line = 15
claim = "load_paths returns an empty set both when the config is absent and when it has no paths, so it cannot tell task 13 whether the file exists"

[[source]]
path = "tomlctl/src/cli/dispatch/tests/finding_classes.rs"
line = 23
claim = "plan.md's class table must match the classes src/plan emits, so task 13's classes need task 14 before the dispatch tests pass"

[[source]]
path = "tomlctl/tests/common/mod.rs"
line = 290
claim = "the test sandbox is a tempdir that is not a git repository"

[[source]]
path = "tomlctl/src/plan/store.rs"
line = 118
claim = "PlanReviewState is the review view shared by plan gate, approve and stage. It is outside task 12's files"
```

## Unactionable

### 1. Task 1: the new fields cannot compile inside the task's files

The Detail adds fields to existing structs: `base_checked` and `base_failed_ids` on GateCommand, `tree` on GateRound, `blocks`, `ready_check` and `note` on Handover, `sources` on Attempt, and `delta_waiver` on Approval. These structs (tomlctl/src/plan/schema.rs:344-512) derive no `Default`. Every struct literal therefore has to be updated:
- approve.rs:281 (Approval)
- records.rs:597 (Handover)
- run_gate.rs:176 (GateRound)
- run_gate.rs:273 and 355 (GateCommand; 355 is in the test module)
- run_notes.rs:108 (Attempt)

None of these files is in task 1's set, and the task says "Touch no file outside this task's files."

The plan does not decide between three options:
- widen task 1's files to cover those sites;
- accept that the crate stays uncompilable until tasks 3, 8, 11 and 12 land;
- use some other shape.

The second option has a real cost. While the crate is broken, every parallel sibling's `cargo clippy` fails on files those workers do not own. Task 2, for example, needs nothing and runs alongside task 1. Task 1's worker also cannot confirm that its own edit compiles. "Follow how VisualAccept was added" is no guide here, because VisualAccept was a new struct with a new vector, not a set of new fields on existing structs.

### 2. Task 6: a MilestoneSpec literal outside the task's files

Adding `baseline_gates` and `baseline_proofs` to MilestoneSpec (tasks/schema.rs:430) also breaks the literal at tomlctl/src/plan/lane.rs:745, inside lane.rs's `mod tests` (line 527). That file is not in task 6's files, and no task in milestone A owns it. Task 12 names lane.rs only as a precedent.

Without that edit, the lib test target does not compile. Task 6's own accept (`plan_milestone::`) and the milestone gate's `cargo nextest run` would then fail. The worker has to break the constraint or leave the gate red, and the plan does not say which.

### 3. Task 7: the narrowing rule cannot be computed from the stored digest

Task 3 stores only the whitespace-stripped digest of each changed file (records.md#tree-digest-on-a-gate-round). Task 7 then says to narrow when every tree entry still matches "(or every differing file's change is whitespace only and none is whitespace-significant)". There are two problems with this:
- **The whitespace-only case cannot happen.** With a whitespace-stripped digest, a file whose change is whitespace only never differs. A file that differs has changed outside its whitespace.
- **Whitespace-significant files cannot be detected.** A whitespace change to a `.py`, `.yaml`, `.yml`, `Makefile` or `.md` file in the tree leaves its stripped digest unchanged. It looks exactly like an untouched file, so a round that changes indentation in a `.py` file would be narrowed, which is the opposite of what records.md#emit-outputs intends.

The rule also says nothing about a file the fix round changed that was not in the round's `tree`, because it was clean when the round was recorded. "Every tree entry still matches" would narrow past such a change.

The worker has to decide whether to:
- refuse narrowing whenever the tree holds a whitespace-significant file;
- compare against git or a raw digest;
- re-scan `git status` for files outside the tree.

None of that is settled by the task or by D12.

## Not counted (a worker can resolve these from the task and code)

- **Task 3.** Recording `tree` on a failing run runs `git status`, but the test sandbox (tests/common/mod.rs:290) and plan_run_records.rs's `flow()` fixture are not git repos. The constraint "a call without --base records exactly what it records today, plus tree" implies an empty tree rather than a refusal when git is unavailable. The task should still say so.
- **Task 12.** plan/store.rs:118 describes PlanReviewState as the one view that plan gate, approve and stage share "so that none of them can disagree". That file is outside task 12's files, so the delta has to be computed from review::store::plan_family through target.rs and threaded into stage.rs and approve.rs separately. It is workable, but it bypasses that convention.
- **Task 13.** ui_capture::load_paths (ui_capture.rs:15) returns an empty set both when the config is absent and when it lacks `paths`, so it cannot tell the check whether the file exists. The worker has to test `root.join(ui_capture::CONFIG)` for existence directly. Separately, tasks 13 and 14 keep the finding_classes parity test (cli/dispatch/tests/finding_classes.rs) red until plan.md is updated. Task 14 needs 13, so the gate sees the pair together.
- **Task 15.** tests/capabilities.rs:1922 also asserts `version == "0.26.0"`. The task bumps Cargo.toml but names only the feature list in that test. A worker editing that file will likely see it, but the Detail should name it.
- **Task 2.** Clap's ArgGroup has no "required unless present", so "gate_rows stays required except when --accept is given" means adding `accept` to the group, or making the group non-required. Either is within the task's file.

## False premises

None found. These premises were checked and hold at HEAD:
- `VISUAL_ACCEPT_SCHEMA_VERSION: i64 = 6` (schema.rs:39), with the compile-time ordering asserts at 54-57 and write_version at 591.
- The worker-build refusal at tasks/schema.rs:377.
- HANDOVER_SET_KEYS (records.rs:60) and upsert_handover (records.rs:550).
- bind_names (view.rs:644).
- drift.rs's changed_files.
- run_note.rs's dropped-check handling (run_note.rs:279).
- FEATURES (cli/types/mod.rs:40).
- The arg structs named in task 2 (cli/types/plan.rs:438, 534, 599, 780, 875).
- approve::gate's `risks` array (approve.rs:100).
- Every file each task lists exists.

## Check commands

All read-only greps were run on the present tree (HEAD 6b6c579c). Each forward check prints 0 matches, so each one fails now and can tell the missing change apart:

| Task | Check | Matches |
|---|---|---|
| 1 | `GATE_ACCEPT_SCHEMA_VERSION: i64 = 7` in schema.rs | 0 |
| 1 | `base_failed_ids`, `delta_waiver` in schema.rs | 0, 0 |
| 1 | `gate_accept` in plan-store.md | 0 |
| 2 | `long = "after-round"`, `"waive-review-delta"`, `"baseline-gate"`, `"baseline-proof"`, `"base"` in cli/types/plan.rs | 0 each |
| 3 | `base_checked`, `fn whitespace_digest` in run_gate.rs | 0, 0 |
| 4 | `extra-round` in run_gate.rs | 0 |
| 5 | `self.run_by == RUN_BY_WORKER && self.writes_build` in tasks/schema.rs | 1 match (line 377), so `! grep -q` fails now as intended; the premise holds |
| 6 | `baseline_gates` in tasks/schema.rs, `baseline_proofs` in milestone.rs | 0, 0 |
| 7 | `after_round`, `"builds"`, `handover:` in emit.rs | 0 each |
| 8 | `"ready_check"`, `"blocks"` in records.rs | 0, 0 |
| 9 | `affected_decisions` in drift.rs | 0 |
| 10 | `fn a_task_bind_reaches_a_row_on_the_bound_tasks_files` in tests/plan_view.rs | 0 |
| 11 | `"sources"` in run_note.rs | 0 |
| 12 | `plan_digest` in review/runs.rs, `delta_waiver` in plan/approve.rs | 0, 0 |
| 13 | the four class names in check.rs | 0 |
| 14 | `--baseline-gate`, `ready_check`, `affected_decisions`, `--waive-review-delta`, `plan/ui-capture-missing` in plan.md | 0 each |
| 15 | `"gate_accept"`, `"review_plan_digest"` in cli/types/mod.rs | 0, 0 |
| 15 | Cargo.toml version | still `0.26.0` |

Not run: every `cargo nextest` and `cargo test` accept (tasks 1, 3-15). Each needs a build of the shared target, and the plan already gives each one to the gate. The filters do pick out the intended tests: `test(/plan::schema::/)` does not match `schema_request`, and `test(/plan::run_note::/)` does not match `run_notes`.

## Coverage

- **Tasks read:** all 15 tasks in milestone A, through the worker view and `tasks show --select id,files`.
- **Files read** (in part, at the cited lines):
  - tomlctl/src/plan/{schema.rs, schema_request.rs, dispatch.rs, approve.rs, records.rs, run_gate.rs, run_notes.rs, run_note.rs, emit.rs, drift.rs, view.rs, lane.rs, store.rs, check.rs}
  - tomlctl/src/tasks/{schema.rs, milestones.rs}
  - tomlctl/src/review/{store.rs, runs.rs}
  - tomlctl/src/cli/types/plan.rs
  - tomlctl/src/ui_capture.rs
  - tomlctl/src/cli/dispatch/tests/finding_classes.rs
  - tomlctl/tests/{plan_run_records.rs, tasks_spec_add.rs, capabilities.rs, common/mod.rs}
- **Design:** the design sections each task cites were read only as embedded in the worker views. Every task cites design/direction.md#milestone-a, and no task is a UI task.
- **Not checked:**
  - Whether review::fields::validate_row would refuse a `plan_digest` key, if task 12 inserted it into the start row rather than the typed struct.
  - Every compile and test outcome, which is left to the gate.
