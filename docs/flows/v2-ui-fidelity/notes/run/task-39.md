Status: done — `inputs` validate now refuses a `plan` ledger record without `flow`

```toml run-note
files_touched = ["tomlctl/src/inputs.rs", "tomlctl/tests/inputs.rs", "claude/skills/tomlctl/references/inputs.md"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/inputs.rs tomlctl/tests/inputs.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^inputs::/) | test(/a_target_needs_its_ledger/)'"
result = "pass"
```

## What changed

- `tomlctl/src/inputs.rs`: in `validate`, the plan-ledger guard now refuses any `ledger = "plan"`
  record that lacks `flow`, with "a `plan` ledger record names the `flow` whose tasks it
  concerns". A plan record carrying `scope` alone is refused by this guard too, and one carrying
  both is refused by the flow+scope check above it, so the old scope-only refusal is subsumed.
- `tomlctl/tests/inputs.rs`: `inputs_plan_ledger_questions_target_a_flow_task` now loops over two
  refused records, the scope-targeted plan question and the flow-less one from the failure
  scenario, asserting a validation error for each and that no record was written.
- `claude/skills/tomlctl/references/inputs.md`: the Target bullet states that ledger `plan`
  requires `flow`.

The gate's reproduce-status accept check was left to the gate. The 25 tests run (inputs integration
module plus the unit test named in the finding) passed.
