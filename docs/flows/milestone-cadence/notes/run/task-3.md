Status: done — How many rule and plan/milestone-width guidance added to write-and-prove.md

Changed claude/skills/plan-v2-engine/references/write-and-prove.md: a `**How many.**` paragraph after the milestone add block in section 1, and a plan/milestone-width sentence appended after the plan/file-only-chain sentence in section 4a. No existing sentence changed; no fenced command added.

Checks: the three grep accept checks pass. `cargo test --lib -- cli::dispatch::tests` has one failure, `finding_classes::documented_plan_classes_match_the_source`, because the doc now names plan/milestone-width, which tomlctl/src/plan/check.rs (a sibling task's file) does not emit yet. Expected to clear once that task lands.

TANGENTIAL: none
