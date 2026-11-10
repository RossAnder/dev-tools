Verdict: Milestone C is mostly carry-out-able as written. Two points stop it: task 17 cannot both delete `FIX_SHAPES` and build without touching `query.rs`, and task 24's guard check uses `node --check`, which fails on the present tree.

# Cold read: milestone C (tasks 17-28)

## Unactionable

### U1. Task 17: removing `FIX_SHAPES` breaks the crate build, and the only file that would fix it is task 18's
- Task 17 accept: "fix_shape's value list is gone from the catalogue", check `! grep -q 'const FIX_SHAPES' tomlctl/src/review/fields.rs`. Constraint: "Touch no file outside this task's files."
- `tomlctl/src/review/query.rs:55` reads `check_value("--fix-shape", args.fix_shape.as_deref(), fields::FIX_SHAPES)?;`. `query.rs` is not in task 17's files. It belongs to task 18, which needs 17.
- Once the const is deleted, the lib stops compiling. Task 17's own build-writing checks (`binary(review_add) & test(blocked)`, `test(schema)`) then fail to compile. So does task 25's guard (`cargo test --lib -- cli::dispatch::tests`), which needs only 17. A worker has to guess among four ways out:
  - keep the const under another name
  - inline the list in `query.rs`, which breaks the file rule
  - leave the build broken until 18 lands
  - report `failed`
- The plan settles none of these. One fix is to give task 17 `query.rs` for the one line, or to drop the const removal from 17's accept and leave it to 18.

## False premises

### F1. Task 24: "the script parses" is checked with `node --check claude/workflows/review-v2.js`, which fails on the present tree
- Running the check now prints `SyntaxError: Illegal return statement` at `claude/workflows/review-v2.js:47` (`return index([], 0)`) and exits 1.
- Workflow scripts are function bodies with a top-level `return`. The repo says so at `tomlctl/src/cli/dispatch/tests/lint.rs:1036`: "It validates no JS syntax, because a script's top-level `return` fails `node --check`."
- The guard therefore fails with or without task 24's change, so it cannot pass. Use the `cli::dispatch::tests` workflow-script gate instead, or a syntax check that wraps the body in a function.

## Check commands (run on the present tree, base `faf02521`)

Premises at base:
- 17 `FIX_SHAPES ... ["edits", "needs-plan"]` grep: holds. 17 `WhenChallenge` grep: holds (`fields.rs:312`, `:619`, `:817`).
- 24 `fix_shape` in `review-v2-method/SKILL.md`: holds (lines 34, 58).
- 25 nine catalogue entries name needs-plan: holds. All nine files listed in task 25 match, and each holds the instruction in one sentence or bullet. In the three `checks/` files the sentence also names `fix_shape = "edits"`, and the rewrite has to drop that too, since the accept greps for `fix_shape`.
- 26 `review_list_fix_shape` in the engine's `SKILL.md`: holds (line 78). It is also at `plan-flow.md:38`, which task 26 names.
- 27 `plan-worker-procedure` block in both workers: holds. `scripts/shared-blocks.toml:80-84` lists exactly those two files.
- 28 `Not started: apply` in the idea doc: holds (`docs/ideas/v2-pipeline.md:13`).
- `after:` premises (18, 19, 20 `"blocked_on"`; 21 `settle.rs`; 23 version 0.23.0; 22 `apply` in `stats.rs`) are not yet true at base, as expected, except one. Task 22's probe `grep -q 'apply' tomlctl/src/review/stats.rs` already holds at base through `"wontapply"` (`stats.rs:675`), so it cannot discriminate. This is minor.

Forward accept checks: each one fails now, as it should before the change:
- 17: `"blocked_on"` grep and `! const FIX_SHAPES`
- 18: `blocked_on` in `read.rs`
- 19: `unblocks`
- 20: `test -f settle.rs`
- 21: `settle_checkpoint`
- 22: `misclassif`
- 23: version 0.24.0, the capability pair, `fix_shape` in `review.md`/README (still present), and glimpse lock. The lock currently reads `tomlctl 0.22.0`.
- 24: method grep and the `unblocks` pair
- 25: lenses grep
- 26: both greps
- 27: all three greps
- 28: both greps

Guards and other checks:
- `bash scripts/verify-shared-blocks.sh`: passes now (24, 27).
- `node --check claude/workflows/review-v2.js`: fails now (see F1).
- C3 / task 18 `! grep -q '"fix_shape") [!=]= "needs-plan"' ...select.rs export.rs digest.rs query.rs`. With GNU grep it fails now, correctly: `export.rs:257` and `:329` and `digest.rs:519` match. Under this session's grep wrapper (ugrep) it passed, because `select.rs` does not exist at base and ugrep `-q` exits 2 on the missing file. `select.rs` is created by task 3 (milestone A), so once A has landed this does not matter. Note only.
- Cargo/nextest checks (17, 18, 19, 20, 21, 22; 25's lib test) were not run, because they need a build. U1 predicts that the ones run between task 17 and task 18 will not compile.

## Other observations (not counted)
- Task 17 says `review triage --convert` must refuse the same rows, but `triage.rs` is not in its files. That is fine: convert validates through `add::field_refusals` (`triage.rs:649`), which goes to `fields.rs`.
- Task 17's `test(schema)` filter matches `schema_agrees_with_field_catalogue` (`schema_out.rs:337`) and also unrelated `*schema*` tests across the crate. It is broad but works.
- Between tasks 17 and 18, `tests/review_query.rs`, `review_digest.rs` and `review_export.rs` (task 18's) still write `fix_shape` and will fail. Both tasks are in the same milestone and the gate runs at the end, so this is expected.
- Task 22 "per agent" comes from the finding's computed `cell` (`fields.rs:526`) to `Cell.agent` (`fields.rs:509`). A worker can infer this.
- Task 23's feature names are fixed in its Design (`review_blocked_on`, `review_settle`, `review_triage_answer`).
- Task 28's "Apply decisions" is the `### Apply` section at `v2-pipeline.md:159`. That file has uncommitted edits in the working tree right now.

## Coverage
- Read the worker views and the `tasks show` file lists for tasks 17-28.
- Read or grepped `tomlctl/src/review/{fields,query,digest,export,triage,add,verdict,runs,stats,schema_out,store}.rs`, `tomlctl/src/cli/types/{mod.rs,review/read.rs,review/export.rs}`, the test files that name `fix_shape`, `tests/review_runs.rs` (the schema test) and `cli/dispatch/tests/lint.rs`.
- Also read: the nine catalogue entries, the review-v2-engine `SKILL.md`, `triage-and-close.md` and `plan-flow.md`, implement-v2-engine `drain.md`, the workflow script, `scripts/shared-blocks.toml`, `CLAUDE.md` and the idea doc.
- `select.rs`, `schedule.rs`, `findings.md` and `apply-v2-engine/SKILL.md` do not exist at base. Tasks 3, 6, 12 and 11 (earlier milestones) create them, and I checked through the task store that they do. I could not read their future content.
- I ran no cargo build or test.
