Status: done — plan check reports plan/milestone-width at info before the first approval

## What changed

- `tomlctl/src/plan/check.rs`: new `milestone_width_findings`, called from the `plan.approvals.is_empty()` branch beside `file_chain_findings`. Milestone order is the store's `[[checkpoints]]` order, with duplicate ids keeping their first position. For each milestone after the first, a row is reported when it has no needs, or when the highest milestone index among its direct needs is below its own index minus one. The finding is one per milestone, at INFO, with sorted `ids`, and a `detail` like ``milestone `C`: 2 could join `A`; 1 of 1 rows could start with an earlier milestone``. The module doc line now lists `plan/milestone-width` as info.
- `tomlctl/tests/plan_check.rs`: a `LATER` fixture adds milestone B with a row that has no needs. There are five `milestone_width_*` tests: a no-needs row in a later milestone is reported at info with exit 0 and ok true; a row needing the milestone just before its own is clean; a row whose need sits two milestones back is named with the milestone it could join; an approved store is clean; a one-milestone store is clean.
- `tomlctl/tests/plan_milestone.rs`, `tomlctl/tests/plan_probes.rs`: unchanged. No fixture there raises the class in a way the tests can see. plan_probes' `two_tasks` row 2 needs row 1 in the milestone just before its own, and plan_milestone's checks only look for other classes with `contains`.

## Deviations

- The detail does not say what to do with a row whose need points at a row in no listed milestone, for example an empty or undeclared checkpoint, or a dangling id. I leave such a row out rather than count the need as index 0. It cannot be placed, and the task store's own checks report dangling needs. The other option, treating it as index 0, could name rows that cannot in fact start earlier.

## Checks

- `rustfmt --edition 2024 --check` on check.rs and plan_check.rs: clean.
- `cargo clippy --manifest-path tomlctl/Cargo.toml --all-targets`: clean.
- `cargo nextest run ... -E 'binary(plan_check) | binary(plan_milestone) | binary(plan_probes)'`: 130 passed.
- Accept check `grep -q 'plan/milestone-width' tomlctl/src/plan/check.rs`: passes.
- Accept check `grep -q 'milestone_width' tomlctl/tests/plan_check.rs`: passes.
- Accept check `cargo nextest run ... -E 'test(milestone_width)'` belongs to the gate, so I did not run it on its own. The milestone_width tests did pass inside the narrow run above.

TANGENTIAL: none
