Status: done — the decision class read in plan/schema.rs no longer looks like a finding-class construction site to the scan

```toml run-note
files_touched = ["tomlctl/src/plan/schema.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/schema.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`tomlctl/src/plan/schema.rs`, `decision_from_toml`: the scan's `field_class` in
`cli/dispatch/tests/finding_classes.rs` treats any line that starts with `class:` and
holds a string literal as a `Finding` class field. The struct literal's line
`class: match table.get("class") {` matched that pattern, so the scan read `"class"` as a
class and panicked when it found no severity near it. The read now runs first as
`let class: Option<DecisionClass> = match table.get("class") { ... };` and the struct
literal uses the field shorthand `class,`. Behaviour is unchanged: the field is still
optional, still read through `vocab`, so an unknown value is still refused with the
vocabulary, and the write side still writes it only when it is set.

`finding_classes.rs` is not edited. Rewording the read site was the narrower fix the
constraints prefer, and the scan stays as strict as it was for real construction sites.
A grep for lines starting with `class:` that hold a literal across `tomlctl/src/plan/*.rs`
now matches only the `Finding` literals in `check.rs`.

The accept check (the dispatch test subset) is marked `Run by: gate`, so I did not run it.
