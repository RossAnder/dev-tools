Status: done — reference docs now list `look` as a run_by owner, stamp 3 for look/UI keys, and name the milestone-update fields after, ui_states and ui_controls

```toml run-note
files_touched = [
  "claude/skills/tomlctl/references/plan.md",
  "claude/skills/tomlctl/references/tasks-store.md",
  "claude/skills/flow-contract-task-store/SKILL.md",
]

[[deviations]]
what = "The SKILL.md schema_version row also gained `finding_links` among the keys that stamp 2"
why = "holds_v2_keys in tomlctl/src/tasks/schema.rs counts finding_links, and tasks-store.md already listed it; the row was rewritten for 3 anyway"
means = "detail: make the same addition to the SKILL.md row"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- `claude/skills/tomlctl/references/plan.md`: the `plan/accept-owner-invalid` row now lists `worker`, `gate`, `covered` or `look`, matching `RUN_BY_OWNERS`. The `plan milestone update` prose now names after, UI states and controls among the changes a call may make, lists `after`, `ui_states` and `ui_controls` among the `[[changes]]` fields, and says those rows' old and new are compact JSON. Both checked against the no-op guard and the change pushes in `tomlctl/src/plan/milestone.rs`.
- `claude/skills/tomlctl/references/tasks-store.md`: the accept-owner paragraph lists `look`. The `states` sentence says such a store is raised to `3`. The root-keys `schema_version` row gives the 3 / 2 / 1 stamp order from `holds_v3_keys` and `holds_v2_keys`, and says a binary that knows only `2` refuses a `3` store.
- `claude/skills/flow-contract-task-store/SKILL.md`: the `schema_version` row gets the same 3 / 2 / 1 rule and the refusal of a `3` store.

The accept check (`review reproduce-status`) is the gate's to run.
