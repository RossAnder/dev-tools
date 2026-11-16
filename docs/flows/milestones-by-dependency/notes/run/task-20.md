Status: done — Design, 4a and 4c of write-and-prove.md now cover milestone order with --after.

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/write-and-prove.md"]

[[checks]]
command = "grep accept check for '--after <id>', 'plan/milestone-order-unqualified', 'elaborate.md'"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/plan-v2-engine/references/write-and-prove.md` (407 lines):

- Added a "Milestone order" paragraph after "How many" in Design. It covers `--after <id>` on `milestone add` and the placement rule for deferred milestones.
- 4a now describes the `plan/milestone-order-unqualified` warning and when to add `--after`.
- 4c "Which milestones" now says a deferred milestone is cold-read when elaborated, by references/elaborate.md.
