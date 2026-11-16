Status: done — tasks/milestones.rs computes upstream milestones, each milestone's state and the pick, with 12 unit tests

```toml run-note
files_touched = ["tomlctl/src/tasks/milestones.rs", "tomlctl/src/tasks/mod.rs"]

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass (only dead_code warnings on the new module, whose callers are plan emit schedule and plan check, still to land)"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- tasks::milestones"
result = "pass (12 passed)"

[[checks]]
command = "rustfmt --edition 2024 --check tomlctl/src/tasks/milestones.rs"
result = "pass"

[[checks]]
command = "test -f tomlctl/src/tasks/milestones.rs && grep -qF 'mod milestones;' tomlctl/src/tasks/mod.rs"
result = "pass"

[[checks]]
command = "grep -qF 'pub(crate) fn schedule(' / 'pub(crate) fn upstream(' tomlctl/src/tasks/milestones.rs"
result = "pass"

[[checks]]
command = "grep -qF '#[cfg(test)]' and #[test] count >= 11 in tomlctl/src/tasks/milestones.rs"
result = "pass (12)"
```

## What changed

- `tomlctl/src/tasks/milestones.rs` (new): `upstream(store)` maps every `[[checkpoints]]` id to its upstream milestones: the milestones reached through `Graph::closure_up` over `needs` and `coupling` (graph errors mapped through `graph::refuse`), plus the ids its `after` names. An `after` naming no stored checkpoint is refused as a validation error. `schedule(store, elaborating)` returns `Schedule { pick: Option<String>, milestones: Vec<MilestoneState { id, state, on, left }> }` in checkpoint order, following design/schedule.md's state table and pick rules, and reports `held` in place of `started`/`runnable`. `WORKER_AGENTS` holds the two worker agent names. The module imports nothing from `crate::plan`, and graph.rs is unchanged. The 12 tests cover the eleven listed cases plus one for the unknown-`after` refusal.
- `tomlctl/src/tasks/mod.rs`: registers `pub(crate) mod milestones;`.

## Choices where the design is silent

- A "stored milestone" is a `[[checkpoints]]` id, because states are reported in `[[checkpoints]]` order. A `[[milestones]]` spec whose id names no checkpoint is skipped. A row whose `checkpoint` is empty or unknown belongs to no milestone. Such rows are still walked, and the walk goes through them to the milestones they reach.
- An `after` that names its own milestone is kept as written. The milestone then waits on itself and is never picked. This is the literal reading and it holds rather than picks. `plan check` is the place to report it.
- An `after` that names a later milestone is not refused here. It still counts as upstream, which only adds holds. The ordering rule belongs to `plan check`.
- An `elaborating` id whose milestone is not `deferred` changes nothing, since the design's `elaborating` row requires `elaboration = deferred`.
- `on` for `deferred` and `elaborating` uses the other milestones' own states (`done` or `empty`), never the `held` override, so a held milestone still counts as unsatisfied.

`Schedule` carries no JSON serialiser. The crate has no serde derive, so `plan emit schedule` builds the object from the fields.
