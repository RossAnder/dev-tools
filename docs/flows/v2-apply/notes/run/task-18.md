Status: done — list, report and selection read blocked findings through the catalogue's `blocked_on` helper; `--fix-shape` and `FIX_SHAPES` are gone.

## What changed

- `tomlctl/src/cli/types/review/read.rs`: `ReviewListArgs.fix_shape` / `--fix-shape` replaced by `blocked_on` / `--blocked-on <value>|any|none` (no alias).
- `tomlctl/src/review/query.rs`: `review list` validates `--blocked-on` against `BlockedOn::VALUES` plus `any` and `none`, and filters through `fields::blocked_on`, so a legacy `fix_shape = "needs-plan"` row lists under `design` and `any`. `none` keeps every finding the helper calls fixable, advisory included.
- `tomlctl/src/review/select.rs`: `--all` passes over any finding the helper calls blocked; `refusal` refuses one with `blocked on <value>`, and a legacy row's reason adds ` (a stored fix_shape = needs-plan)`.
- `tomlctl/src/review/digest.rs`: the report's "Needs a plan" section is now "## Blocked", with one `### On <value>` subsection per `blocked_on` value in catalogue order. It holds live blocked defects and cleanups, and each entry shows its Missing, Option, Metric, Command and Contract lines. Blocked findings stay out of the severity groups, as before, and also out of the Cleanup section.
- `tomlctl/src/review/fields.rs`: removed `FIX_SHAPES`, and also the `#[allow(dead_code)]` on `blocked_on`, since the helper now has readers.
- `tomlctl/src/cli/types/review/export.rs`: the `--all` help of `export-v1` and of `schedule` now reads "less blocked findings".
- `tomlctl/src/review/export.rs`: unchanged. It reads no `fix_shape` and gets the new behaviour through `select`.
- Tests:
  - `review_query.rs`: replaced the fix-shape filter test with `list_filters_by_blocked_on_with_any_and_none` and `list_reads_a_legacy_needs_plan_row_as_blocked_on_design`.
  - `review_digest.rs`: added `report_groups_blocked_findings_by_blocked_on_outside_the_severity_groups`.
  - `review_export.rs`: renamed the refusal test to `export_refuses_advisory_and_a_legacy_needs_plan_row_as_blocked`, and added `export_refuses_a_blocked_finding_naming_its_block_and_all_passes_it_over`.

## Deviations

- Detail said deleting `FIX_SHAPES` is the only edit in fields.rs. I also removed the now-stale `#[allow(dead_code)]` on `blocked_on`. Keeping it would leave a suppression with no use; clippy is clean without it.
- Detail named only `export-v1 --all`'s help text. I reworded `schedule --all`'s help in the same file too, because it shares the same selection and the same stale "needs-plan" wording.
- A legacy row's refusal reason carries a ` (a stored fix_shape = needs-plan)` suffix. This tells the reader why a row with no `blocked_on` is blocked. It also keeps `review_schedule`'s existing `needs-plan` needle passing, since that test is not this task's file.
- `export.rs` is listed as edit but needed no change.
- I ran the four affected test binaries once to confirm. A build-writing check is normally left to the gate.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on every edited file except fields.rs, which has a `mod tests` child: clean after formatting the two test files.
- Accept (no build): `blocked_on` in read.rs passes; `const FIX_SHAPES` absent from fields.rs passes; no `"fix_shape") [!=]= "needs-plan"` in select/export/digest/query passes.
- `cargo nextest run -E 'binary(review_query) | binary(review_digest) | binary(review_export) | binary(review_schedule)'`: 59 passed. This covers the gate's guard, `test(blocked)` and `test(legacy_needs_plan)` filters.

## Handover

None.

## Notes for the gate

- `claude/skills/review-v2-engine/SKILL.md` and `claude/skills/tomlctl/references/review.md` still name `--fix-shape`. The capability `review_list_fix_shape` is still advertised in `tomlctl/src/cli/types/mod.rs`. Tasks 23 and 26 own those files. Until they land, the carrier↔flag `command_lint` gate may flag `--fix-shape`.
- `tomlctl/src/review/schedule.rs` still tests `fix_shape == "needs-plan"` directly in its `--all` refused loop. Task 21 owns that file, and contract C3's check covers it.

TANGENTIAL: none
