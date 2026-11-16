Status: done — CLAUDE.md describes the dependency pick and the --elaborate entry

```toml run-note
files_touched = ["CLAUDE.md"]

[[checks]]
command = "grep -qF 'plan emit schedule' CLAUDE.md && grep -qF -- '--elaborate <id>' CLAUDE.md"
result = "pass"
```

## What changed

CLAUDE.md, Implement v2: the "continues while nothing is parked" clause now says milestones run in the order `tomlctl plan emit schedule` allows, with a deferred milestone skipped and reported. The Deploy bullet's pre-flight list gains `milestone_schedule`. Planning v2 gains an Elaboration bullet for `/plan-v2 <slug> --elaborate <id>`.
