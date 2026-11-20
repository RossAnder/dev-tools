Status: done — the held gate-list test now seeds external_gates through the store's JSON fixture; emit.rs needed no change

```toml run-note
files_touched = ["tomlctl/tests/plan_emit.rs"]

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/tests/plan_emit.rs"
result = "pass"

[[checks]]
command = "cargo nextest run --manifest-path tomlctl/Cargo.toml -E 'test(=plan_emit::gate_list_holds_marked_gates_and_checks)'"
result = "not-run"
```

## What changed

The fault was in the fixture, not the verb. `tomlctl/tests/plan_emit.rs`, `gate_list_holds_marked_gates_and_checks`, parsed the rendered store back into a `toml::Value` and assigned `doc["milestones"][0]["external_gates"] = ...`. In toml 1.2.1, `IndexMut` on a `Value` is `self.get_mut(index).expect("index not found")` (`toml-1.2.1/src/value.rs`, `impl IndexMut for Value`), so assigning to a key that is not there yet panics instead of inserting it. The test died before it ever called `plan emit gate-list`. The gate log shows it failing in about 1.2 s with no assertion output.

The test now builds the store the way `gate_list_visual_holds_states_controls_and_looks` does. It passes `schema_version = 4` and a `milestones` array to the `store()` helper as its `extra` JSON. The first milestone carries `gates = [SUITE, LINT]` and `external_gates = [LINT]`, and the second milestone is the same as `gate_store`'s. The accept entries and assertions are unchanged.

`tomlctl/src/plan/emit.rs` is not changed. Read against the fixture, `gate_list` gives the asserted output:
- `marked` collects the milestone's `external_gates` plus each gate-owned accept check whose `effect` is `external`.
- Gates come first, then accept checks by row id and entry, and each marked one goes to `held`.
- `commands` is then SUITE then accept:1:0, and `held` is LINT then accept:1:1.

The gate runs the accept check; the procedure keeps gate-owned checks off workers, so it was not run here. No clippy run was needed: the edit only replaces one fixture literal with another, and the earlier gate round compiled this file.
