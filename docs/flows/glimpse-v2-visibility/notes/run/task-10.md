Status: done — `plan run report` appends a `[[report]]`, and the run-record leaves' reference moved into plan-run.md to keep plan.md under the line ceiling

## What changed

The first attempt left the code in place and failed only on the 600-line reference ceiling. The replan added `plan-run.md` and `SKILL.md` to the task's files, so this attempt did the split and changed no code.

- `tomlctl/src/plan/run_report.rs` (from the first attempt, unchanged): the `plan run report` leaf. It refuses an undeclared milestone through `load_milestone_store`, parses every row before writing, and appends one `Report` with `at` set to now inside `store::mutate`. It touches only `plan.reports`. Row keys are `kind`, `task`, `text`, `path` and `needs_person`. `kind` is checked against `ReportKind::VOCABULARY`, `task` defaults to 0, `path` defaults to `""` and must be repo-relative with no `..` segment under either separator, and `needs_person` defaults to true. It prints `{ok, milestone, entries}`. A unit test covers the path rule.
- `tomlctl/src/cli/types/plan.rs` (first attempt): `PlanRunOp::Report` and `PlanRunReportArgs`, which take `--milestone`, `--why` and an optional `--ndjson`.
- `tomlctl/src/plan/dispatch.rs` (first attempt): `PlanRunOp::Report(args) => run_report::run(args)`.
- `tomlctl/tests/output_options.rs` (first attempt): a `write` case for `plan run report`, keyed on `entries`.
- `tomlctl/tests/plan_run_records.rs` (first attempt): tests for a report with entries and their defaults, a report with no `--ndjson`, two reports kept in order, and refused rows (an unknown kind, an unknown key, a `..` path) that name line 2 and write nothing. One more checks that only `report` is written.
- `claude/skills/tomlctl/references/plan-run.md` (new): the `plan run notes`, `plan run gate` and `plan run report` sections, moved word for word from plan.md, under a short header and a Contents list. Relative links still resolve, because the file sits in the same directory.
- `claude/skills/tomlctl/references/plan.md`: the three sections are replaced by a `## Run records` stub that links to plan-run.md. The Contents entry and the intro paragraph now point there. The file went from 617 lines to 542.
- `claude/skills/tomlctl/SKILL.md`: a routing line for `references/plan-run.md` after the plan-engine.md line. The plan.md line now leaves out the run-record leaves.

## Deviations

- `text` is required and must not be blank. The detail gives defaults only for `task` and `path`, and an entry without text tells a person nothing.
- An empty `--ndjson` source records a report with no entries, where `plan run gate` refuses an empty batch. design/engine.md says an invocation with nothing to report still records an empty report.
- The `plan run notes` and `plan run gate` sections moved to plan-run.md along with the new report section. Moving only the new section would have split one family of leaves across two files. Nothing links to `plan.md#plan-run-notes`, `#plan-run-gate` or `#plan-run-report` (checked with rg over the repo).

## Checks

- `cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests`: 55 passed, 0 failed. This includes `skill_references_under_line_ceiling` and `skill_markdown_links_resolve`.
- `rustfmt --edition 2024 --check` on run_report.rs and plan_run_records.rs: clean.
- Clippy: not rerun, since this attempt changed no Rust. The first attempt's `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets` was clean, and the lib compiled again for the dispatch tests above.
- Accept checks: `PlanRunOp::Report` in dispatch.rs passes, `plan run report` in plan.md passes (from the Contents entry and the stub), and `"report"` in plan_run_records.rs passes.
- `binary(plan_run_records)` nextest: the gate runs this one, so I did not run it.

## Handover

None.

TANGENTIAL: none
