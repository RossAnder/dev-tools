Status: done — `plan run notes` reads a milestone's run notes into `[[attempt]]` rows and reports each note's problem

```toml run-note
files_touched = [
  "tomlctl/src/plan/run_notes.rs",
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/src/plan/dispatch.rs",
  "tomlctl/tests/output_options.rs",
  "tomlctl/tests/plan_run_records.rs",
  "claude/skills/tomlctl/references/plan.md",
]

[[deviations]]
what = "The run read is the last unskipped `implement:<id>` row, and a run recording no `tasks` is refused with kind=validation"
why = "Matches `plan emit lane-check`'s run lookup and refusal, so the two drain verbs agree on which run a milestone's latest is"
means = ""

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --test plan_run_records --test output_options"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep -q Notes tomlctl/src/cli/types/plan.rs && grep -q PlanRunOp::Notes tomlctl/src/plan/dispatch.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan run notes' claude/skills/tomlctl/references/plan.md"
result = "pass"

[[checks]]
command = "test -f tomlctl/tests/plan_run_records.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'binary(plan_run_records)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/run_notes.rs`: the verb. Inside one `store::mutate` lock it finds the latest `implement:<id>` run, reads `docs/flows/<slug>/notes/run/task-<n>.md` for each dispatched task through `store::authored_dir` and the `authored_anchor` containment check (a note that resolves outside the flow's docs directory reads as missing), parses it with `run_note::parse`, and upserts one `Attempt` on (task, run_id), replacing in place or appending. The milestone is validated with `emit::load_milestone_store`, which also supplies the task rows' statuses. A missing note records `block = "absent"` with the row's status; a malformed block, or a first line that is not a `Status:` line, records `block = "malformed"`; neither fails the command. `problem` is one line, in the order: missing or outside, malformed, no task row, Status word differs from the row. Output goes through `print_rows_compact` with `Rows::Field("rows")`: `{ok, milestone, run_id, rows}`.
- `tomlctl/src/cli/types/plan.rs`: `PlanRunOp::Notes(PlanRunNotesArgs)` with `--slug`, `--milestone` and the shared write bundle.
- `tomlctl/src/plan/dispatch.rs`: the one-line arm and the `run_notes` import.
- `tomlctl/tests/output_options.rs`: a write case for `plan run notes` against the plan fixture's `implement:A` run, rows at `rows`, keyed on `task`.
- `tomlctl/tests/plan_run_records.rs` (new): a block note, an old-style note with TANGENTIAL lines, a missing note, a status disagreement, a malformed block and a non-Status first line, idempotence (byte-identical store and output on a second call), a later run adding attempts beside the earlier ones, only `attempt` changing in the store, and a milestone with no run refused as `not_found`.
- `claude/skills/tomlctl/references/plan.md`: Contents entry, `run ...|notes` in the not_found list, and a `## \`plan run notes\`` section after `plan run skip` with its flag table, output example and output-key table.

## Checks

- Compile check: `cargo clippy` on the crate and on the two test targets, both clean. `rustfmt --edition 2024 --check` clean on run_notes.rs, dispatch.rs, cli/types/plan.rs and plan_run_records.rs. output_options.rs was not rustfmt-checked on its own, since it declares `mod common`.
- `cli::dispatch::tests`: 55 passed.
- Worker accept checks (notes leaf, reference section, tests file): all pass.
- I did not run `binary(plan_run_records)` because it is the gate's check. The tests compile; whether they pass is for the gate to show.

## Notes for the orchestrator

- The run's `run_id` keys the attempts as stored. A run started without `--run-id` and never finished with one keys its attempts on `""`, so two such runs of a milestone would share keys. The engine always passes a run id, so I left this alone.
- The siblings named `cli/types/plan.rs`, `plan/dispatch.rs`, `tests/output_options.rs`, `tests/plan_run_records.rs` and `plan.md` as held by other tasks too. My edits are small hunks next to the `Skip` variant, the `Skip` arm, the abandon case and the `plan run skip` section. When I edited `plan_run_records.rs`, no other task had created it yet.

TANGENTIAL: none
