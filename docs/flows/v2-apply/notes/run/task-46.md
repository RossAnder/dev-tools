Status: done — added a test pinning that an orchestrator verdict on a panel-routed finding is refused unless it refutes an evidence-blocked one

- tomlctl/tests/review_verdict.rs: added fixture PANEL_BLOCKED (one panel-routed finding blocked on evidence, one unblocked) and test `orchestrator_verdict_on_a_panel_finding_is_refused_unless_it_refutes_a_blocked_one`. It submits orchestrator rows (confirmed on the blocked one, refuted on the unblocked one), asserts each fails with "is routed to the panel", status stays unverified and no verdict is written.
- Rows are submitted one per batch, so each refusal is checked on its own.
- Deviations: none.
- Checks: rustfmt clean; the new test passes (cargo test --test review_verdict orchestrator_verdict). The reproduce-status accept check writes build, left to the gate.

TANGENTIAL: none
