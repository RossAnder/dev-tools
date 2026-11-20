Status: done — added three feature strings and bumped tomlctl to 0.26.0

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md", "tomlctl/Cargo.toml", "Cargo.lock"]

[[checks]]
command = "grep for the three feature strings in tomlctl/src/cli/types/mod.rs"
result = "pass"

[[checks]]
command = "grep -q '^version = \"0.26.0\"' tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/capabilities.rs"
result = "pass"
```

## What changed

- `mod.rs`: `plan_emit_commit_paths`, `effect_mark`, `plan_run_gate_blocks` added to FEATURES after `plan_emit_lane_check`.
- `capabilities.rs`: three strings added to the feature list; version pin now 0.26.0.
- `README.md`: sample block and table carry the three features, sample version 0.26.0, `commit-paths` named in the `plan emit` leaf list.
- `Cargo.toml` and `Cargo.lock`: version 0.26.0 (lock edited by hand, one line).

The nextest capabilities check is the gate's and was not run.
