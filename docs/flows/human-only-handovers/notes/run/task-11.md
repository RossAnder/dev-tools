Status: done — Handovers paragraph added to Design; approval shows `handovers` as its own item.

Changes:
- claude/skills/plan-v2-engine/references/write-and-prove.md: new **Handovers** paragraph in section 1 after Milestones, with a record example and the `plan handover set` command.
- claude/skills/plan-v2-engine/references/approve.md: item 7 `stops` is gate commands only; new item 8 `handovers` with reason, action, when, done-when. The empty-as-`none` rule is unchanged.

Deviations: none.
Handover: none.

Checks: both accept checks pass (grep). No compile check needed (docs only). The installed tomlctl predates `handover set` (it reports an unrecognised subcommand); the command matches the CLI reference in claude/skills/tomlctl/references/plan.md.

TANGENTIAL: none
