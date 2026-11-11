Status: done — `review cover` cuts an over-cap note or rule-out reason, `review add --dry-run` reports without writing, and the cell return marks `same_as` rows

## What changed

- `tomlctl/src/review/cover.rs`: new `cut_to_cap`. Any text over its cap is cut on a character boundary and ends `…(+K)`, where K is the number of characters cut. The stored text, marker included, stays within the cap. Text within the cap is returned byte for byte. `cover` now cuts the note to `fields::cap(Record::Cell, "note")` and no longer refuses it. `read_ruled_out` cuts an over-cap `reason` before `fields::ruled_out_refusals` runs, and counts the reasons it cut. The envelope gains `note_truncated: K` and `reasons_truncated: N`, each present only when something was cut. A replayed seal with the same long note matches its stored cut note, so it reports `unchanged: true`. A unit test covers the marker crossing a digit boundary.
- `tomlctl/src/review/add.rs`: the supersede, pool and `finish_row` logic moved into one `finish` closure. The locked write uses it as before, and `--dry-run` uses it against the store as loaded. The dry run takes no lock, mints nothing and prints `{ok, dry_run: true, run, cell, findings, superseded, refused}`, where `findings` entries have no `id`. `cell_return_entry` takes `Option<&str>` for the id. It adds `same_as: true` when the row names a `same_as`, and builds a `Map` so `id` stays the first key under `preserve_order`. The module header now mentions `--dry-run`.
- `tomlctl/src/review/schema_out.rs`: the cell-return findings item gains an optional `"same_as": {"type": "boolean"}`.
- `tomlctl/src/cli/types/review/cells.rs`: `ReviewAddArgs` gains `--dry-run` (`dry_run: bool`).
- `tomlctl/tests/review_cover.rs`: new `cover_noting` helper and tests `cover_cuts_an_over_cap_note` (which also covers the replay and an exactly-at-cap note stored verbatim) and `cover_cuts_an_over_cap_ruled_out_reason`.
- `tomlctl/tests/review_add.rs`: new `add_with` helper and tests `add_dry_run_writes_nothing` (the store bytes stay unchanged, `refused` is reported and `superseded` is previewed) and `cell_return_marks_same_as` (covers the write path and the dry run).

## Deviations

- `cover.rs` has its own `REASON_CAP = 200`, a copy of the private `RULED_OUT_REASON_CAP` in `fields.rs`. `fields.rs` belongs to a sibling and is outside this task's files, so I could not expose that constant. The alternatives were parsing the refusal text for the cap, which is brittle, or failing the task to widen its files. `cover_cuts_an_over_cap_ruled_out_reason` pins the value: a reason exactly at 200 is stored verbatim. Any over-cap reason also still hits the `fields.rs` refusal, so if the two values drift the result is a refusal, not silent acceptance.
- `--dry-run` also prints `superseded`, the ids the write would withdraw, and exits 0 even when every row would be refused. The write path exits non-zero in that case. Printing the refusals is the point of a dry run, and an error exit would bury them in stderr.

## Handover

None.

## Checks

- Compile check: `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` on the shared tree fails only on files I do not hold: `src/review/recheck.rs` cannot resolve `ReviewRecheckTargetsArgs` (sibling `cli/types/review/read.rs` is mid-edit), plus an unused import in sibling `triage.rs`. To check my own files, I ran clippy on a scratch copy of the crate with a separate target dir: the working tree, minus `recheck.rs`, with `review/mod.rs`, `review/dispatch.rs`, `cli/types/review/read.rs` and `cli/types/review/mod.rs` reset to HEAD. It was clean, with no warnings.
- Narrow tests in the same scratch copy: `binary(review_add) | binary(review_cover)`, `cover_cuts`, `a_cut_text_fits`, `schema_out`. All 41 passed. In that copy, `cli::dispatch::tests` failed only `catalogue_validates`, because the copy sits outside the repo and has no `claude/` tree. The other 25 tests that ran passed.
- `rustfmt --edition 2024 --check` reported diffs in `cover.rs`, `add.rs` and `tests/review_cover.rs`. I applied them with `rustfmt --edition 2024`, and all six files now check clean.
- Accept checks, all with `Writes build: no`: `grep -q 'fn cover_cuts_an_over_cap_note'`, `grep -q 'fn add_dry_run_writes_nothing'` and `grep -q 'fn cell_return_marks_same_as'` all pass.

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | the `review add` flag table lacks `--dry-run`, and the `review cover` and `review schema` text describes neither the `note_truncated`/`reasons_truncated` envelope keys nor the cell-return `same_as` key | agents read the reference, not the source, so the new behaviour is undiscoverable until documented
TANGENTIAL: debt | tomlctl/src/review/fields.rs | `RULED_OUT_REASON_CAP` is private, so `review cover` duplicates the 200 value; making it `pub(crate)` would let cover.rs import it | two copies of one cap can drift
