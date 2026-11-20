Status: done — added the bun/node eval-of-scripts default audit pattern, tests and docs

```toml run-note
files_touched = ["tomlctl/src/agents/audit.rs", "claude/skills/tomlctl/references/agents.md", "tomlctl/src/cli/types/cmd.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/agents/audit.rs"
result = "pass"

[[checks]]
command = "grep -q 'scripts/' claude/skills/tomlctl/references/agents.md"
result = "pass"
```

## What changed

- audit.rs: new default pattern `\b(?:bun|node)\s+(?:-e|--eval)\b.*\bscripts/\S`; the two default-pattern tests gain the five commands from the spec.
- agents.md: the defaults description names the pattern.
- cmd.rs: the `--pattern` help text mentions it.

No clippy or nextest was run (the nextest accept check is the gate's; sibling workers are editing the crate). The dispatch-tests subset for the claude/ markdown edit was not run.
