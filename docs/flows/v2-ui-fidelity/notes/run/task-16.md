Status: done — implement-v2-visual Workflow dispatches ui-inspector over groups of at most four states and returns an index; node:test cases added

```toml run-note
files_touched = ["claude/workflows/implement-v2-visual.js", "claude/workflows/tests/visual.test.mjs"]

[[deviations]]
what = "Each index entry carries a `status` field (ok, no-return, error, missing) beside the action's {state, control, verdict, defects, note, error}"
why = "The detail asks a dead agent's states to come back with status no-return, which the action's field list has no slot for; verdict is null whenever status is not ok, so visual.md can tell an unjudged state from a judged one"
means = "the index shape in the task's action"

[[deviations]]
what = "The test file carries its own copy of the harness loader instead of importing it"
why = "harness.test.mjs exports nothing, and importing it would register its tests a second time under node --test; editing it is outside this task's files"
means = "reuse the loader in tests/harness.test.mjs"

[[checks]]
command = "grep -q 'ui-inspector' claude/workflows/implement-v2-visual.js && grep -q 'implement-v2-visual' claude/workflows/tests/visual.test.mjs"
result = "pass"

[[checks]]
command = "node --test claude/workflows/tests/visual.test.mjs"
result = "pass"
```

## What changed

`claude/workflows/implement-v2-visual.js` (new, meta name `implement-v2-visual`, script version 1):

- Validates `{slug, milestone, round, capture_dir, design, states, controls, looks}` and refuses the whole launch, with errors ending `no agent launched`, before starting any agent. State names must match `^[A-Za-z0-9][A-Za-z0-9_.-]*$`, since each becomes a capture file name and an `--only` value. A name in both `states` and `controls`, or twice in one list, is refused. Each look must name only known states. A look's `task` may be a positive integer or a one-line ref, because the gate-list `visual` emitter (a sibling task) was not yet written to confirm which.
- Groups the units into `ceil(n/4)` groups of at most four. Controls go round-robin first, one per group, so as many inspectors as possible face a planted defect. Clean states then fill the groups in args order.
- Dispatches one `ui-inspector` per group, all at once, with a schema for the inspector's return. Each prompt names the slug, milestone, round, capture_dir, design, its note path `docs/flows/<slug>/notes/run/visual-<milestone>-<round>-<n>.md`, its states with their `control` flags, and only the looks that name one of its states, as JSON lines. The prompt embeds no `tomlctl` command.
- Returns `{states, errors, script_version}` with one entry per asked state, clean states first and then controls, each list in args order. A dead inspector's states come back `no-return`, a thrown or malformed one's `error`, and a state its return left out `missing`. A wrong note path keeps the verdict and sets `error`. No `Date.now`, `Math.random` or argless `new Date`.

`claude/workflows/tests/visual.test.mjs` (new): seven node:test cases. They cover grouping with controls spread, note paths and index order; prompt fields and look filtering; dead and thrown inspectors; a missing state and a wrong note path; fourteen bad-arg refusals that start no agent; absent args; and the no-clock/no-random constraint.

The directory-wide `node --test claude/workflows/tests/` check is the gate's. I ran only this task's own file.
