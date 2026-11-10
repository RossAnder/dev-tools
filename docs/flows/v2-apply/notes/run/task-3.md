Status: done — export-v1's finding selection and refusal rules now live in `review/select.rs`, which export-v1 calls

## What changed

- `tomlctl/src/review/select.rs` (new): `Selection` (chosen, held, refused, projected) with `Selection::new(verb, stored, root, family, all, ids)`, `refusal(f, family)`, `latest_verdict`, the `text` field helper and `TestGlobs`. They were moved out of export.rs unchanged, except as listed below. The family the destination accepts is an input, and the verb that prefixes the one error (a finding naming a run missing from the store) is passed in, so export-v1's error text stays `review export-v1: ...`.
- `tomlctl/src/review/export.rs`: the moved items are removed. `run` calls `select::Selection::new(VERB, &stored, &root, family, args.all, &args.ids)`. The imports are trimmed: `globset`, `run_catalogue`, `fields::{self, Role}` and `View` are dropped, and `select::{self, text}` is imported. `ledger_family` and its test stay here, as the constraint asks.
- `tomlctl/src/review/mod.rs`: `mod select;` added.

The refusal strings, the held reasons, the order and the JSON shapes are byte-for-byte the same as before, so export-v1's output and refusals do not change.

## Deviations

None from a `suggested` means; the task has none. Design choices I made:
- `Selection::new` takes `all: bool, ids: &[String]` instead of `&ReviewExportArgs`. That way a second verb can call it without building export's args struct. The alternative was a small shared args struct, which would add a type for no gain.
- `verb: &str` is a parameter, not a `VERB` constant inside select.rs, because the error message has to keep naming export-v1. One comment now says "must not fail selection" instead of "must not fail export"; this is comment text only.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml`: clean.
- `rustfmt --edition 2024 --check` on select.rs and export.rs: clean.
- Accept check "review group has a select module": passes.
- Accept check "export-v1 calls select:: and no longer holds refusal": passes.
- Accept check `cargo nextest ... binary(review_export)`: Writes build: yes, so the gate runs it.

## Handover

None.

TANGENTIAL: none
