Status: done — plan check gains plan/needs-gate-effect (warning) and plan/external-gate-unknown (error), with tests and table rows

```toml run-note
files_touched = ["tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs", "claude/skills/tomlctl/references/plan.md"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_check.rs"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep -q 'plan/needs-gate-effect' tomlctl/src/plan/check.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan/needs-gate-effect' claude/skills/tomlctl/references/plan.md"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_check::/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/check.rs`: `milestone_findings` now reports `plan/external-gate-unknown`
  (error) for each `external_gates` entry equal to none of the milestone's `gates`, worded like
  `plan/gate-probe-unknown` beside it. A new `needs_gate_effect_findings` walks each pending row's
  `needs` transitively with a visited set, and reports the row once (warning) when a reached row
  other than itself, in the same checkpoint, holds an accept entry with `effect = "external"` whose
  owner is `gate`. When several qualify, the lowest reached id is named. The detail names both
  rows and the check, and says the effect exists only after the milestone's gate. The walk passes
  through rows in other milestones, but only a reached row in the row's own milestone is reported.
  Rows with no checkpoint are skipped. The module header lists the new warning.
- `tomlctl/tests/plan_check.rs`: five tests. A same-milestone need warns and names rows 2 and 1
  and the check. A two-hop need warns on each pending row that reaches the effect. A need across
  milestones is clean. A need on a row with no external accept is clean. An `external_gates`
  entry naming no gate errors, and a matching one is clean.
- `claude/skills/tomlctl/references/plan.md`: added rows for both classes to the check-class
  table. The file already held uncommitted edits from sibling tasks (the `--external-gate` flag
  rows and the `plan emit` leaf list), and they are left as found.

The gate runs the plan_check nextest filter, so it was not run here. The first clippy run failed
on `tomlctl/src/plan/run_gate.rs` and `gate_blocks.rs`, which are a sibling's in-flight files. A
rerun a few minutes later was clean across all targets.
