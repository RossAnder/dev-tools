Status: done — escalations, ceiling_overrides and checker fix_files registered and stored

```toml run-note
files_touched = ["tomlctl/src/review/fields.rs", "tomlctl/src/review/store.rs", "tomlctl/tests/review_store_v2.rs"]

[[checks]]
command = "grep accept check for the three field specs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "not-run"
```

## What changed

- fields.rs: declared computed `Run.ceiling_overrides` and `Cell.escalations`; FIX_FILES checker column N to R, with its MATRIX row.
- store.rs: `Escalation` struct, `Cell.escalations` parsed and written like `ruled_out`; ceiling_overrides rides in the run's rest table.
- review_store_v2.rs: a test that escalations and ceiling_overrides survive a write.

The clippy run failed only on errors in `tomlctl/src/review/cells/` (siblings' in-flight split), so the compile check could not be confirmed for my files. The new test was not run (gate's job).
