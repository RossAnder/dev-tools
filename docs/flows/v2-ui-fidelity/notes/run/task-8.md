Status: done — plan ledger added to the inputs store, with tests and reference docs

```toml run-note
files_touched = ["tomlctl/src/inputs.rs", "tomlctl/tests/inputs.rs", "claude/skills/tomlctl/references/inputs.md"]

[[checks]]
command = "grep -q '\"plan\"' tomlctl/src/inputs.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"
```

## What changed

- `tomlctl/src/inputs.rs`: `LEDGERS` gains `plan`; validation refuses a `plan` ledger record that names a `scope`.
- `tomlctl/tests/inputs.rs`: two tests prefixed `inputs_plan_ledger` (add and list by flow and item, scope refused, list by ledger). The nextest gate check was not run (gate-owned).
- `claude/skills/tomlctl/references/inputs.md`: ledger values and the Target rule document `plan`.
