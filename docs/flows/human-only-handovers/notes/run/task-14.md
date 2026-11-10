Status: done — fixture now stages its handover through `plan handover set --ndjson`

- tomlctl/tests/review_plan_records.rs: added a HANDOVER payload constant (reason restart, action, when, done_when, source_task 1) and replaced the `handover add --command` call with `handover set --ndjson` via the fixture's payload helper. No expectation changed; H1 is still minted.

Deviations: none.

Checks: `! grep -q '"--command"'` passes. `cargo nextest ... binary(review_plan_records)`: 12 passed.

TANGENTIAL: none
