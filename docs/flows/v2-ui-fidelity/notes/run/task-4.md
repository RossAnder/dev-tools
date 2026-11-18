Status: done — milestones carry optional ui_states and ui_controls, set by --ui-state, --ui-control and --clear-ui, and stamp the task store at version 3

```toml run-note
files_touched = [
  "tomlctl/src/tasks/schema.rs",
  "tomlctl/src/plan/milestone.rs",
  "tomlctl/src/cli/types/plan.rs",
  "tomlctl/tests/plan_milestone.rs",
  "tomlctl/src/plan/lane.rs",
  "tomlctl/src/tasks/milestones.rs",
]

[[tangential]]
kind = "question"
area = "claude/skills/tomlctl/references/plan.md"
summary = "The new --ui-state, --ui-control and --clear-ui flags on plan milestone add and update are not yet in the plan reference's flag tables; a sibling documentation task in this milestone owns that file."
why = "If the flag-table gate compares clap flags with the reference, it fails until that documentation lands."

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check on each edited file"
result = "pass"

[[checks]]
command = "grep -q 'ui_states' tomlctl/src/tasks/schema.rs && grep -q 'ui_controls' tomlctl/src/tasks/schema.rs && grep -q 'ui-control' tomlctl/src/cli/types/plan.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/milestone_ui/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/tasks/schema.rs`: `MilestoneSpec` gains `ui_states` and `ui_controls`. They are read with `str_array` and written only when they hold entries, so a milestone without them keeps its bytes. `SCHEMA_VERSION` is now 3, and a new `V2_SCHEMA_VERSION` (2) sits beside `BASE_SCHEMA_VERSION` (1). A new `holds_v3_keys` predicate is true when any keyed milestone carries a ui list. `to_toml` stamps 3, 2 or 1 from `holds_v3_keys`, then `holds_v2_keys`, then the base. `holds_v2_keys` is unchanged, so `tasks remove`'s v2 detection is unaffected. The unit tests now use `V2_SCHEMA_VERSION` where they meant the v2 stamp. A new test checks that either ui list alone stamps 3 and round-trips. The newer-version refusal test now accepts 3 and refuses 4. Task 5 can extend `holds_v3_keys` to look entries.
- `tomlctl/src/cli/types/plan.rs`: `plan milestone add` takes the repeatable `--ui-state` and `--ui-control`. `update` takes the same two flags plus `--clear-ui`.
- `tomlctl/src/plan/milestone.rs`: before the lock, both verbs refuse a name outside `^[a-z0-9][a-z0-9-]{0,63}$` or a name given twice to one flag. `add` also refuses a name that appears in both lists. On `update`, each flag replaces its own list whole, and `--clear-ui` empties both but is refused alongside either flag. `MilestonePatch` gains `ui_states` and `ui_controls`. `patch_milestone` checks that the resulting lists share no name, including one already stored in the other list, and writes an audit row for each list that changed on an audited store.
- `tomlctl/src/plan/lane.rs`, `tomlctl/src/tasks/milestones.rs`: in the test `MilestoneSpec` literals only, the new fields are empty.
- `tomlctl/tests/plan_milestone.rs`: four `milestone_ui_*` tests cover set, replace and clear (including the stamp moving between 3 and 2), refusal of bad, doubled and cross-list names with no write, byte-identical bytes after a set followed by a clear, and the audit rows.

The `milestone_ui` nextest accept check is the gate's to run and was not run here. Clippy compiled the new tests without error.
