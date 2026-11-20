Status: done — plan drift section moved to plan-engine.md; plan.md is 571 lines

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan.md", "claude/skills/tomlctl/references/plan-engine.md"]

[[checks]]
command = "test $(wc -l < claude/skills/tomlctl/references/plan.md) -le 600 && grep -q '^## `plan drift`' claude/skills/tomlctl/references/plan-engine.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- plan-engine.md: appended the `plan drift` section unchanged, added it to Contents, and named it in the intro.
- plan.md: replaced the section body with a two-line pointer under the same heading, repointed the Contents entry, and named `plan drift` in the intro's plan-engine.md sentence. CRLF endings kept.
