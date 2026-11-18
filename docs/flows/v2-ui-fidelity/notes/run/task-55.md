Status: done — rewrote the capture-failure sentence in visual.md step 4

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/visual.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/visual.md`: the first sentence of "Check the capture" now says a `fail` block or a second `timeout` is a capture failure, and `pass` and `flaky` go on to the checks. This matches the later rule that a first timeout is retried. The reproduce-status accept check is the gate's and was not run.
