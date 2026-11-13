Status: done — worker run-note contract now opens with the toml run-note block

```toml run-note
files_touched = ["claude/agents/plan-worker-deep.md", "claude/agents/plan-worker-lite.md"]

[[checks]]
command = "bash scripts/verify-shared-blocks.sh"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass, 55 passed"

[[checks]]
command = "grep accept checks (toml run-note present; TANGENTIAL line form absent)"
result = "pass"
```

## What changed

Rewrote 'The note' section of the shared plan-worker-procedure block identically in both worker agents: Status line, blank line, `toml run-note` example block with all tables, field guidance, prose. The TANGENTIAL line form is removed. The Return section is unchanged.
