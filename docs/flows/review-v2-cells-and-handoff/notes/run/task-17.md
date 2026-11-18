Status: done — four review features added to FEATURES, the capabilities test and the README

```toml run-note
files_touched = ["tomlctl/src/cli/types/mod.rs", "tomlctl/tests/capabilities.rs", "tomlctl/README.md"]

[[checks]]
command = "grep for the four feature names in tomlctl/src/cli/types/mod.rs"
result = "pass"
```

## What changed

- `tomlctl/src/cli/types/mod.rs`: FEATURES gains review_propose_kinds, review_plan_ceiling, review_escalations, review_stats_yield.
- `tomlctl/tests/capabilities.rs`: the expected list pins the four names.
- `tomlctl/README.md`: sample capabilities block and feature table list them.

The capabilities nextest check is the gate's and was not run.
