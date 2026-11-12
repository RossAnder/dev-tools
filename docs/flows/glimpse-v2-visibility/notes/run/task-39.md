Status: done — the plan index test's attempt expectation now matches the fixture; the lookup was right

## What was wrong

The lookup, `PlanIndex::attempts_for`, indexes `[[attempt]]` records by their `task` field and is correct. The fixture's `plan.attempt` array holds two records: task 3 (`done`, milestone A) and task 8 (`parked`, milestone B). The task 8 record is consistent with the rest of the fixture: task 8 is the parked row, and the latest report's `parked` entry names it. The test's `assert!(plan.attempts_for(&snap, 8).is_empty())` was the stale part.

## What changed

- `glimpse/src/model/plan.rs`: in `records_are_found_by_task_milestone_and_ref`, replaced the "no attempts for task 8" assertion with one that task 8 has exactly one attempt, whose status is `parked`, and added an assertion that task 2, which has no attempt recorded, returns none (keeping the empty-lookup case the old line was presumably after).
- `glimpse/tests/fixtures/snapshot.json`: not changed; the fixture is not wrong.

## Deviations

None. The suggested means (compare the fixture's attempt tasks with what `attempts_for` matches on) was followed.

## Checks

- `rustfmt --edition 2024 --check glimpse/src/model/plan.rs`: clean.
- `cargo clippy --manifest-path glimpse/Cargo.toml --all-targets`: compiles; one warning, `PlanIndex::health` is never used, from code outside this edit (an existing method with no caller yet).
- Accept check `cargo test ... model::plan::tests::records_are_found_by_task_milestone_and_ref`: `Run by: gate`, not run.

## Handover

None.

TANGENTIAL: debt | glimpse/src/model/plan.rs | `PlanIndex::health` has no caller, so clippy warns dead_code | a gate running clippy with `-D warnings` would fail unless a later task wires it into a view
