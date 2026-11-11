Status: done — added a test pinning that --answer-file is refused on a decision-blocked finding that is not open.

- tomlctl/tests/review_triage.rs: new test `answer_is_refused_on_a_decision_blocked_finding_that_is_not_open`. For both `refuted` and `unverified` it blocks F3 on decision, rewrites the status, runs `--answer-file`, and asserts failure with "--answer-file answers an open finding", store bytes unchanged, block and status still in place.
- Deviations: none.
- Checks: rustfmt --edition 2024 --check clean; `cargo test --test review_triage answer_is_refused` passes (2 tests). Accept check (reproduce-status, Writes build: yes) left to the gate.

TANGENTIAL: none
