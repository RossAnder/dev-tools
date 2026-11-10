Status: done — check and stage tests cover plan/handover-incomplete and the in-place repair

Changes:
- tomlctl/tests/plan_check.rs: handover fixtures use the new keys (shared HANDOVER_HEAD); added tests for an undone handover missing `when`, one missing every field (names all four), and a done handover missing fields (clean).
- tomlctl/tests/plan_stage.rs: HANDOVER_NEXT matches the new remedy; the absent-task test now repairs H1 via `plan handover set --ndjson -` with source_task 1 and asserts ready, with no `handover done`. Test renamed accordingly.

Deviations: none.

Checks:
- grep accept check: passes.
- cargo clippy --test plan_check --test plan_stage: clean.
- rustfmt applied to the two files only.
- The nextest accept check writes build, so it is the gate's to run; not run.

TANGENTIAL: none
