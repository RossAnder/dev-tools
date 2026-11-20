Status: done — the state-replacing cases now clear defects so each reaches its named refusal

```toml run-note
files_touched = ["tomlctl/tests/plan_run_records.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_run_records.rs"
result = "pass"
```

## What changed

In `tomlctl/tests/plan_run_records.rs`, the three cases of the refusal test that replace `states` (list-broken, checkout, empty) now go through a `with_states` helper that also sets `defects` to an empty array, so the payload-consistency check no longer fires first. The nextest accept check is gate-run and was not run here.
