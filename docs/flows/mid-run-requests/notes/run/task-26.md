Status: done — Pre-flight bullet now names the command that lists unapplied granted request paths

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/run-milestone.md"]

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/run-milestone.md`: the Pre-flight bullet on paths granted by an unapplied request now ends with the `tomlctl plan request list --slug <slug> --where applied=false --select id,task,paths --lines` command and says to take each `paths` item whose `ruling` is `granted`. The reproduce-status accept check is the gate's.
