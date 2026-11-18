Status: done — added the `not shown` look answer and updated the pass verdict

```toml run-note
files_touched = ["claude/agents/ui-inspector.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

claude/agents/ui-inspector.md: pass 3 now offers `met`, `not met`, `unsure` or `not shown` (with the no-defect, no-unsure explanation), and the `pass` verdict accepts look entries that are met or not shown. The reproduce-status accept check is the gate's.
