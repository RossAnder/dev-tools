Verdict: Not as written. 4 unactionable points (tasks 5, 7, 8, 9) and 1 false premise (task 11) would stop a worker or send them the wrong way. Tasks 1, 2, 3, 4, 6, 10, 12 and 13 can be carried out as written.

# Cold read: milestone A (tasks 1-13)

## Unactionable

### U1. Task 5: "the newest (flow, milestone)" has no defined order
- Sentence (design `stores.md#rechecks`, quoted in the task): "The current recheck of a finding is the row with the highest `round` for the newest `(flow, milestone)` it has rows under".
- What is missing: what makes a `(flow, milestone)` pair "newest". The row carries `at`, but it is a date, so two milestones rechecked on the same day tie. Array position doesn't work either, because a same-key write replaces a row in place and keeps its old position. Milestone ids could be compared, but nothing says they sort. `recheck-status` reads this rule ("0 only when the current one matches --expect"), and task 12's reproduce rounds depend on it too. The worker has to choose a tiebreak, and the plan never settles one.

### U2. Task 7: reconcile cannot see a removed task through `[[finding_links]]`
- Sentence (design `verbs.md#review-reconcile`): "`review reconcile --flow <slug>` reads the flow's `[[finding_links]]` and moves each `scheduled` finding". The task's accept check also requires that it "reopens on a removed row".
- Conflict: task 1 (design `stores.md#task-side-link`) says "Removing a task row removes its link entry". This matches today's code, where `drop_ref_entries` drops every entry keyed on the ref (`tomlctl/src/tasks/schema.rs:470-474`). Once a row is removed, no link names its findings, so a reconcile that reads the links finds nothing to reopen. The `scheduled -> open` transition has to start from the review store's own `scheduled` findings and their `apply_flow`/`apply_task` fields (task 2). The plan does not say this.
- The design also says "moves each scheduled finding in this store", but the verb's signature is only `--flow <slug>`. It names no store flag (`--slug`/`--scope`, as in `ReviewStore` at `tomlctl/src/cli/types/review/mod.rs`), and it doesn't say whether links whose `store` is a different path are skipped. The worker has to decide both.

### U3. Task 8: the implementer projection is out of reach from the task's files
- Sentence: "print a block for each finding ... read from the linked review store in the implementer projection". Files: `tomlctl/src/plan/view.rs` and `tomlctl/tests/plan_view.rs` only. Constraint: "Touch no file outside this task's files."
- Evidence: `review/mod.rs:6` and `:8` declare `mod fields;` and `mod store;` as private. The projection `View` is `pub(super)` (`tomlctl/src/review/store.rs:448`), and so is `View::project` (`store.rs:498`). The loader `store::load` (`store.rs:1166`) and `fields::project` (`fields.rs:1055`) are `pub(crate)`, but they sit inside those private modules. The only things `review/mod.rs` exposes are `catalogue`, `dispatch` and `Tier`. `plan/view.rs` therefore cannot load a review store or project a finding without editing `tomlctl/src/review/mod.rs`, and probably `store.rs` as well. The only other route is to re-implement the projection, which would bypass the vis tables. Neither path is allowed by the task as written. The file set needs `review/mod.rs`, which tasks 3-7 also edit, or a stated alternative such as a re-export added by an earlier task.

### U4. Task 9: "reopened" cannot be counted "from the store alone"
- Sentence: "apply outcomes (scheduled, resolved, reopened) ... read from the store alone".
- What is missing: what marks a finding as reopened. Task 7, and the task 2 design, clear `apply_flow`/`apply_task` when a finding returns to `open`. Neither says that reconcile writes `reopen_reason` (the `open` companion, `fields.rs:146`) or any other marker. After the reopen, an apply-reopened finding looks the same as any other `open` finding. The worker either has to invent a marker in a file it does not own (`reconcile.rs`, task 7) or leave the count out.

## False premises

### F1. Task 11: approving the new flow before its files are committed
- Claim (design `engine.md#the-apply-carrier`, steps 3 and 6): the new flow is "approved after step 5 with `plan approve --review skipped-flag`". Step 6 then commits "the flow files and the review store".
- Code: `refuse_uncommitted` (`tomlctl/src/plan/approve.rs:358-390`) refuses approval while `git status --porcelain --untracked-files=all --ignored` reports anything for the flow's committed stores or authored files. The refusal reads "commit the flow's files first, since the approval names a commit that holds the plan". `claude/skills/tomlctl/references/plan.md` §`plan approve` names `tasks.toml`, `plan.toml` and `brief.md`. At that point in the design order they are newly written and untracked, so `plan approve` fails every time on the flow-less path. A skill written to this order is wrong. It needs a commit before the approval, and the approval stamp then dirties the stores again, so a second commit follows.

## Observations (non-blocking)
- Task 6, `spec.constraints`: "a finding's `decision_answer` adds 'Decision: <answer>'". No such field exists at base: `grep -c decision_answer tomlctl/src/review/fields.rs` gives 0, and no milestone-A task adds one. A worker can read it as optional and absent for now.
- Task 6, `spec.accept` for defects is "left to the gate" without a `writes_build` value. The `Accept` doc (`tasks/schema.rs:258`, an absent key reads as true) and `drain.md` line 188 imply that it should be omitted or set true. That is inferable.
- Task 4: tomlctl already exits 1 on every error (`lib.rs:394-406`). A refusal or an unknown id therefore exits 1, the same code as "anchor does not hold". This is safe for both premise and `--gone` checks, but the test for "refuses --gone on a defect" has to tell the two apart by stderr.

## Check commands

These were run on the present tree at `faf02521`. rc is given for each.

Premises (all as the plan expects):
- `grep -q 'pub(crate) struct BacklogLink' tomlctl/src/tasks/schema.rs`: 0
- `grep -q 'SCHEMA_VERSION: i64 = 1' tomlctl/src/review/store.rs`: 0
- `grep -q 'Exported => "exported"' tomlctl/src/review/fields.rs`: 0
- `grep -q 'fn refusal(f: &Finding, family: Family)' tomlctl/src/review/export.rs`: 0
- `grep -q 'fn check_old' tomlctl/src/review/add.rs`: 0
- `grep -q 'pub(crate) fn add_many' tomlctl/src/tasks/add.rs`: 0
- `grep -q 'fn worker(view: &Plan' tomlctl/src/plan/view.rs`: 0
- `grep -q 'fn outcomes' tomlctl/src/review/stats.rs`: 0
- `grep -q '^version = "0.22.0"' tomlctl/Cargo.toml`: 0
- `grep -q 'version, "0.22.0"' tomlctl/tests/capabilities.rs`: 0
- `grep -q '"implement-v2.md"' tomlctl/src/cli/dispatch/tests/skills.rs`: 0
- `grep -q 'commands/implement-v2.md' scripts/link-v2-review.sh`: 0
- `grep -q 'Vetting, apply and harvest' claude/skills/implement-v2-engine/SKILL.md`: 0
- `grep -q 'review export-v1' claude/skills/review-v2-engine/references/triage-and-close.md`: 0
- The after:N premises fail at base, as they should: `rechecks` in `store.rs` 1, `select.rs` 1, `finding_links` in `schema.rs` 1, `schedule.rs` 1.

Forward grep/test accept checks all fail at base, so each one can tell a change from no change. Each returned 1, or 2 where the file does not exist yet:
- T1: `finding_links` in `schema.rs` 1, in `tasks-store.md` 1
- T2: `"scheduled"` 1, `SCHEMA_VERSION: i64 = 2` 1
- T3: `mod select` 1
- T4: `anchor.rs` 1
- T5: `recheck.rs` 1
- T7: `reconcile.rs` 1
- T8: `finding_links` in `view.rs` 1
- T9: `--apply` in `measurement.md` 1
- T10: version 0.23.0 1, `"review_schedule"` 1, glimpse lock 0.23.0 1 (the lock currently pins `tomlctl` 0.22.0), `review schedule` in `review.md` 1
- T11: `apply-v2.md` 2, skill 2, link scripts 1
- T12: `findings.md` 2, SKILL link 1, `review_reconcile` 1
- T13: `/apply-v2` in `triage-and-close` 1, `! grep 'review export-v1'` 1, `! grep -E 'review-apply|optimise-apply'` 1, `/apply-v2` in `CLAUDE.md` 1
- C1 check: 2

Not run, because each builds the crate. They are left to the orchestrator.
- Every `cargo nextest ... -E 'binary(...)'` accept: tasks 1, 2, 3, 4, 5, 6 (including the C2 `emitted_checks_parse` check), 7, 8, 9 and 10.
- Task 11's `cargo test --lib -- cli::dispatch::tests`.

Installed cargo-nextest is 0.9.133, which defaults to failing when a filter matches no tests. A filter on a new binary, or task 9's `test(apply)`, so fails at base. `tests/review_stats.rs` has no test whose name contains `apply`.

Guards that cannot catch a missing change:
- Task 8, accept 2 ("the worker view of a linked task prints its findings"): `binary(plan_view)` is marked `guard` and passes on the existing tests, so it cannot show that the new behaviour landed. Only the `view.rs` grep is forward.
- Task 11, accept 4 ("the carrier gate names apply-v2.md"): `cli::dispatch::tests` is a guard that passes at base whether or not `apply-v2.md` is registered. Nothing greps `skills.rs` for `"apply-v2.md"`.

## Coverage

Read all 13 tasks through `tomlctl plan view --for worker` and `tasks show --select id,files`.

Code checked against the tasks:
- `tasks/schema.rs`, `tasks/add.rs`
- `review/{mod,fields,store,export,resolve,triage,add}.rs`
- `cli/types/review/{mod,export,verdicts}.rs` and the file list
- `plan/view.rs` and `plan/approve.rs`
- `capabilities.rs`, `cli/types/mod.rs` FEATURES, `tests/capabilities.rs`
- `cli/dispatch/tests/{skills,lint,store_guard_gate,output_gate}.rs`
- `scripts/link-v2-review.{sh,ps1}`, `glimpse/Cargo.lock`
- `claude/skills/tomlctl/references/plan.md`
- `claude/skills/implement-v2-engine/{SKILL.md,references/drain.md}`, `claude/commands/implement-v2.md`
- `claude/skills/review-v2-engine/{SKILL.md,references/triage-and-close.md,compose.md,survey.md}`, `claude/commands/{review,optimise}-v2.md`, `CLAUDE.md`

Not checked: no cargo build or test was run. Whether a version-1 review store fixture anywhere else asserts the written version stayed at 1 was checked only by grep. The hits were in v1 ledgers, flow stores and `review/store.rs`, which task 2 owns.
