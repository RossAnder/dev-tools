Status: done — approve, view and lifecycle tests pinned to the separate handovers list and the new record shape.

Changes:
- tomlctl/tests/plan_approve.rs: handover fixtures use reason/action/when/done_when; stops expects only the gate entry; new `handovers` assertion with all six fields; gate key order includes `handovers`.
- tomlctl/tests/plan_view.rs: fixtures use the new keys; markdown assertions match the new handover line; digest `undone_handovers` carries every field.
- tomlctl/tests/plan_lifecycle.rs: handover created via `plan handover set --ndjson` with a complete row; gate assertions read `handovers` (one open, then empty after done); the rendered-view assertion looks for the action text.

Deviations: none.

Checks:
- `grep -q done_when tomlctl/tests/plan_approve.rs`: pass.
- `cargo nextest run ... -E 'binary(plan_approve) | binary(plan_view) | binary(plan_lifecycle)'`: 72 passed.

TANGENTIAL: none
