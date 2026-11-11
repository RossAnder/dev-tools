Status: done — reproduce-status selects by --flow/--milestone, and schedule mints one gate-owned milestone check per fix row

## What changed

- `tomlctl/src/cli/types/review/verdicts.rs`: `ReviewReproduceStatusArgs` gains `--flow <slug>` and `--milestone <id>`. The positional ids are now `required_unless_present = "flow"`; `--flow` conflicts with the ids and requires `--milestone`, and `--milestone` requires `--flow`. A call naming ids alone behaves exactly as before.
- `tomlctl/src/review/dispatch.rs`: builds a `reproduce::Select` (the ids, or a milestone selection) and passes it to `reproduce::status`.
- `tomlctl/src/review/reproduce.rs`: a new `Select` enum, and `status` takes it in place of the id slice. The new `milestone_fixes` reads the flow's `tasks.toml`, which must exist or the call fails `not_found`. It then collects, in row order, the `fixes` of every `[[finding_links]]` entry for this review store (`store == target.rel()`) whose row's `checkpoint` is the milestone. The ids are deduplicated. An empty selection prints `{"ok": false, "findings": []}` and exits 1. The id form keeps its "no finding ids given" error and its `not_found` on an unknown id.
- `tomlctl/src/review/schedule.rs`: `Compose::accept` now gives every finding that is not a `remove` cleanup the same accept entry, `tomlctl review reproduce-status --flow <flow> --milestone <checkpoint> <store-flag> --expect gone`. That entry also carries `behaviour = "every finding this milestone fixes no longer reproduces"`, `kind = "forward"`, `run_by = "gate"` and `writes_build = false`. The `remove` cleanup arm is unchanged. `Compose::row` drops a row's duplicate accept entries, so a row keeps one copy of the milestone check. Rows already in a task store are not touched.
- `tomlctl/tests/review_reproduce.rs`: added `reproduce_status_selects_by_milestone`, `reproduce_status_milestone_excludes_positional_ids` and `reproduce_status_milestone_expect_fails_on_no_findings`, with a seeded flow fixture that includes a link to another store, to show the store filter works.
- `tomlctl/tests/review_schedule.rs`: added `scheduled_fix_row_carries_one_gate_owned_check`. Updated `a_cleanup_and_a_defect_become_designed_rows` to expect the milestone check, `run_by = "gate"` and `writes_build = false`. Updated `findings_sharing_a_file_fold_into_one_task` to expect one accept entry where it expected two.

## Deviations

- The design says an empty milestone selection exits 0 "or 1 under `--expect`". `--expect` is still required in both forms: the constraint keeps the positional form unchanged, and making the flag optional for only one form would add an output with no `expect`. So an empty milestone selection always exits 1, which is what the required means pins. The exit-0 branch cannot be reached.
- A missing task store for `--flow` fails `kind=not_found` instead of printing an empty selection. The design does not cover this case. A flow with no task store has no links to lose, so a hard error tells the caller more than an empty pass would.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean at the time it ran.
- `cargo nextest run ... -E 'binary(review_reproduce) | binary(review_schedule)'`: 33 of 33 passed, including all four new tests. This ran before a whitespace-only rustfmt fix to the two test files.
- `rustfmt --edition 2024 --check` on all six edited files: clean after that fix.
- Accept check `grep -q 'fn reproduce_status_selects_by_milestone' tomlctl/tests/review_reproduce.rs`: passes.
- Accept check `grep -q 'fn scheduled_fix_row_carries_one_gate_owned_check' tomlctl/tests/review_schedule.rs`: passes.
- `cargo test --lib -- cli::dispatch::tests`: could not compile. The only error is E0425 `MilestoneSpec` not found in `tomlctl/src/plan/probes.rs`, a sibling's in-flight edit. Not repaired and not rerun.

## Tangential

TANGENTIAL: debt | claude/skills/tomlctl/references/review-apply.md | the `reproduce-status` flag table and the `schedule` accept description still name only the per-finding form; the `--flow`/`--milestone` flags and the gate-owned milestone check need documenting | a later docs task for this milestone's review surface should pick it up, or readers will mint per-finding checks by hand
TANGENTIAL: debt | claude/skills/implement-v2-engine/references/findings.md | it still describes a fix row's accept check as `review reproduce-status <F> ... --expect gone` with no `writes_build` | the engine's gate-list text no longer matches what schedule mints
