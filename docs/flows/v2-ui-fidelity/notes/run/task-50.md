Status: done — the look-state test now pins the control-state explanation

```toml run-note
files_touched = ["tomlctl/tests/plan_check.rs"]

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_check::check_ui_a_look_state_the_milestone/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_check.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_check.rs`: in the look-state-unknown test, the plain case asserts its finding detail lacks the control-state explanation, and the control case asserts its detail contains it.
