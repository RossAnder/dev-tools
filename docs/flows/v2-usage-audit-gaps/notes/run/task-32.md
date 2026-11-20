Status: done — baseline gate and proof help and docs reworded

```toml run-note
files_touched = ["tomlctl/src/cli/types/plan.rs", "claude/skills/tomlctl/references/plan.md"]

[[checks]]
command = "! grep -q 'base commit when it fails' tomlctl/src/cli/types/plan.rs claude/skills/tomlctl/references/plan.md"
result = "pass"

[[checks]]
command = "! (grep -A3 'long = \"baseline-proof\"' tomlctl/src/cli/types/plan.rs | grep -q 'replace the whole list')"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed
Help strings for --baseline-gate (add and update) and --baseline-proof in plan.rs, and the two matching rows in plan.md, now describe a gate proved on the unchanged tree before approval; the proof help says upsert by gate. The dispatch test subset could not compile because of an in-flight edit in tomlctl/src/plan/records.rs (a sibling's file), so it was not run.
