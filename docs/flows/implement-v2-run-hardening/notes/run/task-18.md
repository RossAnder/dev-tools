Status: done — the long-command cut assertion now expects 200 chars plus the `...` marker

```toml run-note
files_touched = ["tomlctl/tests/agents_audit.rs"]

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(=agents_audit::audit_reports_a_bun_suite_and_cuts_long_commands)'"
result = "not-run"
```

## What changed

`tomlctl/tests/agents_audit.rs`: the cut-row assertion checks the `...` suffix and a length of 203. The accept check is the gate's to run, so it was not run here.
