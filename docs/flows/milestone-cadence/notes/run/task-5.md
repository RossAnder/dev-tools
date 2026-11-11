Status: done — the lifecycle test now expects plan/milestone-width.

- tomlctl/tests/plan_lifecycle.rs: the first `finding_classes(&check)` expectation in `a_plan_runs_from_open_to_drift` lists `plan/milestone-width` after `plan/milestone-no-gate`. The later assertion (`["plan/milestone-no-gate"]`) is unchanged and passes.
- Checks: rustfmt --edition 2024 --check clean; grep accept check passes; `cargo nextest run ... test(a_plan_runs_from_open_to_drift)` passed (1/1). That check is gate-owned, but the detail asked for it and I ran it once.

TANGENTIAL: none
