Status: done — platform rung and Context7-first line added

```toml run-note
files_touched = ["claude/skills/research-methods/SKILL.md", "claude/agents/plan-investigator.md"]

[[checks]]
command = "grep -q 'llms.txt' claude/skills/research-methods/SKILL.md && grep -q 'Context7 first' claude/agents/plan-investigator.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed
- SKILL.md: a paragraph after the source hierarchy giving the hosted-platform rung (Context7, then llms.txt or a search, then a fetch of a URL taken from those), reading docs/references/ first, and the guessed-URL/404 rule.
- plan-investigator.md: a Context7-first sentence beside the ToolSearch line.
