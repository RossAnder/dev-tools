Status: done — v1 After Merge is now a human-only list in the handover shape, with an `install:` key and the rule in Format rules

## What changed

`claude/skills/flow-contract-plan-output-format/SKILL.md`:

- `## After Merge` template: replaced the agent-performable examples (install, resync a plugin cache, update a memory note, run a predicted criterion by hand) with the six human-only kinds, a pointer to the **After Merge** format rule for the bullet shape, and `_None._` as the expected case. The verbatim-repeat-in-final-report sentence is kept.
- `## Verification Commands` optional keys: added `` - `install: <command>` `` (repeatable, in order) for installs, deploys and links into the Claude config dir. `/implement` Phase 3 runs it last, after `lint:` and the audit, through the verification agent, and only when every earlier command passed. Checkpoints and the apply flows skip it.
- `**Format rules:**`: added one **After Merge** bullet (two physical lines). It carries the rule, the one-action shape `- **<reason>**: <one action>. **When:** <trigger>. **Done when:** <observable result>.`, the six reasons by name, and where each kind of agent-performable work goes instead, plus the mixed-step split.

Headings, the section-order bullet and the description are unchanged. The file is 262 lines, up from 254.

## Deviations

- None from a `suggested` means. I chose to restate the reasons inline instead of linking `docs/flows/human-only-handovers/design/handover.md`. A global skill should not depend on one flow's design doc, and the review fence reads only this file.

## Checks

- Compile check: not applicable, since only markdown changed.
- Accept 1 (`! grep -q 'update a memory note'`): pass.
- Accept 2 (`` grep -q '^- `install: <command>`' ``): pass.
- Accept 3 (Format rules block contains `Done when`): pass.
- C4 first half (`` `install: <command>` `` in this file): pass. The `implement.md` half belongs to the sibling task.

TANGENTIAL: none
