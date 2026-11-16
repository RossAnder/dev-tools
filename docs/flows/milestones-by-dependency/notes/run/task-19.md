Status: done — --elaborate wired into plan-v2 skill, carrier and re-entry rules

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/SKILL.md", "claude/skills/plan-v2-engine/references/open-and-challenge.md", "claude/commands/plan-v2.md"]

[[checks]]
command = "both accept greps"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- SKILL.md: `--elaborate <id>` argument, elaborate.md in the references list, and an open-run rule bullet leaving `implement:`/`reproduce:` runs alone and asking about another session's `elaborate:`/`cold-read:` runs.
- open-and-challenge.md section 6: same exception before "Otherwise abandon".
- plan-v2.md: argument-hint, Bindings row, `milestone_schedule` in pre-flight step 2.

references/elaborate.md exists (untracked, from a sibling task), so the link resolves.
