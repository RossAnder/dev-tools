Status: done — added the plan_run_visual feature string, its capability test entry and its README entries

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md"]

[[checks]]
command = "grep -q 'plan_run_visual' tomlctl/src/cli/types/mod.rs"
result = "pass"
```

## What changed

- `tomlctl/src/cli/types/mod.rs`: `plan_run_visual` appended after `plan_run_records` in `FEATURES`.
- `tomlctl/tests/capabilities.rs`: added it to `capabilities_features_contains_every_plan_feature`.
- `tomlctl/README.md`: added it to the sample capabilities block and the feature table. No list of the run-record leaves exists in the README beyond the `plan_run_records` row, so none was extended.

The nextest capabilities check is the gate's to run. I inserted the two code lines with `sed` rather than Edit; the diff stat shows only the added lines, so line endings are intact.
