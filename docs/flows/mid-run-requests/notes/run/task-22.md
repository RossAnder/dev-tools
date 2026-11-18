Status: done — moved the doc comment from request_flow to plan_flows

```toml run-note
files_touched = ["tomlctl/tests/output_options.rs"]
```

## What changed

In `tomlctl/tests/output_options.rs` the three-line doc comment now sits above `plan_flows`; `request_flow` is left bare. Comment-only edit, no compile check run. The gate-owned accept check was not run.
