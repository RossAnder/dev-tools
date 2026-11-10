Status: done — `tomlctl review stats --apply` counts natively applied findings and reproductions from the review store alone

## What changed

- `tomlctl/src/cli/types/review/read.rs`: `ReviewStatsArgs` gains the `--apply` bool flag. (The file already carried an earlier task's uncommitted `ReviewAnchorArgs` edit; it was left as found.)
- `tomlctl/src/review/stats.rs`: `--apply` counts as a section, so it alone prints no `rows`, and it emits `apply`:
  - `apply.outcomes`: one row per (lens, kind) with `scheduled` (status `scheduled`), `resolved` (status `resolved` with a non-empty `apply_flow`), `reopened` (status `open` with a `reopen_reason` opening `apply: `), and `reproductions` (each finding's last reproduction row, by verdict, every verdict key present).
  - `apply.reproductions`: `rows` (every reproduction row by verdict) and `findings` (each finding's last row by verdict).
  - A reproduction of a finding the view drops (superseded, or another family under `--family`) is not counted. `--outcomes` and the other sections are untouched. The module header names `--apply` as the native counterpart.
- `tomlctl/tests/review_stats.rs`: added `stats_apply_counts_outcomes_and_reproductions_from_the_store` and `stats_apply_reads_no_v1_ledger_and_leaves_outcomes_alone`, the second checking that `--outcomes` output is the same with `--apply` beside it.
- `claude/skills/review-v2-engine/references/measurement.md`: added a signals row for `review stats --apply`, the `--all --apply` command, and a paragraph saying `--apply` is the native counterpart and `--outcomes` still reads only v1 exports.

## Deviations

- The `apply: ` reopen prefix is redeclared in stats.rs as `APPLY_REOPEN_PREFIX`. The source constant `REOPEN_PREFIX` in `tomlctl/src/review/reconcile.rs` is private, and that file is outside this task. Making the reconcile constant `pub(super)` and importing it would remove the duplicate.
- Scope: the design section `design/verbs.md#review-stats-apply` also lists per-lens/per-agent blocked counts and settle outcomes by tier. The task's action and accept checks cover only apply outcomes and reproductions, so only those were built.
- Output shape: the design does not fix the JSON shape. I chose `apply: {outcomes: [...], reproductions: {rows, findings}}` to match how `--outcomes` groups by (lens, kind). Counting both every row and each finding's last row makes the result readable as both the number of checks run and where findings stand now.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on stats.rs, read.rs and tests/review_stats.rs: clean.
- Accept `grep -q -- '--apply' .../measurement.md`: passes.
- Accept `cargo nextest ... -E 'binary(review_stats) & test(apply)'`: has `Writes build: yes`, so the gate runs it. I did not run it.

## Handover

None.

TANGENTIAL: debt | claude/skills/tomlctl/references/review.md | The `review stats` flag reference and its output key list do not mention `--apply` or the `apply` key, and that file was outside this task | Agents reading the tomlctl skill will not find the new section
TANGENTIAL: debt | tomlctl/src/review/reconcile.rs | `REOPEN_PREFIX` is private, so stats.rs keeps its own copy of `apply: ` | The two copies can drift, and stats would then stop counting reopens
