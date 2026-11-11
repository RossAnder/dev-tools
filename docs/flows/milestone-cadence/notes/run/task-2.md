Status: done — plan/milestone-width documented in both tomlctl references

Changes:
- claude/skills/tomlctl/references/plan.md: added the plan/milestone-width row under plan/file-only-chain in the class table.
- claude/skills/tomlctl/references/tasks.md: the severity row now names plan/file-only-chain and plan/milestone-width.

Checks:
- Both accept greps pass.
- `cargo test --lib -- cli::dispatch::tests`: 54 pass, 1 fails: finding_classes::documented_plan_classes_match_the_source. It fails because tomlctl/src/plan/check.rs (a sibling task's file) does not yet emit plan/milestone-width. It should clear once that task lands; the gate should confirm.

TANGENTIAL: none
