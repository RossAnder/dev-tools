Status: done — plan.toml holds [[request]] rows with per-item rulings, mints Q ids, stamps schema_version 5 only when a request is held, and ReportKind gains `request`.

```toml run-note
files_touched = ["tomlctl/src/plan/schema.rs", "tomlctl/src/plan/schema_request.rs", "tomlctl/src/plan/mod.rs"]

[[deviations]]
what = "The vocabulary! macro now names the trait as `$crate::plan::schema::Vocabulary` in its impl, and `Table` became pub(super) alongside the read and write helpers"
why = "A macro_rules body resolves an unqualified trait path at the call site, so schema_request.rs would otherwise have to import Vocabulary just for the macro; the record helpers all take `&Table`"
means = "widen the visibility of exactly what schema_request.rs uses"

[[deviations]]
what = "An absent `by` on a path or change item reads as unset (\"\"); every other vocabulary key, `ruling` and `field` included, is refused when absent"
why = "\"\" is a legal `by` value meaning nobody has ruled yet, the same way a handover's absent reason reads as none; a present unknown value is still refused naming the vocabulary"
means = ""

[[deviations]]
what = "The `paths` and `changes` arrays and `ruled_at` are left out of the TOML while empty or unset"
why = "Follows the run and handover records, which drop empty optional keys, so a change-only request writes no `paths = []`; the round trip is the same either way"
means = ""

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 tomlctl/src/plan/schema_request.rs tomlctl/src/plan/schema.rs"
result = "pass"

[[checks]]
command = "grep -q 'struct Request' tomlctl/src/plan/schema_request.rs && grep -q 'Request => \"request\"' tomlctl/src/plan/schema.rs && grep -q 'mod schema_request' tomlctl/src/plan/mod.rs"
result = "pass"

[[checks]]
command = "grep -A1 '#\\[test\\]' tomlctl/src/plan/schema.rs tomlctl/src/plan/schema_request.rs | grep -q 'fn [a-z_0-9]*request'"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/plan::schema/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/schema_request.rs` (new): `Request`, `PathItem` and `ChangeItem`, with the vocabularies `RequestStatus` (open, ruled), `Ruling` (open, granted, denied, needs-user), `RuledBy` ("", tomlctl, orchestrator) and `ChangeField` (accept, means, constraints, detail, files) built with the `vocabulary!` macro. It also holds `request_from_toml` / `request_to_toml`, which write the keys in the order the spec gives, and `PlanStore::mint_request_id`, which follows `mint_id`'s rule under prefix `Q` and adds no `RecordKind` variant. Its tests cover: a rendered round trip that is byte-stable on a second write; key order at the root, the record and both item kinds; the version 5 stamp, both over run records and over the base; a store without a request keeping its tier and writing no `request` key; an open request omitting `ruled_at` and empty arrays; Q1 then Q2 with the high-water mark; a removed top id never being minted again; unknown keys being dropped at all three levels; an unknown status, ruling, by or field value being refused with its vocabulary listed; an absent `by` reading as unset; and missing required keys being refused.
- `tomlctl/src/plan/schema.rs`: `SCHEMA_VERSION` is now 5 and is stamped when the store holds a request. The class tier keeps 4 as `CLASS_SCHEMA_VERSION`, and the const asserts order all four tiers. `PlanStore.requests` is read and written under the key `request`, after `report`. `ReportKind::Request => "request"` is added. The `vocabulary!` macro is re-exported with `pub(super) use vocabulary;`, and `Table`, `records`, `insert_records`, `nested`, `required_u32`, `str_or`, `required_str`, `required_datetime`, `vocab` and `bool_or_false` became pub(super). The module doc and the version consts' docs now describe the new tier. The class-tier test was renamed to `a_decision_class_round_trips_and_stamps_the_class_version` and pinned to `CLASS_SCHEMA_VERSION`, and the fixture gained `requests: Vec::new()`.
- `tomlctl/src/plan/mod.rs`: `pub(crate) mod schema_request;`.

## Checks

On the first clippy run the lib failed to compile on a parse error at `tomlctl/src/plan/view.rs:715`, a sibling's file that was mid-edit. On the rerun the whole crate compiled, tests included. The only output was two `dead_code` warnings, for `ID_PREFIX` and `mint_request_id` in schema_request.rs. Nothing calls the minting function until the `plan request` verbs land in this milestone, and the gate's clippy does not deny warnings. The schema nextest filter is the gate's to run (Run by: gate), so I did not run it.
