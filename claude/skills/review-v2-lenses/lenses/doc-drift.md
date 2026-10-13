+++
id = "doc-drift"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["comment-rot", "prose-drift", "missing-docs"]
activation = "candidate"
apply_when = { signals = ["docs-touched"] }
slice = "per-file"
v1_category = "quality"
+++

# doc-drift

## Question

Where do the comments and prose in this slice say something the code no longer does, and what does a reader need documented that nothing documents?

## Owns

`comment-rot` — a comment or doc comment that no longer describes the code beside it: a stated invariant the code does not keep, a parameter or return described wrongly, a branch explained as doing what its sibling does, a "why" whose reason was removed.

`prose-drift` — a README, guide, skill or reference page that contradicts current behaviour: a flag, default, output shape, file location, step order or limit that the code now handles differently.

`missing-docs` — behaviour a user or maintainer needs and no document covers: a new command, option, config key or file format with no entry in the doc that lists its siblings, or a failure mode or constraint a caller must know that only the code reveals.

## Probes — code

Treat every documented claim as a hypothesis and check it against the code; never the reverse.

- **Comments.** For each comment in or beside the changed lines, read the code it describes and test each claim: the invariant, the parameter meanings, the return and error cases, the stated reason. A comment the change made false is a row even when the change did not touch the comment.
- Doc comments on public items the change altered: parameters added, renamed or reordered, errors added, defaults changed.
- **Prose.** For each doc that names something in the slice — a command, flag, key, path, output field or procedure — run or read the code path and compare: defaults, accepted values, output shape, ordering, limits. Quote the doc line and cite the code line that contradicts it.
- Examples and snippets in docs: does each one still parse and produce what the doc says it produces?
- **Missing docs.** For each user-facing addition in the slice, find the doc that lists its siblings. An option added to a command whose other options are documented, with no entry for it, is missing.
- A constraint the code enforces that a caller trips over, such as a size cap, a required ordering or a platform limitation, documented nowhere a caller would look.
- When the `docs-touched` signal fired, start with the changed docs and check each changed claim against the code it describes.

## Does not flag

- Confirmations: a comment or doc that matches the code is a coverage-note line, never a row.
- A name that no longer exists, where the passage is otherwise right; that stale reference belongs to ripple.
- The same guidance written in two docs, and docs describing removed features or superseded procedures; those belong to doc-consolidation.
- Historical text describing past behaviour on purpose: changelogs, release notes, decision records, migration notes and commit-message examples.
- Comments that are true but terse, wording and style preferences, and requests for comments on code whose signature already says what the comment would.
- Error messages, log lines and help text the program itself emits; diagnostics-dx owns those.
- A deliberate gap a project rule or decision record sets, such as an internal module the project chooses not to document; challenge it as advisory if a reader is still misled.
- Docs outside the run's scope, and a rewrite whose gain is negligible against churning a stable doc.

## Edit forms

- `change` — correct a comment or doc passage. `old` is the exact current text; `edit` gives the corrected text and the code it was checked against.
- `remove` — delete a comment whose claim is false and adds nothing once corrected, such as a narration of what the next line does. `old` is the exact comment.
- `add` — the missing doc entry, beside its siblings. `old` is the adjacent anchor text, or `target_file` names a new doc only when no existing doc lists the siblings.

## Advisory angles

- `direction` — a documentation structure the evidence suggests, such as generating a flag table from the code because it has drifted repeatedly.
- `challenge` — a documented decision that the behaviour has since outgrown.

## Severity calibration

- `critical` — a doc or comment that leads a reader to a destructive action: deleting data, disabling a safety check, or running a command that does something other than what the doc says on a path that writes.
- `major` — prose drift on a user-facing default, flag or procedure that sends a user down a failing path, or a comment stating an invariant callers rely on that the code no longer keeps.
- `minor` — a comment or doc that is wrong in a detail a careful reader would catch from the code, or a missing entry for an option with a safe default.
