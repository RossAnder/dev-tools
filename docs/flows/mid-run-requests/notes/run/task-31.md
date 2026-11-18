Status: done — requests.md now links report.md for the line shape and states the cross-milestone holder gap correctly

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/requests.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

requests.md: the Report paragraph no longer restates the line shape and points to report.md; the started-holder bullet now says `add` and `rule` compare only the requester's milestone rows. The accept check is the gate's.
