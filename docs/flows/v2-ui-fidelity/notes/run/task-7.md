Status: done — plan emit gate-list prints a `visual` object for a milestone with ui_states

```toml run-note
files_touched = ["tomlctl/src/plan/emit.rs", "tomlctl/tests/plan_emit.rs", "claude/skills/tomlctl/references/plan-engine.md"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/plan/emit.rs tomlctl/tests/plan_emit.rs"
result = "pass"

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "grep -q '\"visual\"' tomlctl/src/plan/emit.rs && grep -q 'looks' tomlctl/src/plan/emit.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(/gate_list_visual/)'"
result = "not-run"
```

## What changed

- `tomlctl/src/plan/emit.rs`: `gate_list` matches `RUN_BY_LOOK` explicitly and collects each look entry as
  `{task, accept, behaviour, states}` in row-id then entry order; look entries enter neither `commands` nor
  `skipped`. When the milestone's spec has non-empty `ui_states`, the output gains
  `visual: {states, controls, looks}`; otherwise the object is built exactly as before (`{commands, skipped}`),
  so output for a milestone without ui_states is byte-identical.
- `tomlctl/tests/plan_emit.rs`: `gate_list_visual_holds_states_controls_and_looks` (schema_version 3 fixture with
  ui_states, ui_controls and look entries across two rows, plus a gate-owned check between them) and
  `gate_list_visual_is_absent_without_ui_states`.
- `claude/skills/tomlctl/references/plan-engine.md`: the gate-list section documents the `visual` key.

The first clippy run failed on `tomlctl/src/plan/check.rs` (a sibling's in-flight edit, missing
`ui_scope_findings`); a rerun moments later was clean. The nextest accept check is the gate's to run.
