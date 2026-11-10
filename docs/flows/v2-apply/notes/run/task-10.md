Status: done — tomlctl is 0.23.0, announces the six apply features, and review.md documents every apply verb, status and field.

## What changed

- `tomlctl/Cargo.toml`: version 0.22.0 -> 0.23.0.
- `tomlctl/Cargo.lock`, `glimpse/Cargo.lock`: refreshed with `cargo tree --manifest-path <crate>/Cargo.toml >/dev/null`; each changes only the tomlctl version line.
- `tomlctl/src/cli/types/mod.rs`: `FEATURES` gains `tasks_finding_links`, `review_schedule`, `review_reconcile`, `review_anchor`, `review_reproduce`, `review_stats_apply`, under one group comment.
- `tomlctl/tests/capabilities.rs`: the exhaustive feature list gains the same six; the version pin and its message move to 0.23.0; the read-only help list gains `review anchor` and `review reproduce-status`, the write help list `review reproduce`, `review schedule` and `review reconcile` (each verified against its clap type: `ReadIntegrityArgs` vs `WriteIntegrityArgs`).
- `claude/skills/tomlctl/references/review.md`: new section "Apply through a v2 flow" with a flag table and output shape for `review schedule`, `review reconcile`, `review anchor`, `review reproduce` and `review reproduce-status`; `review stats --apply` flag row, `apply` output bullet and example; `[[reproductions]]` in the store-shape block with its upsert key; a schema-version paragraph (2 only while a reproduction or scheduled finding is held); `scheduled` in the status list, with the `apply_flow`/`apply_task` link fields, triage's never-moves list and the resolve target table; Shared behaviour bullets (integrity bundles, missing store, replays, partial batches) updated for the new verbs. Every flag and output key was read from the clap types in `cli/types/review/{export,read,verdicts}.rs` and the verb modules (`review/{schedule,reconcile,anchor,reproduce,stats}.rs`), not from the design file.
- `claude/skills/tomlctl/SKILL.md`: the review row of the Quick Reference lists `reproduce`, `reproduce-status`, `anchor`, `schedule` and `reconcile`; the capability table gains one row for the six features, linking the new review.md section.
- `tomlctl/README.md`: version and features array in the capabilities example, one feature-table row per new feature, and command-summary lines for the five verbs plus `--apply` on the stats line.

## Deviations

- `tomlctl/src/capabilities.rs` is listed as an edit but is unchanged. The only candidate addition was a `SCOPED_ENUM_VALUES` entry for `review reproduce-status --expect`, but its drift test (`scoped_enum_values_match_their_validators`) must name the validator's vocabulary (`ReproduceVerdict::VALUES`), which lives in the private `review::fields` module; exposing it needs `tomlctl/src/review/mod.rs`, outside this task's files. No existing review verb's post-parse vocabulary (`verdict --verdict`, `triage --expect-status`) is listed there either, so leaving it out matches the code. No new mutex group needs the const (`schedule`'s ids/`--all` conflict is clap-native).

## Checks

- `rustfmt --edition 2024 --check tomlctl/tests/capabilities.rs`: clean. `cli/types/mod.rs` has `mod` children, so its format check is left to the gate (the edit is six string literals and a comment in an existing array).
- Compile check skipped: the Rust edits are string-literal additions to a const array and a test list, reasoned to compile.
- Accept, writes build no: version 0.23.0 — pass; `"review_schedule"` in the capability list — pass; glimpse lock carries 0.23.0 — pass; review.md documents `review schedule` — pass.
- Accept, writes build yes (`cargo nextest run ... binary(capabilities) | test(command_lint) | test(flag_table_lint)`): the gate's, not run.

TANGENTIAL: debt | tomlctl/src/capabilities.rs | `review reproduce-status --expect`, `review verdict --verdict` and other post-parse review vocabularies are absent from SCOPED_ENUM_VALUES because `review::fields` is private | an agent reading `capabilities .commands` sees these as free strings; re-exporting the vocabularies would let the table list them under its drift test
