Status: done — placeholder renamed to `<earlier-id>` in the earlier-checkpoint listing command.

- claude/skills/apply-v2-engine/SKILL.md: `checkpoint=<earlier id>` became `checkpoint=<earlier-id>`. The surrounding prose does not name the placeholder, so nothing else changed.
- Deviations: none.
- Checks: the grep accept check passed. The `command_lint` cargo test writes build, so it is left to the gate.
- The closing code fence at line 209 is followed on the same line by prose (the line starts with three backticks and then "The engine's report"). I left it alone because the task says to change nothing else.

TANGENTIAL: bug | claude/skills/apply-v2-engine/SKILL.md | The closing fence on the line after the listing command shares its line with prose ("``` The engine's report ..."), so the code block does not close cleanly. | It may break rendering and any fence-aware linting.
