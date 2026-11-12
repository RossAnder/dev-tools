Status: done — `tasks snapshot` now emits each task's spec, file notes, backlog and finding links, and top-level `milestones` and `changes`

## What changed

- `tomlctl/src/tasks/snapshot.rs`: `task_json` now takes the store and the row. The row's existing keys come out unchanged (moved into `row_json`). After `commit` it adds `spec` (only when the store holds a spec for the row's `ref`), `file_notes` (path to note, every note keyed on the ref, `{}` when there are none), `closes` and `refs` (`[]` when there is no backlog link), and `finding` (`{store, fixes, settles}`, only when there is a finding link). The snapshot has its own `spec_json`, with keys in this order: outcome, constraints, means, executor, design, precedent, accept, premises. Every accept entry always carries `run_by`, `covered_by`, `owner` (`Accept::owner`) and `probeable` (`probes::skip_reason(..).is_none()`), plus `probe` once one is recorded. Every premise carries `claim`, `check`, `at` and `probeable` (`probes::premise_skip_reason(store, ..).is_none()`), plus `probe` (`at`, `neg_rc`, `verdict`) once recorded. The new top-level keys `milestones` (`id`, `gates`, `elaboration`, `defer_reason`, `gate_probes`) and `changes` (`at`, `ref`, `field`, `old`, `new`, `actor`, `reason`) come after `agents`. A change's `at` is the datetime's display form, or `""` when it has none, the same as `plan::lane` and `plan::view`. No `plan` key was added. `revision` and `SNAPSHOT_INPUTS` are untouched. The unit test of key order now expects the new keys, and checks that a v1 store's new keys are empty.
- `tomlctl/src/tasks/show.rs`: `premises_json` adds `probe` when one is recorded. Two helpers, `premise_probe_json` and `accept_probe_json`, are shared with the snapshot, and `accept_json` now calls `accept_probe_json`, so its output is byte-identical to before. In `tasks show`, the only change is the premise `probe` key.
- `tomlctl/src/plan/probes.rs`: `skip_reason` and `premise_skip_reason` are now `pub(crate)`, so the tasks module can reach them.
- `tomlctl/src/tasks/update.rs`: `spec_value("premises")` strips premise probes before serialising, the same way it already did for accept. Without this, review edits on `spec.premises` would stop matching their `expect` once a premise was probed. The `review_value` doc comment now says this. `spec_moves` still serialises premises with their probe, which matches how accept already behaves there.
- `tomlctl/tests/tasks_snapshot.rs`: the top-level key-order list gains `milestones` and `changes`. New cases:
  - a v1 store gains only empty keys;
  - a v2 task carries its spec, with a probed worker accept, a gate-owned accept that is not probeable, a probed `base` premise, an `after:` premise on a pending task (not probeable) and one on a done task (probeable), plus a file note, backlog link and finding link;
  - a v2 store carries a milestone with a gate probe, and a change row;
  - `tasks show` reports a recorded premise probe, but no `probeable`, and no empty `run_by`.

## Deviations

None from a `suggested` means. The `required` means is followed: the snapshot builds its accept entries itself, and `show::accept_json` keeps omitting empty `run_by` and `covered_by`. Choices the spec left open:
- `file_notes` includes every note keyed on the row's ref, not only notes for paths in `files`. The design says the object is "for this row's `ref`".
- Inside `spec`, the `ref` key is left out, because the design's key list does not include it.

## Checks

- `rustfmt --edition 2024` on all five files: clean after applying.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean in my files. One warning, "assertion has a constant value" at `src/plan/schema.rs`, is in a sibling's file.
- Accept "milestones and changes keys" grep: pass.
- Accept "probeable" in snapshot.rs grep: pass.
- Accept "probeable" in tests/tasks_snapshot.rs grep: pass.
- Accept `cargo nextest ... binary(tasks_snapshot)`: not run, because it is the gate's (Run by: gate).

TANGENTIAL: none
