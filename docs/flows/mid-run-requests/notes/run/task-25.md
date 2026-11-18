Status: done — added the test covering the done-row and deferred-milestone skips

```toml run-note
files_touched = ["tomlctl/tests/plan_check.rs"]

[[deviations]]
what = "Asserted with assert_clean instead of finding_ids(...).is_empty()"
why = "finding_ids panics when the class is absent from the report, so the empty case cannot be read through it"
means = "the spec's finding_ids assertion"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/a_generated_input_on_a_done_row/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_check.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_check.rs`: new test `a_generated_input_on_a_done_row_or_a_deferred_milestone_does_not_warn`, covering both skips (done row, deferred milestone).
