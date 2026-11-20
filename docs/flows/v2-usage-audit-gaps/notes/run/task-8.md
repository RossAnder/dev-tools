Status: done — handover set accepts blocks, ready_check and note

```toml run-note
files_touched = ["tomlctl/src/plan/records.rs", "tomlctl/tests/plan_records.rs"]

[[checks]]
command = "grep -q '\"ready_check\"' tomlctl/src/plan/records.rs && grep -q '\"blocks\"' tomlctl/src/plan/records.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "fail"
```

## What changed

- records.rs: HANDOVER_SET_KEYS gains blocks, ready_check, note. upsert_handover parses and stores them on mint and update; blocks ids are checked against the task store (loaded only when a row carries blocks) and an unknown id is refused naming it. record_value reads the new keys. done stays refused.
- plan_records.rs: tests for storing blocks and ready_check, note update on an existing handover, and an unknown blocked id refused.

Clippy failed only on sibling files (approve.rs, emit.rs, run_gate.rs missing struct fields); none in my files. The nextest check is the gate's. The new tests were not run for that reason.
