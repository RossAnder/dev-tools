Status: done — plan/generated-output-unheld warning added and tested

```toml run-note
files_touched = ["tomlctl/src/plan/check/generated_outputs.rs", "tomlctl/src/plan/check.rs", "tomlctl/tests/plan_check.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q 'plan/generated-output-unheld' tomlctl/src/plan/check/generated_outputs.rs && grep -q 'mod generated_outputs' tomlctl/src/plan/check.rs"
result = "pass"

[[checks]]
command = "grep -q 'plan/generated-output-unheld' tomlctl/tests/plan_check.rs"
result = "pass"
```

## What changed

- New `generated_outputs.rs`: warns per pending row of a non-deferred milestone that holds a generator input without every output; a malformed config is one warning naming no row.
- `check.rs`: module declared, registered after `ui_scope_findings` outside the before-approval block, header list updated.
- `plan_check.rs`: four tests (unheld warns, held clean, no config clean, malformed warns once). The four ran and passed narrowly.
