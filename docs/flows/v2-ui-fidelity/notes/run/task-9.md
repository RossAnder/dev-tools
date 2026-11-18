Status: done — documented decision class, the silent one-way refusal, milestone ui flags, both schema bumps and five new check classes

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan.md", "claude/skills/tomlctl/references/plan-store.md", "claude/skills/review-v2-method/references/plan-records.md"]

[[checks]]
command = "grep -q 'ui-accept-structural' plan.md && grep -q -- '--ui-control' plan.md && grep -q 'compromise' plan-store.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "fail"
```

## What changed

- plan.md: `class` on `plan decide`, the `plan gate` assumptions grouping and `class` field, the approve refusal over a silent one-way decision, `--ui-state`, `--ui-control` and `--clear-ui` on milestone add and update, the tasks.toml version 3 stamp, and check-class rows for the five new classes plus the look exemption on `plan/accept-no-check`.
- plan-store.md: `class` in the sample and the decision key table, the stamp rule now 2, 3 or 4.
- plan-records.md: `class` added to a decision's keys.

## Dispatch tests

Three of the `cli::dispatch::tests` fail, none from these files. `documented_plan_classes_match_the_source` and `the_plan_scan_reads_both_construction_sites_and_no_tasks_class` fail because the class scanner reads `class` in `tomlctl/src/plan/schema.rs` as a finding class (source-side, not mine). `skill_markdown_links_resolve` fails on a missing `claude/skills/plan-v2-engine/references/ui-layout.md`, which a sibling task is writing. The scanner failure needs a fix in the source or the test, and `tomlctl/src/plan/schema.rs` is outside this task's files.
