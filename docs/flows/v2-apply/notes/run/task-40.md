Status: done — `tomlctl capabilities` now reports `review settle --tier` as mechanical, cited, user, taken from the settle verb's own constant

## What changed

- `tomlctl/src/review/mod.rs`: added `pub(crate) use fields::SETTLE_TIERS;` beside the existing re-exports. Nothing else changed. The uncommitted `mod settle;` line already in this file's diff was there before this task started, from the earlier settle task, and was left alone.
- `tomlctl/src/capabilities.rs`:
  - added `("review settle", "tier", crate::review::SETTLE_TIERS)` to `SCOPED_ENUM_VALUES`, so the bare `tier` id stops falling back to the unscoped `ENUM_VALUES` duplicate tiers (A, B, C) for this one subcommand;
  - added a matching arm `("review settle", "tier") => crate::review::SETTLE_TIERS` to `scoped_enum_values_match_their_validators`. `review::settle` validates `--tier` against `fields::SETTLE_TIERS`, so this is the validator's own vocabulary;
  - added the unit test `review_settle_tier_reports_the_settle_vocabulary_not_the_dup_tiers`, which asserts that `build_agent_context` reports `review settle --tier` values as `["mechanical", "cited", "user"]` with type `enum`.
  - No other entry in either table changed. The unscoped `("tier", ["A","B","C"])` entry still applies to every other `tier` flag.

## Deviations

None.

## Checks

- `rustfmt --edition 2024 --check tomlctl/src/capabilities.rs`: clean after one `rustfmt` apply. The file's only `mod` is its inline `tests` module. `review/mod.rs` was not run through rustfmt because it has `mod` children; the one-line edit follows the surrounding style.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- Accept check "the scoped table names the settle tier" (grep): passes.
- Accept check "capabilities unit tests and capability integration tests" (`cargo nextest ... -E 'test(capabilities::) | binary(capabilities)'`): Writes build is yes, so the gate runs it. Not run here.

## Handover

None.

TANGENTIAL: none
