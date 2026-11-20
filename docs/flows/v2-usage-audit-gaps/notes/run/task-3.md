Status: done — plan run gate --base records base reruns by order, skips parse in gate_blocks.rs, and a failing run records its tree digests

```toml run-note
files_touched = [
  "tomlctl/src/plan/run_gate.rs",
  "tomlctl/src/plan/gate_blocks.rs",
  "tomlctl/tests/plan_run_records.rs",
  "claude/skills/tomlctl/references/plan-run.md",
]

[[deviations]]
what = "A `--accept` call is refused with a validation error in run_gate.rs's run()"
why = "The flag (added by the CLI types task) has to be destructured for run_gate.rs to compile, and the gate_rows group lets `--accept` stand alone, so without a branch it would fall through to the no-rows refusal. The acceptance task replaces this branch."
means = ""

[[deviations]]
what = "A base result of `timeout` leaves the command base_checked = false, like a skip"
why = "records.md defines pass and fail base results only; a timed-out base rerun says nothing about whether the failure predates the milestone, so marking it checked would read as 'not a base failure' on no evidence."
means = ""

[[deviations]]
what = "A skip must name the failing command at its position, or the base call is refused naming its line"
why = "The mapping is by order; a cheap equality check on the one row that carries the round's own command text catches a misordered call before it writes wrong base fields. Rerun rows are not checked, since a narrowed rerun names a different command line."
means = ""

[[deviations]]
what = "A `skip:` line is recognised by gate_blocks::parse only when its new `skips` argument is true (set by --base); without it the line stays ordinary text"
why = "The constraint says a call without --base records exactly what it records today; recognising skip lines unconditionally would also end a fail block's tail at a log line starting `skip: `."
means = ""

[[deviations]]
what = "A rerun merge (--rerun) rewrites tree from the merged outcome: written on fail, cleared otherwise"
why = "The tree belongs to the recorded run's outcome; a merge that turns a run to pass leaves no failing run for gate-list to compare against."
means = ""

[[checks]]
command = "grep -q 'base_checked = true' tomlctl/src/plan/run_gate.rs"
result = "pass"

[[checks]]
command = "grep -q 'fn whitespace_digest' tomlctl/src/plan/run_gate.rs"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/run_gate.rs tomlctl/src/plan/gate_blocks.rs tomlctl/tests/plan_run_records.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`tomlctl/src/plan/run_gate.rs`
- `run()` destructures the new `base`, `accept` and `json` fields, keeps the loaded task store, and passes `base` to the blocks parser.
- `--base` goes to `record_base`: parses each row (a skip row holds only `command` and `skip`), finds the round's latest run (refused naming `tomlctl plan run gate --slug <slug> --milestone <id> --round <n> --blocks <src>` when there is none), refuses a row count other than the run's `fail` commands, refuses a skip naming another command, then sets each command's `base_checked`, `base_failed_ids` and `base_whole_failed` by records.md's mapping. `base_whole_failed` is set only for a command in the milestone's `gates`. Prints one entry per failing command with a computed `base_failure`.
- Without `--base`, a `skip` row is refused naming `--base`, and when the run's outcome is `fail` the round's `tree` holds every normalised path in the milestone rows' `files`, sorted, with raw and whitespace-stripped SHA-256 from disk under the repo root, `deleted` for an unreadable path.
- `pub(super) fn whitespace_digest(path) -> (raw, stripped)` and `pub(super) const DELETED` for `plan emit gate-list` to read. `latest_run` is split out of `target_round` and shared.

`tomlctl/src/plan/gate_blocks.rs`: `parse` takes `skips: bool`; with it, `skip: <command>` is its own row `{"command", "skip": true}` and ends a block's tail. A unit test covers both modes and a blank skip.

`tomlctl/tests/plan_run_records.rs`: nine tests over a `base_flow` fixture whose gates are `cargo test` and `cargo nextest`. They cover full and partial base failures with foreign ids ignored, a whole gate failing at base, a whole command passing at base, an id-less command outside the gates, a skip between two reruns in `--blocks`, a skip without `--base` refused, a base call on a round with no run refused, the wrong count refused, and the tree on a failing run with none on a passing run.

`claude/skills/tomlctl/references/plan-run.md`: the `tree` paragraph, the `--base` flag row and synopsis, a Base reruns section (order mapping, skips, the result table, the base-failure rule, refusals, output), and the skip line added to the tail-ending rule.

## Checks

The clippy run fails on three errors, all in files other tasks hold: `tomlctl/src/plan/approve.rs` (an unmentioned `waive_review_delta` field), `tomlctl/src/plan/emit.rs` (an unmentioned `after_round` field) and `tomlctl/src/plan/records.rs` (wrong argument count in a test). It reports none in the four files of this task. Because those errors stop the compile, borrowck and lints never ran on this code, so the gate's clippy is the first full check. For the same reason the dispatch-tests subset, which CLAUDE.md asks for after a `claude/**/*.md` edit, could not build and was not run. The `plan_run_records` nextest check belongs to the gate.
