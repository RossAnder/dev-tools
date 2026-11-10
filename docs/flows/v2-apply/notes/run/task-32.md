Status: done — the apply verbs moved from review.md into the new review-apply.md, linked from review.md and the tomlctl skill

## What changed

- `claude/skills/tomlctl/references/review-apply.md` (new, 114 lines): title, an intro paragraph in the shape of `review-runs.md`'s (which file owns the shared behaviour and store shape, and whose call the workflow is), a `## Contents` list, then the whole `## Apply through a v2 flow` section moved verbatim, with its intro paragraph and the `schedule`, `reconcile`, `anchor`, `reproduce` and `reproduce-status` headings, examples and flag tables. A `diff` against the original lines showed one changed line: the `[Store shape](#store-shape)` link in `reproduce` now reads `review.md#store-shape`, because the anchor is no longer in the same file.
- `claude/skills/tomlctl/references/review.md` (662 → 564 lines): the section body is replaced by a one-line pointer under the kept `## Apply through a v2 flow` heading, in the same form as the existing pointer for the `run` verbs. The Contents entry now names review-apply.md. The Shared behaviour bullets that name these verbs (integrity bundles, missing store, replays, partial batches), the `[[reproductions]]` store shape and `stats --apply` stay in review.md, which owns them.
- `claude/skills/tomlctl/SKILL.md`: added review-apply.md to the References list after review-runs.md. The capability-table row for `tasks_finding_links` / `review_schedule` / … / `review_stats_apply` now links `references/review-apply.md` instead of `references/review.md#apply-through-a-v2-flow`.

## Deviations

None from the suggested means. One judgement call: I kept the `## Apply through a v2 flow` heading in both files. In review.md it holds the pointer, so the existing `#apply-through-a-v2-flow` anchor still resolves. In review-apply.md it carries the moved intro paragraph above the verb headings, the way `review-runs.md` uses `## Verbs`.

## Checks

- Compile check: not run. The change is markdown only and touches no crate source.
- Accept: `test "$(wc -l < claude/skills/tomlctl/references/review.md)" -lt 600`: passes (564).
- Accept: `grep -q 'review schedule' …/review-apply.md && grep -q 'references/review-apply.md' …/SKILL.md`: passes.
- Accept: `cargo nextest run … skill_references_under_line_ceiling | command_lint | flag_table_lint`: Writes build: yes, so this is the gate's to run. I did not run it. `command_lint_scan_set` already scans every reference file, the flag tables keep their `### \`tomlctl review <verb>\`` headings and row shape, and review-apply.md is 114 lines.
- `review.md#apply` search in claude/: the only hit was SKILL.md's capability row, which is now re-pointed. No `review.md#tomlctl-review-{schedule,reconcile,anchor,reproduce}` links exist.

## Handover

None.

## Tangential

TANGENTIAL: debt | claude/skills/apply-v2-engine/SKILL.md | Its schedule-rule step (line 83) sends the reader to the tomlctl skill's `references/review.md`, section `tomlctl review schedule`, which now lives in `references/review-apply.md` | Prose pointer, not a link, so no lint catches it, and an orchestrator following it finds only a pointer one hop short; outside this task's files, so left unedited
