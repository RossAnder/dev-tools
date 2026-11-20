Status: done — accept entries carry an optional `effect = "external"` (gate-owned only), milestones an `external_gates` list, and a store holding either is stamped schema_version 4.

```toml run-note
files_touched = [
  "tomlctl/src/tasks/schema.rs",
  "tomlctl/src/tasks/add.rs",
  "tomlctl/src/tasks/update.rs",
  "tomlctl/src/tasks/show.rs",
  "tomlctl/src/tasks/snapshot.rs",
  "tomlctl/src/tasks/milestones.rs",
  "tomlctl/src/plan/store.rs",
  "tomlctl/src/plan/lane.rs",
  "tomlctl/src/plan/milestone.rs",
  "tomlctl/tests/tasks_spec_add.rs",
  "claude/skills/tomlctl/references/tasks-write.md",
  "claude/skills/tomlctl/references/tasks-store.md",
]

[[deviations]]
what = "The snapshot projection (tasks/snapshot.rs spec_json) writes `effect` only when set, unlike `run_by`, `covered_by` and `states`, which it always writes."
why = "tests/tasks_snapshot.rs asserts the exact JSON of an unmarked accept entry and is outside this task's files; always writing `effect` would break it. Writing only when set keeps every existing snapshot byte-identical. The alternative was requesting that test file to add `\"effect\": \"\"`; the choice is easy to unwind and invisible to a user."
means = "wire `effect` wherever `covered_by` is, including tasks/snapshot.rs"

[[deviations]]
what = "The `effect` vocabulary check lives in `Accept::owner_problem` as well as in the add parser's `one_of`, and the vocabulary is the constant `EFFECTS = [\"external\"]` beside `RUN_BY_OWNERS`."
why = "Mirrors how `run_by` is validated in both places, so `plan check` reports an unknown `effect` read from a hand-edited store, not only the parser."
means = "detail: an unknown `effect` value is refused naming the vocabulary"

[[deviations]]
what = "The schema unit test `newer_schema_version_is_refused` now uses `SCHEMA_VERSION` and `SCHEMA_VERSION + 1` instead of the literals 3 and 4."
why = "With the newest version raised to 4, the literal 4 became this binary's own version and the test would have failed."
means = ""

[[checks]]
command = "cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets"
result = "pass"

[[checks]]
command = "rustfmt --edition 2024 --check <the ten edited .rs files>"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "pass"

[[checks]]
command = "grep -q 'external_gates' tomlctl/src/tasks/schema.rs"
result = "pass"

[[checks]]
command = "grep -q '\"effect\"' tomlctl/src/tasks/add.rs"
result = "pass"

[[checks]]
command = "grep -q 'external_gates' claude/skills/tomlctl/references/tasks-store.md"
result = "pass"
```

## What changed

- `tomlctl/src/tasks/schema.rs`: `SCHEMA_VERSION` is now 4, and a new `V3_SCHEMA_VERSION = 3` is the stamp `holds_v3_keys` selects, so a version-3 store keeps its stamp. New `holds_v4_keys` (an accept entry with a non-empty `effect`, or a keyed milestone with non-empty `external_gates`) is checked first in `to_toml`'s stamp ladder. New constants `EFFECT_EXTERNAL` and `EFFECTS`. `Accept` gains `effect`, read by `accept_from_toml` and written by `accept_to_toml` only when set. `owner_problem` refuses an unknown `effect` and refuses `effect` whenever `owner()` is not `gate` (so worker, implied worker, covered and look are refused, and `plan probes` can never run a marked command). `MilestoneSpec` gains `external_gates`, read and written only when non-empty. Tests: effect round-trips and stamps 4 (with `run_by = "gate"` and with the implied gate owner); effect refused on worker, implied worker, covered and look entries, and an unknown value refused; a milestone whose only mark is `external_gates` stamps 4 and round-trips; a version-3 store with no mark keeps its stamp and bytes. The two existing version-3 tests now use `V3_SCHEMA_VERSION`.
- `tomlctl/src/tasks/add.rs`: `effect` added to `ACCEPT_FIELDS`; `accept_entry` reads it with `one_of("effect", &EFFECTS)`.
- `tomlctl/src/tasks/show.rs`: `accept_json` emits `effect` when set.
- `tomlctl/src/tasks/snapshot.rs`: `spec_json` emits `effect` when set (see deviation).
- `tomlctl/src/plan/store.rs`: `entry_matches` compares `effect`; test literal gains the field.
- `tomlctl/src/tasks/update.rs`, `tomlctl/src/tasks/milestones.rs`, `tomlctl/src/plan/lane.rs`: test struct literals gain the new field.
- `tomlctl/src/plan/milestone.rs`: the two `MilestoneSpec` literals gain `external_gates: Vec::new()`; nothing else changed.
- `tomlctl/tests/tasks_spec_add.rs`: integration tests for the round-trip and stamp 4 through `tasks add-many` and `tasks show`, the refusals, and a look-only spec staying at stamp 3.
- `claude/skills/tomlctl/references/tasks-write.md`: the `spec` row documents `effect`.
- `claude/skills/tomlctl/references/tasks-store.md`: the `schema_version` row gives the version-4 rule; the accept-owner section documents `effect` and its refusals; the milestones paragraph documents `external_gates`.

The gate-owned nextest accept check was not run here, as the procedure leaves it to the gate. Clippy's one warning names `tomlctl/src/plan/commit_paths.rs`, a sibling's file. The README's `ui_look` capability row still says the store is stamped `schema_version = 3`; that stays true for a UI-only store, and the README is a sibling's file, so it is left unchanged.
