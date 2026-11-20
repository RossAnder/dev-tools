Status: done — pre-flights and CLAUDE.md updated

```toml run-note
files_touched = ["claude/commands/implement-v2.md", "claude/commands/plan-v2.md", "CLAUDE.md"]

[[checks]]
command = "grep -q 'gate_accept' claude/commands/implement-v2.md && grep -q 'review_plan_digest' claude/commands/plan-v2.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed
- implement-v2.md: eight new feature names added to the pre-flight list.
- plan-v2.md: four new feature names added.
- CLAUDE.md: linked agent/SKILL.md reload behaviour, Deploy list of features, and worker build licence sentence.
