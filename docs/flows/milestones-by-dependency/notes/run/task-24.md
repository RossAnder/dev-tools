Status: done — second finding-class expectation in the lifecycle test is back to ["plan/milestone-no-gate"]

```toml run-note
files_touched = ["tomlctl/tests/plan_lifecycle.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_lifecycle.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_lifecycle.rs`: the late `finding_classes(&check)` assertion in `a_plan_runs_from_open_to_drift` now expects only `plan/milestone-no-gate`. The first expectation is unchanged. The accept check is marked `Run by: gate`, so I did not run it.
