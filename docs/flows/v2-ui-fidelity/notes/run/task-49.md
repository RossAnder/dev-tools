Status: done — added the test for the unsurfaced one-way decision branch of amend_report

```toml run-note
files_touched = ["tomlctl/tests/plan_stage.rs"]

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/plan_stage::a_changed_brief/)'"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_stage.rs"
result = "pass"
```

## What changed

`tomlctl/tests/plan_stage.rs`: new test `a_changed_brief_clears_an_unsurfaced_one_way_decision_before_naming_the_amend_it_would_refuse`, placed before the existing check-error test. It seeds an approved plan with an edited brief and an agent-assumed one-way decision with no `surfaced`, then asserts `next == "plan decide"` and the two blockers.
