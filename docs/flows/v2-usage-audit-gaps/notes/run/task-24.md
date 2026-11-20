Status: done — questions.md now marks the open-picker claim as unverified

```toml run-note
files_touched = ["claude/skills/implement-v2-engine/references/questions.md"]

[[checks]]
command = "grep -q 'unverified' claude/skills/implement-v2-engine/references/questions.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

`claude/skills/implement-v2-engine/references/questions.md`: the sentence saying an open picker blocks every later turn now states it as an unverified assumption. It says no observation has settled whether a `SendMessage` arriving while a picker is open is queued or lost. The rule to rule requests before opening a picker, and the reading of a timeout as denial, are kept, and the text says they hold either way.
