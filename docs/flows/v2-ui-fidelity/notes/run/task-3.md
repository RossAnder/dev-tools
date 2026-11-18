Status: done — plan view prints a decision's class on its Standing line and the digest counts decisions by class

```toml run-note
files_touched = ["tomlctl/src/plan/view.rs", "tomlctl/tests/plan_view.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -q 'view_decision_class' tomlctl/tests/plan_view.rs"
result = "pass"
```

## What changed

- `tomlctl/src/plan/view.rs`: the Standing line appends `, <class>` when set; the digest's `counts.decisions` gains `by_class`, present only when at least one decision is classed, so output for unclassed stores is unchanged.
- `tomlctl/tests/plan_view.rs`: added `view_decision_class`. The nextest run is the gate's.
