Status: done — the id-prefix writer race now builds its commands with common::tomlctl_process()

```toml run-note
files_touched = ["tomlctl/tests/items_id_prefix.rs"]

[[checks]]
command = "grep -q 'tomlctl_process' tomlctl/tests/items_id_prefix.rs"
result = "pass"
```

## What changed

tomlctl/tests/items_id_prefix.rs: in `concurrent_id_prefix_adds_mint_distinct_ids` each writer uses `common::tomlctl_process()` with `TOMLCTL_ROOT` set to the sandbox root and `TOMLCTL_LOCK_TIMEOUT` kept at the test's existing `30`. No assertion changed. No compile check was run; the edit only swaps the command builder.
