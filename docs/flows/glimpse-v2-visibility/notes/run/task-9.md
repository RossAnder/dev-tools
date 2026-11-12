Status: done — `tomlctl plan run gate` records a gate round with its commands and keeps each non-pass log's cut tail under the flow's gates directory

## What changed

- `tomlctl/src/plan/run_gate.rs`: the leaf. Parses and checks every row before the store lock (unknown key, unknown outcome, blank or duplicated `command` refuse the batch naming the line). Inside `store::mutate` it either appends a new `[[gate_round]]` with `seq` one past the highest for (milestone, round), or under `--rerun` picks the round's latest run (refused, `kind=validation`, when there is none) and replaces commands by `command`, appending new ones. For each non-pass row with a `log`, it reads the source only when its canonical path lies under the canonical `std::env::temp_dir()`, keeps the last 200 lines cut to the last 64 KiB (starting on a UTF-8 boundary) as `.claude/flows/<slug>/gates/<milestone>-r<round>-s<seq>-c<n>.log`, `n` the 1-based position after the merge, and stores that repo-relative path; otherwise `log = ""` and the command goes in `unread`. Round outcome: pass when all pass/flaky, timeout when every other command timed out, else fail. `at` is the call time. Output `{ok, milestone, round, outcome, commands:[{command,outcome,log}], unread}`, `--lines` splitting on `commands`. Two unit tests (round outcome, tail cut).
- `tomlctl/src/cli/types/plan.rs`: `PlanRunOp::Gate(PlanRunGateArgs)` with `--milestone`, `--round`, `--ndjson`, `--rerun` and the write bundle.
- `tomlctl/src/plan/dispatch.rs`: `PlanRunOp::Gate(args) => run_gate::run(args)`.
- `tomlctl/tests/plan_run_records.rs`: failing round with a kept, cut log tail; the 200-line tail; pass round (exit omitted, defaults, no gates dir); `--rerun` replacing one command; a plain re-call minting `seq = 2`; `--rerun` on an unrecorded round refused; a log outside the temp directory and a missing log both stored `""` and listed in `unread`; refused rows (unknown key, unknown outcome) writing nothing and naming line 2; only `gate_round` written.
- `tomlctl/tests/output_options.rs`: the `plan run gate` write case, rows on `commands`.
- `claude/skills/tomlctl/references/plan.md`: contents entry, the `run …|gate` list in the preamble, and the `plan run gate` section after `plan run notes`.

## Deviations

- The plan.md section is written with long unwrapped prose lines and the row rules folded into the flag table: the file sat at 577 lines after the notes section and the `skill_references_under_line_ceiling` gate refuses a reference over 600 lines; a normally wrapped section reached 626. Alternatives: split the run-record verbs into a new reference file (outside this task's files), or drop content. It now sits at 599.
- Choices the detail left open: a non-pass row with no `log` stores `""` and is not listed in `unread` (nothing was named to read); `exit = null` reads as absent; a fractional `duration_s` is rounded; a command named twice in one batch is refused; the milestone id must be file-name safe (letters, digits, `-`, `_`, `.`, not leading `.`) since it names the kept files; a `--rerun` replacement that now passes leaves the earlier kept file on disk, unreferenced, rather than deleting it.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on run_gate.rs, dispatch.rs, cli/types/plan.rs: clean (the two test files were formatted with rustfmt too; `tests/common` was left unchanged).
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests plan::run_gate`: 57 passed (first run failed on the 600-line reference ceiling, fixed as above); `plan::run_gate` unit tests 2 passed.
- Accept `grep -q "PlanRunOp::Gate" tomlctl/src/plan/dispatch.rs`: pass.
- Accept `grep -q "plan run gate" claude/skills/tomlctl/references/plan.md`: pass.
- Accept `grep -q "\"gate\"" tomlctl/tests/plan_run_records.rs`: pass.
- `binary(plan_run_records)` nextest: gate's to run, not run here; the tests compile under clippy `--all-targets`.

TANGENTIAL: debt | claude/skills/tomlctl/references/plan.md | plan.md is at 599 of the 600-line reference ceiling, so the `plan run report` section will overflow it | the run-record verbs (notes, gate, report) want their own reference file, e.g. a plan-run.md linked from plan.md, or the milestone gate's dispatch tests fail
