Status: done — the schedule surface is documented in the tomlctl references and the task-store contract

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan.md", "claude/skills/tomlctl/references/plan-engine.md", "claude/skills/tomlctl/references/tasks-store.md", "claude/skills/tomlctl/SKILL.md", "claude/skills/flow-contract-task-store/SKILL.md"]

[[checks]]
command = "grep accept check (plan emit schedule, plan/milestone-order-unqualified, clear-after)"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- plan.md: `--after` on milestone add and update, `--clear-after` on update, a `plan check` flag table with `--milestone`, three new check classes, and the `plan/milestone-width` row now says deferred milestones are skipped as targets. File is 552 lines.
- plan-engine.md: open-run paragraph qualified for `elaborate:` and `cold-read:` runs after approval, a `plan emit schedule` section with the states table and output keys, the `need-not-committed:<task>` dropped reason, and a lane-check sentence on deferred-milestone exclusions.
- tasks-store.md: `after` on `[[milestones]]` and in the snapshot field list.
- SKILL.md: `schedule` in the Quick Reference row and the plan-engine.md description.
- flow-contract-task-store/SKILL.md: `after` in the `[[milestones]]` bullet and example block.

The installed binary predates `plan emit schedule`, so its output was documented from the source and design doc, not from a run.
