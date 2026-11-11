Status: done — two tests added to tomlctl/tests/review_schedule.rs

- Added `a_finding_bridging_two_groups_merges_them_in_chosen_order` (before the medium-defect test): F1 on a.rs, F2 on b.rs, cleanup F3 on a.rs with instances b.rs; asserts one task, fixes [F1,F2,F3], files [a.rs,b.rs].
- Added `depends_on_reaches_an_earlier_call_and_reports_an_unscheduled_dep` (after the rerun-neighbour position, before `a_rerun_after_the_task_landed_marks_without_minting`): second call's F2 needs the first call's task id; F3's dep on F9 appears in dropped_deps.
- Placement is near the named neighbours rather than strictly adjacent for the first test; no behaviour changed.
- Checks: rustfmt --check clean; `cargo test --test review_schedule -- bridging depends_on` passed (3 tests). The reproduce-status accept checks write build and are the gate's.

TANGENTIAL: none
