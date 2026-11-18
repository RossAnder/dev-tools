Status: done — plan_request and generated_config registered, pinned, documented; carrier requires plan_request

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md", "claude/commands/implement-v2.md"]

[[checks]]
command = "grep accept check for both features and the carrier"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/capabilities.rs"
result = "pass"
```

## What changed

- `tomlctl/src/cli/types/mod.rs`: appended both features to FEATURES.
- `tomlctl/tests/capabilities.rs`: added both to the expected list; version pin now 0.25.0 with its message.
- `tomlctl/README.md`: feature table rows, sample block entries, sample version 0.25.0.
- `claude/commands/implement-v2.md`: `plan_request` added to the step 2 feature list.

The nextest capabilities check is the gate's and was not run.
