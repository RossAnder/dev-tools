Status: done — handover set, reshaped record, plan/handover-incomplete and gate `handovers` key documented.

Changes:
- claude/skills/tomlctl/references/plan.md: `plan handover add` section replaced by `plan handover set` (rule sentence, six-reason table, accepted keys, add-or-update rule, refused `done`, flag table, output); contents link and verb list updated; stage remedy table merged handover-task-unknown and handover-incomplete onto the set remedy; check-class table gained plan/handover-incomplete; gate table split into `stops` (gates only) and a new `handovers` row; digest `undone_handovers` fields listed.
- claude/skills/tomlctl/references/plan-store.md: sample and key table use reason/action/when/done_when; schema_version 2; source_task row says plan check reports unknown ids.
- claude/skills/tomlctl/references/flow.md: support-matrix row names `plan handover set`.
- claude/skills/tomlctl/SKILL.md: quick-reference row reads `handover {set\|done}`.

Deviations: none.
Compile check: not applicable (docs only). Accept checks: all four ran, pass. command_lint/finding-class gates not run (gate's job).

TANGENTIAL: none
