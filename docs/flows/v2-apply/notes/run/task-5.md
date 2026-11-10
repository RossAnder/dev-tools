Status: done — `tomlctl review reproduce --ndjson` and `tomlctl review reproduce-status <ids> --expect` added

## What changed

- `tomlctl/src/review/reproduce.rs` (new): both verbs.
  - `record` (`review reproduce`): reads the NDJSON batch and checks each row: the keys allowed are `finding`, `flow`, `milestone`, `round`, `task`, `verdict` and `by`, plus a required `citation`. `round` must be a positive integer, `verdict` one of `gone|reproduces|unsettled` and `by` one of `verifier|orchestrator`. An unknown finding is refused. Each good row is upserted on `(finding, flow, milestone, round, by)` and stamped with today's date as `at`. A changed row is removed and appended again at the end, and a new key is appended. Refused rows are reported with their line and reasons while the rest land, the same way `review verdict` handles a batch. A batch where every row is refused is a validation error, and nothing is written. The verb works on a trial copy first, so a replay that changes nothing is reported as `unchanged` and writes nothing. It never touches `[[findings]]` or `[[verdicts]]`. The store's own `holds_v2_rows` stamps `schema_version = 2`. Output: `{ok, reproductions, refused, unchanged}`.
  - `status` (`review reproduce-status`): a read-only verb. A finding's current reproduction is its last `[[reproductions]]` row. Prints `{ok, expect, findings:[{id, matches, verdict, flow, milestone, round, by}]}`. It exits 0 when every named finding's current reproduction has the `--expect` verdict, and 1 otherwise, including when a finding has no reproduction. An unknown finding id is a `not_found` error, the same as `review anchor`. An `--expect` value outside the vocabulary is a validation error.
- `tomlctl/src/cli/types/review/verdicts.rs`: new `ReviewReproduceArgs`, with the store group, a required `--ndjson` and the write integrity flags. New `ReviewReproduceStatusArgs`, with comma- or space-separated ids, the store group, a required `--expect` and the read integrity flags, which include `--strict-read`.
- `tomlctl/src/cli/types/review/mod.rs`: new `ReviewOp::Reproduce` and `ReviewOp::ReproduceStatus` variants, the second named `reproduce-status`.
- `tomlctl/src/review/dispatch.rs`: routes both variants.
- `tomlctl/src/review/mod.rs`: adds `mod reproduce;`.
- `tomlctl/tests/review_reproduce.rs` (new): six tests:
  - a row is keyed as designed, `at` is stamped, statuses and verdicts are byte-equal before and after, and `schema_version` is 2;
  - a write to an existing key replaces the row and moves it to the end, a new key appends, and an identical replay writes nothing and does not make an older round current again;
  - refusals of an unknown finding, a bad verdict, a missing citation and a bad `by`, with the good row landing;
  - a batch where every row is refused writes nothing;
  - reproduce-status exits 1 when there is no reproduction or when it differs, exits 0 only when the current one matches, and handles several ids;
  - an unknown id or a bad `--expect` is refused, and the read writes nothing.

## Deviations and choices

- The verbs take fields rather than the args structs, as `review anchor` already does. `crate::cli` re-exports the review argument types through an explicit list in `tomlctl/src/cli/mod.rs`, which this task does not hold, so the new structs cannot be named from `src/review/`. Matching on the enum variant in dispatch needs no import.
- The design says a write with an existing key "removes the old row and appends the new one". I applied that to a changed row only. A row identical except for `at` keeps its place and writes nothing. Otherwise a replayed round-1 write would move behind a later round-2 row and become the current reproduction again. This is the same replay rule `Store::upsert_verdict` uses.
- Choices the design left open:
  - `task` is optional, since the design's refusal list does not name it.
  - Any key outside the eight row fields is refused. A caller-supplied `at` is one of them, because the verb stamps it.
  - A partly refused batch lands its good rows, following `review verdict`.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on reproduce.rs, tests/review_reproduce.rs, dispatch.rs and cli/types/review/verdicts.rs: clean after applying it. `cli/types/review/mod.rs` and `review/mod.rs` declare `mod` children, so I left them to the gate.
- Accept check "the reproduction module exists" (`test -f tomlctl/src/review/reproduce.rs`): passes.
- Accept check `cargo nextest run ... -E 'binary(review_reproduce)'`: its `Writes build:` line reads yes, so the gate runs it and I did not.

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the review flag reference does not document `review reproduce` or `review reproduce-status` | agents learn the verbs from that reference
TANGENTIAL: debt | tomlctl/tests/capabilities.rs | the read-only `--help` list does not include `review reproduce-status`, and the write-side list does not include `review reproduce` | the integrity-flag gates do not cover the new verbs
