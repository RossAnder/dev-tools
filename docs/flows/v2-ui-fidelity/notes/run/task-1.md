Status: done — decisions carry an optional closed-vocabulary `class`, written only when set and stamping schema_version 4

```toml run-note
files_touched = ["tomlctl/src/plan/schema.rs", "tomlctl/src/plan/records.rs", "tomlctl/tests/plan_records.rs"]

[[deviations]]
what = "Added a named RUN_RECORDS_SCHEMA_VERSION (3) constant for the middle tier rather than writing the literal, and kept newer_schema_version_is_refused as SCHEMA_VERSION + 1 (now 5) instead of a literal 5"
why = "The existing tiers are named constants with compile-time ordering asserts; a third named tier keeps that idiom and the refusal test follows SCHEMA_VERSION automatically"
means = "move the test newer_schema_version_is_refused to 5"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/src/plan/schema.rs tomlctl/src/plan/records.rs tomlctl/tests/plan_records.rs"
result = "pass"

[[checks]]
command = "grep -q 'compromise' tomlctl/src/plan/schema.rs && grep -q '\"class\"' tomlctl/src/plan/records.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/decision_class/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/schema.rs`: new `DecisionClass` vocabulary (structure, look, ux, cost, vocabulary, architecture, compromise) declared with the `vocabulary!` macro beside `Provenance` and `Door`. `Decision` gains `class: Option<DecisionClass>`; `decision_from_toml` reads it when present (an unknown value is refused with the list), `decision_to_toml` writes it after `door` only when set, so classless stores keep their bytes. `SCHEMA_VERSION` is now 4, with a new `RUN_RECORDS_SCHEMA_VERSION` (3); `write_version` stamps 4 when any decision has a class, 3 for run records without a class, and `BASE_SCHEMA_VERSION` (2) otherwise. `from_toml` still refuses anything above `SCHEMA_VERSION`. Module header updated. Unit tests: the fixture now carries the run-records stamp, the tests that asserted the run-records stamp use the new constant, and three new tests cover class round-trip and stamping, no `class` key when unset, and refusal of an unknown class.
- `tomlctl/src/plan/records.rs`: `class` added to `DECISION_KEYS` (so it is also a review-edit target through `review_keys`), parsed in `upsert_decision` through `vocab_value` for create and update, and projected in `record_value` (`""` when unset, matching how a handover's absent reason projects).
- `tomlctl/tests/plan_records.rs`: integration tests for setting and updating a class through `plan decide` (stamp 4, unclassed sibling has no key), refusal of an unknown or wrong-case class naming the accepted values with nothing persisted, and a classless store staying at schema_version 2 with no `class` key.

No CLI flag change was needed: `plan decide` takes NDJSON rows, so the new key flows through `DECISION_KEYS`. A class cannot be cleared once set (a null or empty value is refused), the same as a handover's reason. The nextest accept check is the gate's to run, so I did not run it here.
