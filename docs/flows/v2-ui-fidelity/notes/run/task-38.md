Status: done — spec_block now renders a look accept entry with Run by and States

```toml run-note
files_touched = ["tomlctl/src/plan/view.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/view.rs"
result = "pass"
```

## What changed

`tomlctl/src/plan/view.rs`: imported `RUN_BY_LOOK`; `spec_block` tests it before the empty-check branch and prints `Run by: look` and `States:` (code-quoted, comma-joined). The final edit swapped to `code_all` after the clippy run; trivially equivalent. The reproduce-status accept check is the gate's.
