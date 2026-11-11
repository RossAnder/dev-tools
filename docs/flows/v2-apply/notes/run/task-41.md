Status: done — twelve findings fixed in tomlctl/src/review: four defects with unit tests, plus the shared helpers the cleanups asked for

## What changed

- `store.rs`: added `Finding::text(key) -> Option<&str>`, `Finding::clear_apply_link()`, `Finding::edited_paths()`, and `Target::rel()`, which gives the repo-relative store path. `REPRODUCTION_CORE` is now `pub(super)`. The test helper `companion` now delegates to `Finding::text`.
- `fields.rs`: added `APPLY_LINK_FIELDS` and `check_cap(verb, what, record, name, text)`. `blocked_on` and `clear_block` now read through `Finding::text`. The `Ty::StrList` arm of `check_value` now refuses any entry that is blank after trimming ("entry {i} is blank"), before the `min_items` and `max_items` counts. The companion-disjointness test now iterates over `APPLY_LINK_FIELDS`. New unit test: `a_blank_option_does_not_count_toward_the_minimum`.
- `verdict.rs`: `settle_refute(finding, by, verdict)` now checks `by == "orchestrator"` itself, and both callers (`apply_one`, `decide`) pass `by`. In `apply_one`, the status step is now gated on `!resolve::awaits_resolution(finding)` instead of `same_as.is_empty()`.
- `settle.rs`: removed the private `text` helper. The cap check now goes through `fields::check_cap`. `settle` now calls `clear_apply_link`. `recorded` now detects an unlinked finding by iterating `APPLY_LINK_FIELDS`, and detects a refute with `verdict::settle_refute`. New unit test: `a_settle_refute_decides_a_regression_row`. It seeds a scheduled, evidence-blocked row with `same_as` set and `outcome = regression`, then asserts that the refute leaves it refuted, unlinked and replay-stable.
- `reconcile.rs`: removed `link_text`. `store_rel` now comes from `target.rel()`. `reopen` now uses `clear_apply_link`. The row found by the `apply_task` ref is accepted only through a new `is_own` check: `apply_task_id` absent, or matching the row id, or a `[[finding_links]]` entry of this store ties the row to the finding. Otherwise reconcile falls back to `retitled()`. New unit test: `a_freed_ref_reused_by_another_findings_task_is_not_followed`, which is the finding's own scenario.
- `export.rs`: the v1 description now goes through a new `with_answers`. It appends `Fix sketch:` and `Decision:` lines from the finding, trimmed and only when non-empty. `description()` is unchanged, so `review schedule` does not print the sketch twice. `store_rel` now comes from `target.rel()`. Calls to `select::text` were replaced with `Finding::text`. New unit test: `an_unblocked_findings_answers_reach_the_description`.
- `reproduce.rs`: removed `KEYS`. `parse_row` now accepts only keys in `REPRODUCTION_CORE` other than `at`, and the refusal lists those same keys. An optional key given as a blank string is now read as empty, so a blank `task` no longer gets "is required". New unit tests: `a_blank_optional_task_is_recorded_as_none` and `at_is_not_a_key_a_batch_supplies`.
- `select.rs`: removed `select::text`, and its callers now use `Finding::text`. `is_test_removal` now ends with `f.edited_paths().any(..)`.
- `schedule.rs`: `edited` now uses `Finding::edited_paths`, keeping its trim and de-duplication. `store_rel` now comes from `target.rel()`. Calls to `select::text` were replaced with `Finding::text`.
- `anchor.rs`: removed `rest_str`. Callers use `finding.text(k).filter(non-empty)`.
- `apply.rs`, `digest.rs`: removed the private `text` and `rest_str`. Callers use `Finding::text`.
- `stats.rs`: `apply_state`, `block_of`, `settle_outcome` and `BlockTally::add` read through `Finding::text`. The Refuted arm of `settle_outcome` now uses `verdict::settle_refute`.
- `triage.rs`: `Mode::recorded` reads through `Finding::text`. The cap check in `read_mode` now goes through `fields::check_cap`.
- `cover.rs`: the note cap check now goes through `fields::check_cap`. The message text is unchanged.

## Deviations

- The fixes for F87, F92 and F97 named tests in `tomlctl/tests/review_export.rs`, `review_settle.rs` and `review_reconcile.rs`. Those files are not among this task's files, so I wrote equivalent unit tests in the modules themselves (`export.rs`, `settle.rs`, `reconcile.rs`). The F87 test exercises `with_answers` directly rather than a full `export-v1` run.
- F117: `select::text` returned `&str`, defaulting to empty. I deleted it rather than keeping a wrapper, and its callers in export and schedule now use `f.text(k).unwrap_or_default()`, or compare against `Some(..)`. Keeping it as a one-line delegate would have left a duplicate helper.
- F118 listed `schedule.rs` as an instance. `mark_scheduled` there inserts the three link fields with their values rather than spelling out a clear, so I left it as is. No helper fits an insert of three distinct values.
- F96: the blank-entry refusal applies to every `StrList` field validated through `check_value`, as the fix prescribes. That also covers `evidence`, `instances`, `depends_on`, and cell `slice`, `concerns` and `facets`. A blank entry in any of them is now refused at write time; before, it was accepted.
- F92: as part of F122, `decide` now matches `_ if settle_refute(finding, by, verdict)` rather than `("orchestrator", _) if ...`. The two are equivalent, since `settle_refute` now checks `by` itself.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean, no warnings.
- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- review::`: 114 passed, including the 6 new tests.
- `rustfmt --edition 2024 --check` on each edited file: export, fields and reproduce needed formatting, which I applied; the rest were clean.
- The accept checks (`tomlctl review reproduce-status F<n> --slug v2-apply --expect gone`) all read `Writes build: yes`, so they are the gate's to run, not mine.
- The integration suites `review_settle`, `review_reconcile`, `review_export`, `review_reproduce` and `review_triage` were not run; the gate runs them. One risk to watch: an integration test that expects the old `reproduce` unknown-key wording. That wording is unchanged, and the key list is in the same order.

TANGENTIAL: debt | tomlctl/src/review/triage.rs | Several more `rest.get(k).and_then(as_str)` reads remain (tier, severity, converted_to, target) in triage, and in stats, apply and resolve | The `Finding::text` accessor now exists; converting them would finish the consolidation
