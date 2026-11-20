Status: done — pre-flight feature list and refusal sentence extended with the four new features

```toml run-note
files_touched = ["claude/commands/implement-v2.md"]

[[checks]]
command = "grep for the four feature strings in claude/commands/implement-v2.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

claude/commands/implement-v2.md: added `plan_emit_commit_paths`, `effect_mark`, `plan_run_gate_blocks` and `plan_run_visual` to the pre-flight feature list, and extended the refusal sentence to cover the `plan emit commit-paths` leaf, the external-effect mark, `plan run gate --blocks` and `plan run visual`.
