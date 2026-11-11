Status: done — accept entries carry `run_by` and `covered_by`, and every `writes_build` reader follows the effective owner

## What changed

- `tomlctl/src/tasks/schema.rs`: `Accept` gains `run_by` and `covered_by` (both `""` when absent), the `RUN_BY_*` constants and `RUN_BY_OWNERS`, `Accept::owner()` (the effective owner: `run_by`, else `worker` for `writes_build = false` and `gate` otherwise) and `Accept::owner_problem()` (unknown `run_by`, `worker` on a building check, `covered` without `covered_by`, `covered_by` without `covered`). The codec reads both keys and writes each only when set, so an entry with neither keeps its bytes. The v2 round-trip fixture now carries a `gate` and a `covered` entry; a new unit test pins the derived owner and the absent keys.
- `tomlctl/src/tasks/add.rs`: `ACCEPT_FIELDS` takes both keys; `accept_entry` reads `run_by` against the vocabulary and refuses any `owner_problem` at write.
- `tomlctl/src/tasks/show.rs`: `accept_json` prints `run_by` and `covered_by` only when set, so existing review `expect` objects still match.
- `tomlctl/src/tasks/update.rs`: only the test literal; `keep_probes` and the review-spec projection use struct-update syntax and carry the new fields unchanged.
- `tomlctl/src/plan/probes.rs`: `skip_reason` adds `run_by is not worker` after the existing `writes_build is not false`, so only a worker-owned non-building check is probed.
- `tomlctl/src/plan/check.rs`: `plan/accept-owner-invalid` (from `owner_problem`) and `plan/accept-cover-unknown` (a `covered_by` matching no gate of the row's checkpoint milestone), both errors; `plan/probe-absent` now uses `skip_reason`, the same rule as the probes.
- `tomlctl/src/plan/store.rs`: `entry_matches` compares `run_by` and `covered_by` when an applied entry carries them; test literal updated.
- `tomlctl/src/plan/view.rs`: under `Writes build:` each check prints `Run by: worker`, `Run by: gate` or ``Run by: gate (covered by `<command>`)``.
- `tomlctl/src/plan/stage.rs`: both new classes map to `tasks update --ndjson`, and the class-enumerating unit test lists them.
- Tests: `accept_run_by_round_trips` and `accept_worker_owner_refuses_a_building_check` (tasks_spec_add.rs); `worker_view_prints_run_by` (plan_view.rs); `gate_owned_check_is_not_probed` (plan_probes.rs); `check_reports_unknown_cover`, `check_reports_a_worker_owner_on_a_building_check`, `a_gate_owned_check_is_never_absent_a_probe` (plan_check.rs); two more `spec_shape_clears` cases for the new classes (plan_stage.rs).

## Deviations

- None from a `suggested` means. Choices the detail left open:
  - `accept-owner-invalid` also covers an unknown `run_by` value and a `covered_by`/`covered` mismatch, so a hand-edited store gets the same rules as a write. The alternative was a separate class per case; the shapes page names only two classes.
  - The covered command in the view is printed in backticks, the same as the `Check:` line, where the shapes page shows a bare `<command>` placeholder.
  - `keep_probes` still drops a probe when only `run_by` or `covered_by` changes, because it compares every field. Keeping a probe across an owner-only change would mean widening that comparison. I left it conservative.

## Handover

None.

## Checks

- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `rustfmt --edition 2024 --check` on every edited file: clean after applying it to schema.rs and check.rs.
- The five accept greps: all pass.
- The narrow run from the detail, `cargo nextest run ... -E 'test(run_by) | test(owner) | test(cover) | ...'`, plus the stage and probe-absent cases: 57 passed. The plan_probes, plan_check, plan_view, tasks_spec_add and plan_stage binaries, with the lib `tasks::`/`plan::` tests: 605 passed.

## Tangential

TANGENTIAL: debt | claude/skills/tomlctl/references/plan-probes.md | the reference page and write-and-prove.md still say any `writes_build = false` row is probed, and the skip reason now also reads `run_by is not worker` | the docs belong to the reference-page task of this plan and will mislead until it lands
