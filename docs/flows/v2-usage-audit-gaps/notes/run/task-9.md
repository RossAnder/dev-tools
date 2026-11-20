Status: done — plan drift now emits affected_decisions; compile and tests not runnable yet because of sibling edits

```toml run-note
files_touched = ["tomlctl/src/plan/drift.rs", "tomlctl/tests/plan_drift.rs"]

[[checks]]
command = "grep -q 'affected_decisions' tomlctl/src/plan/drift.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "not-run"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/drift.rs tomlctl/tests/plan_drift.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/drift.rs`: the git log is now parsed into unowned commits with their files (`parse_commits`). `changed_files` derives from those. New `affected_decisions` lists `{id, binds_hit, commits}` per decision whose path binds (not all-digit) meet `changed_files`, and a decision with no hit is left out. The key is added to the report, and the existing keys keep their shape. `parse_log` is kept for the unit tests under `cfg(test)`.
- `tomlctl/tests/plan_drift.rs`: two tests. A hand commit on a bound file lists the decision and its commit. An untouched bind lists nothing.

## Checks

Clippy failed on sibling files only (schema.rs, approve.rs, emit.rs, records.rs), from other workers' in-flight edits. Nothing named drift.rs. The plan_drift nextest run did not compile for the same reason, so the new tests have not been run. The gate runs them.
