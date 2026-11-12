Status: done — headless render cases added to glimpse/tests/cli.rs

Changed glimpse/tests/cli.rs: helper `once_surface` plus five cases, all through the existing Sandbox and the fixture:
- plan surface at 140x60 prints decisions, milestones, notes, health (case-insensitive) and D1.
- plan surface at 80x30 prints the tab short label `Pl`.
- inbox surface prints `needs you`.
- v2 task details: `--once` has no flag that opens the details panel (only `--select`, which selects). Deviation from the detail's first choice, as it allowed: the case asserts the fixture carries the spec `outcome` and `premises` keys the panel draws `Outcome`/`Premises` from, and that `--select 8` renders `8 Wire`. The panel's own text is covered by unit tests in view/task_spec.rs.

Checks: rustfmt --edition 2024 --check clean; accept check 1 (grep) passes; `cargo test --test cli once_` ran 13 passed (including the new five). The full crate gate is the gate's.

TANGENTIAL: debt | glimpse/src/model/plan.rs | `PlanIndex::health` is never used and warns dead_code in the bin | the Plan surface may not be drawing health from it
TANGENTIAL: annoyance | glimpse/src/runtime.rs | `--once` cannot open the details panel, so v2 task details are not testable at binary level | a `--details` flag would close the gap
