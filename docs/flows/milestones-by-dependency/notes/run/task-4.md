Status: done — added the `plan emit schedule` leaf

```toml run-note
files_touched = ["tomlctl/src/plan/schedule.rs", "tomlctl/src/plan/mod.rs", "tomlctl/src/plan/emit.rs", "tomlctl/src/cli/types/plan.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/schedule.rs"
result = "pass"

[[checks]]
command = "both accept greps"
result = "pass"
```

## What changed

- `tomlctl/src/plan/schedule.rs` (new): loads plan.toml and the task store, builds the elaborating set from open `elaborate:<id>` runs, calls `tasks::milestones::schedule`, prints `{"pick","milestones":[{id,state,on,left}]}`; pick is an empty string when none.
- `tomlctl/src/plan/mod.rs`: `mod schedule;`.
- `tomlctl/src/plan/emit.rs`: dispatch arm.
- `tomlctl/src/cli/types/plan.rs`: `PlanEmitOp::Schedule` and `PlanEmitScheduleArgs`.

The `output_options` test entry for the new leaf belongs to a sibling task's files.
