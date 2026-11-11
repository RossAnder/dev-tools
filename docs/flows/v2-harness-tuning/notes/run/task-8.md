Status: done — findings carry `fix_files`, and `review schedule` widens the minted row's files by it, noting absent paths `(new)`.

## What changed

- `tomlctl/src/review/fields.rs`: catalogued `fix_files` after `instances` as a `StrList` on defect and cleanup rows, at most 8 items, checked as paths (`PathForm::Path`), optional and caller-supplied. New visibility row `FIX_FILES = [R, R, N, R, R, R, N, R]`: finder, verifier and implementer read it, and so do orchestrator, audit and report. Checker and backlog do not. Added the matching row to the projection matrix test.
- `tomlctl/src/review/schedule.rs`: a `fix_files(f)` reader. `edited()` chains it after `f.edited_paths()`, so `fold` groups findings by the widened set, `Compose::row` writes that set to `files`, and the lite rule (`files.len() <= 2`) counts it. `Compose` gains `root`, and `Compose::row` writes `file_notes = { <path> = "(new)" }` for each `fix_files` path absent under the repo root. The key is omitted when no path is new. `Finding::edited_paths` (store.rs) and `fields::edited_paths` are untouched, so the test-removal hold-back and the cleanup route behave as before. A store with no `fix_files` yields the same rows as before.
- `tomlctl/tests/review_schedule.rs`: helper `file_notes_of`, plus tests `fix_files_widen_the_minted_row` (a defect whose `fix_files` names another finding's file folds both into one row holding both files, with no file notes) and `fix_files_new_path_is_noted_new` (an existing path gets no note, an absent test path gets `(new)`, and the three-file row runs deep).

## Deviations

- The task fixed only finder, verifier and implementer visibility, so I set the other five roles myself. Orchestrator, audit and report read the field, as they do the other fix-routing fields (`target`, `tier`). Checker and backlog do not. The alternative was hiding it from everyone but those three, which would keep it out of `review show --for orchestrator` and the report even though it decides a scheduled task's files.

## Checks

- `rustfmt --edition 2024 --check` on all three files: clean, after two layout fixes in the test file.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `cargo nextest run -E 'test(fix_files) | test(projection_table_matches_matrix) | binary(review_schedule)'`: 23 passed.
- `cargo nextest run -E 'test(schema) | test(/review::fields/)'`: 153 passed.
- Accept `grep -q 'fn fix_files_widen_the_minted_row' tomlctl/tests/review_schedule.rs`: passes.
- Accept `grep -q 'fn fix_files_new_path_is_noted_new' tomlctl/tests/review_schedule.rs`: passes.

## Handover

None.

TANGENTIAL: debt | claude/skills/review-v2-method/references/finding-kinds.md | the finder-facing field docs do not mention `fix_files` yet | finders will not supply the field until the method skill names it
