Status: done — plan gate emits a separate handovers array and the views carry all handover fields.

Changes:
- tomlctl/src/plan/approve.rs: gate_report `stops` now holds gates only; new `handovers` key directly after `stops` lists undone handovers as {id, reason (string or null), action, when, done_when, source_task}.
- tomlctl/src/plan/view.rs: `open_items.undone_handovers` carries the same six fields; markdown line is `- [ ] H1 (restart): <action> When: <when>. Done when: <done_when>. (after task <n>)`, `(no reason)` when reason is absent.

Deviations: none. The action is printed raw rather than as an inline code span (the spec's line shows plain text).

Checks: cargo clippy on tomlctl clean. Three accept checks (handovers key present, no `"kind": "handover"`, done_when in view.rs) pass.

TANGENTIAL: none
