Status: done — the run-note block accepts a [[sources]] table, drops a bad entry alone and carries the sources onto the attempt

```toml run-note
files_touched = ["tomlctl/src/plan/run_note.rs", "tomlctl/src/plan/run_notes.rs"]

[[deviations]]
what = "fetched_at accepts a TOML date or a string that parses as one (a datetime with a date part is also accepted); a time-only value is dropped as not a date. A `sources` value that is not an array at all makes the block malformed, the same as any other top-level type error; only a bad entry inside the array is dropped alone"
why = "The spec says only 'a date, required'. The repo's other date readers (as_date in tasks, review and agents schema) also accept a string that parses, so a worker who quotes the date is not penalised. Rejecting quoted dates would be stricter, but a worker would then lose the entry for a formatting slip"
means = "design/records.md#run-note-sources"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/run_note.rs tomlctl/src/plan/run_notes.rs"
result = "pass"

[[checks]]
command = "grep -q '\"sources\"' tomlctl/src/plan/run_note.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "fail"
```

## What changed

- `tomlctl/src/plan/run_note.rs`: `RunNote` and `BlockFields` gain `sources: Vec<Source>`. `read_block` now reads a top-level `sources` key as an array. Each entry goes through a new `source()` reader, and an entry that fails is pushed to `dropped` as `the source #<n> was dropped: <why>`, so it shows in `reason` the same way an unknown check result does. An entry is dropped when it is not a table, holds an unknown key or a non-string field, has an empty `claim`, has neither a non-empty `url` nor a non-empty `query`, or has a `fetched_at` that is missing or not a date. `how_verified` and `version` are optional. Every other unknown top-level key still makes the block malformed. The module doc is updated to match. New unit tests: `sources_parse_and_a_bad_entry_is_dropped_alone` (good entries kept and every field read, with entries lacking a claim, a fetched_at, or a url and query each dropped and named), `a_source_with_an_unknown_key_or_a_non_date_is_dropped_alone`, and `sources_that_are_not_an_array_are_malformed`.
- `tomlctl/src/plan/run_notes.rs`: `read_attempt` copies `note.sources` onto `Attempt.sources`. An earlier edit had already initialised that field (the `Source` type and its store reader and writer already exist in `schema.rs`).

## Checks

The compile check fails, but only in files this task does not hold. The errors are E0027 in approve.rs and emit.rs, where patterns do not mention the new `waive_review_delta` and `after_round` fields, and E0061 in a test in records.rs, where `upsert_handover` needs a fourth argument. All three files are being edited by other workers right now. No diagnostic names run_note.rs or run_notes.rs. Because the lib-test target does not compile, the new unit tests could not run here. That check is the gate's in any case (`cargo nextest run ... -E 'test(/plan::run_note::/)'`).
