Status: done — snapshot emits `plan` (null without plan.toml) and tasks-store.md documents every new key

Changes:
- tomlctl/src/tasks/snapshot.rs: plan.toml is parsed once into a `PlanStore`; `running_milestone` and the new top-level `plan` key (`toml_to_json` over `plan::schema::to_toml`, null when absent) both read it. `plan` is inserted after `changes`. The unit test's key-order list gains `plan`.
- tomlctl/tests/tasks_snapshot.rs: key-order list gains `plan`; new test `the_plan_store_rides_under_plan_and_is_null_without_one` covers no plan.toml and one holding a decision and a run.
- claude/skills/tomlctl/references/tasks-store.md: envelope table gains rows for task `spec`, `file_notes`, `closes`/`refs`, `finding` and top-level `milestones`, `changes`, `plan`.

Earlier-attempt edits to snapshot.rs and tasks_snapshot.rs (the per-task keys, milestones, changes) were already in place and kept.

Checks:
- cargo clippy (tomlctl): clean.
- rustfmt --edition 2024 --check on both .rs files: clean.
- Worker accept checks (grep for "plan" in snapshot.rs, the awk/grep on tasks-store.md, grep in tasks_snapshot.rs): pass.
- cargo nextest binary(tasks_snapshot): gate-owned; my one run hit a transient `rust-lld: permission denied` writing the lib-test exe, so it did not execute.

Deviations: none.

TANGENTIAL: none
