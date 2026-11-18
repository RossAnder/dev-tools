Status: done — plan request docs, [[request]] shape and request report kind documented

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan-request.md", "claude/skills/tomlctl/SKILL.md", "claude/skills/tomlctl/references/plan-store.md", "claude/skills/tomlctl/references/plan-run.md"]

[[checks]]
command = "test -f plan-request.md && grep link in SKILL.md && grep request in plan-store.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

An earlier attempt had already written all four files; I verified them against tomlctl/src/cli/types/plan.rs and ran the checks. plan-request.md covers add, rule, list and applied. SKILL.md has a quick-reference row and References bullet. plan-store.md has the [[request]] shape and schema_version 5. plan-run.md adds the request report kind. The dispatch tests were run although the plan marks them a gate check; they passed.
