Status: done — generated.md and plan-request.md now match the code for add's warnings, path canonicalisation and item refs

```toml run-note
files_touched = ["claude/skills/tomlctl/references/generated.md", "claude/skills/tomlctl/references/plan-request.md"]

[[checks]]
command = "! grep -qF 'and `plan request add` refuse' generated.md && grep -qF '`warnings`' plan-request.md"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"
```

## What changed

- generated.md: the loader rules say plan check warns, plan emit gate-list refuses, and plan request add (for a payload with paths) falls back to no generators and reports a `warnings` array.
- plan-request.md: the add section documents canonicalised paths (a path that canonicalises to nothing is refused), the folded key as canonical form with case folded, and the `warnings` output key. The rule section documents `p<n>`/`c<n>` item refs with plain digits and the parsed-ref duplicate check.
