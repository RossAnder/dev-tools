Status: done — plan/handover-incomplete raised in check.rs; stage.rs routes both handover classes to plan handover set.

Changes:
- tomlctl/src/plan/check.rs: handover_finding became handover_findings (Vec, flat_map). Done handovers yield nothing; undone ones yield plan/handover-incomplete (error) naming the empty keys among reason, action, when, done_when, plus the unchanged plan/handover-task-unknown.
- tomlctl/src/plan/stage.rs: HANDOVER_REPLACE replaced by HANDOVER_SET (plan handover set --ndjson ...) in its definition, PROVE_ORDER, fn remedy (both classes) and the exhaustive remedy test (new class added).

Deviations: none.

Checks:
- cargo clippy on tomlctl: clean.
- grep accept checks 1 and 2: pass.
- cargo test plan::stage (Writes build: yes): left to the milestone gate.
- Contract C1 doc grep on plan.md depends on task 7.

TANGENTIAL: none
