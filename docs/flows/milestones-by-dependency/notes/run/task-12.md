Status: done — milestone_schedule added to FEATURES, capabilities test, README and implement-v2 pre-flight

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md", "claude/commands/implement-v2.md"]

[[checks]]
command = "grep -qF feature in mod.rs, implement-v2.md, README.md"
result = "pass"
```

## What changed
- mod.rs and capabilities.rs: `milestone_schedule` after `plan_emit_lane_check`.
- README: sample block entry and feature-table row.
- implement-v2.md: pre-flight list, stale-binary sentence ("pick a milestone by its dependencies"), and the `--milestone` Bindings row reworded.

The nextest accept check is the gate's; not run.
