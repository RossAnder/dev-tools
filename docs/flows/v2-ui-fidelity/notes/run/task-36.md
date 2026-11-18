Status: done — inspector prompts carry no control flag, list states by name and give each state its look numbers

```toml run-note
files_touched = ["claude/workflows/implement-v2-visual.js", "claude/workflows/tests/visual.test.mjs"]

[[deviations]]
what = "A control carries the looks of clean states sharing its surface prefix, and every look only when no clean state shares it, not every look unconditionally"
why = "The task's action line says a control carries every look of the milestone, but its detail (and D41, which revises D39) gives the prefix rule; the detail and the named test's assertions follow D41, so the detail was implemented"
means = "action: a control carries every look of the milestone"

[[deviations]]
what = "Added a second new test, 'a prompt whose states carry no look prints an empty looks block', covering the `looks none` line and the `- none` looks block"
why = "Both printed forms are specified in the detail and no existing test exercised them after the old looks assertion changed shape"
means = "detail: add one test named exactly ..."

[[checks]]
command = "! grep -qF '(control: ' claude/workflows/implement-v2-visual.js"
result = "pass"

[[checks]]
command = "node --test --test-reporter=tap --test-name-pattern='a control carries every look' claude/workflows/tests/visual.test.mjs | grep -q '^ok [0-9]* - implement-v2-visual: a control carries every look and no prompt says which states are controls'"
result = "pass"

[[checks]]
command = "node --test claude/workflows/tests/"
result = "pass"
```

## What changed

`claude/workflows/implement-v2-visual.js`: `inspectorPrompt` now prints the group's states sorted by name as
`- <state>: looks <k>, <k>` (or `looks none`), where `<k>` is a look's 1-based position in `a.looks`. A clean state
carries the looks naming it; a control carries the looks naming a clean state that shares its surface prefix (the name
up to its last `-`), or every look when no clean state shares that prefix. The `looks:` block prints each carried look
once, in number order, as `- <k>: {"task":..,"accept":..,"behaviour":".."}` with no `states`, or `- none`. `control`
left `RETURN_SCHEMA.required` but stays in `properties`. Args checks, grouping, note path and the returned index (still
carrying `control` from the args) are unchanged.

`claude/workflows/tests/visual.test.mjs`: `statesOf` parses the new lines into `[state, lookNumbers]`; `looksOf` parses
`[k, look]`; `judging` takes the test's own control list. Existing tests keep their assertions on the new shape (the
looks test now also checks each state's look numbers and that `states` is not shown). Added the named test for the
prefix rule and an empty-looks test.

The accept check that pipes TAP into `grep -q` prints an EPIPE trace from node once grep exits early; the check's exit
status is grep's and it passes. 32 of 32 Workflow script tests pass.
