Status: done — --milestones few|default bound in the carrier and described in the engine.

Changes:
- claude/commands/plan-v2.md: argument-hint gains `[--milestones few|default]`; Bindings table gains the `--milestones` row after `--depth`.
- claude/skills/plan-v2-engine/SKILL.md: Arguments bullet after `--depth`; Re-entry sentence lists `--milestones` among the arguments that are not stored.

Deviations: none.

Checks:
- Both accept checks (grep) pass.
- `cargo test --lib -- cli::dispatch::tests`: 54 pass, 1 fails: `finding_classes::documented_plan_classes_match_the_source`. This concerns plan check classes (sibling files tomlctl/src/plan/check.rs and plan.md in flight), not files I hold; not repaired.

TANGENTIAL: none
