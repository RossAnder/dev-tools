Status: done — gate-list prints marked commands under a held key

```toml run-note
files_touched = ["tomlctl/src/plan/emit.rs", "tomlctl/tests/plan_emit.rs", "claude/skills/tomlctl/references/plan-engine.md"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml"
result = "pass"

[[checks]]
command = "grep -q '\"held\"' tomlctl/src/plan/emit.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/^plan_emit::/)'"
result = "not-run"
```

## What changed

- emit.rs: `gate_list` precomputes the set of commands marked anywhere (milestone `external_gates`, gate-owned accept checks with `effect = "external"`); marked commands go to `held` with their normal `source`, never `commands`. `held` is printed only when non-empty. Doc comment rewritten.
- plan_emit.rs: new test `gate_list_holds_marked_gates_and_checks`; the existing unmarked tests stand.
- plan-engine.md: `held` key documented and the holds sentence replaced.

The nextest accept check is the gate's to run. Clippy warned of an unused `gates_and_marks` in milestone.rs, a sibling's in-flight work.
