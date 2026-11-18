Status: done — plan-store.md now says a request item's `by` is `""` while open

```toml run-note
files_touched = ["claude/skills/tomlctl/references/plan-store.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/tomlctl/references/plan-store.md`: replaced "absent while open" with `""` while open in the request item `by` description. The reproduce-status accept check is the gate's and was not run.
