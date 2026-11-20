Status: done — test call passes None for task_ids and the blocks check is a let-chain

```toml run-note
files_touched = ["tomlctl/src/plan/records.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/records.rs"
result = "pass"
```

## What changed

`tomlctl/src/plan/records.rs`: the test helper `set` now passes `None` as `task_ids` to `upsert_handover`. The nested `if` in the blocks check is now one let-chain. Clippy reported nothing for records.rs. The gate-owned accept checks were not run.
