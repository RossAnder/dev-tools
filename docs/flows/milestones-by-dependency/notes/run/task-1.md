Status: done — MilestoneSpec carries an `after` list, read, written (omitted when empty) and exported in the snapshot.

```toml run-note
files_touched = ["tomlctl/src/tasks/schema.rs", "tomlctl/src/tasks/snapshot.rs", "tomlctl/src/plan/milestone.rs"]

[[checks]]
command = "grep -qF 'pub(crate) after: Vec<String>' tomlctl/src/tasks/schema.rs"
result = "pass"

[[checks]]
command = "grep -qF '\"after\": ' tomlctl/src/tasks/snapshot.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"
```

## What changed

- schema.rs: `after` field, read with `lf_array`, written only when non-empty, fixture updated, one round-trip unit test (not run; the nextest accept check is the gate's).
- snapshot.rs: `"after"` in the milestone object.
- milestone.rs: `after: Vec::new()` at both constructions.
