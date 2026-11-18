Status: done — added a files-less ui-only signal and the two assertions

```toml run-note
files_touched = ["tomlctl/tests/review_survey.rs"]

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^review_survey::ui_capture_paths/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/review_survey.rs"
result = "pass"
```

## What changed

`tomlctl/tests/review_survey.rs`: UI_CHANGE gains a `ui-only` diff signal with `detect = { ui_capture_paths = true }`. The declared-files test asserts it selects only `tui/src`. The no-config test asserts it selects no root (the assertion accepts either an absent or an empty root list).
