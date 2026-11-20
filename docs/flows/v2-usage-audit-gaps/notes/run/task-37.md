Status: done — both tests now use an unknown owner; the plan_check test also asserts a worker-run building check is clean

```toml run-note
files_touched = ["tomlctl/tests/plan_check.rs", "tomlctl/tests/plan_stage.rs"]

[[checks]]
command = "cargo nextest run -E 'test(/^plan_check::.*owner/) | test(/^plan_stage::/)'"
result = "pass"
```

## What changed
plan_check.rs: test renamed to check_reports_an_unknown_owner_and_accepts_a_worker_building_check; it asserts clean for run_by worker with writes_build true, and plan/accept-owner-invalid for run_by "nobody". The stray covered_by case is kept.
plan_stage.rs: the owner-invalid fixture uses run_by "nobody".
