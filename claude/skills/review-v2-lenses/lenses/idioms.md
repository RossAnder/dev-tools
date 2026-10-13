+++
id = "idioms"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["pattern-consistency", "language-idioms", "naming-drift"]
activation = "candidate"
slice = "per-subsystem"
methods = ["history-idiom"]
v1_category = "quality"
+++

# Idioms

## Question

Does the change follow the patterns this codebase converged on and the language and framework as they are meant to be used, and name things the way the code around it does?

## Owns

`pattern-consistency` — divergence from the pattern the codebase converged on for the same job, decided from history with the `history-idiom` method, never from which file was read first.

`language-idioms` — a builtin or standard-library facility reimplemented by hand, a type-system or framework feature worked around, the framework's intended pattern bypassed.

`naming-drift` — names that disagree with the vocabulary the surrounding code uses for the same thing: two words for one concept, or one word for two.

## Probes — code

- For each pattern the change introduces, search for sibling code doing the same job before calling it novel. Is there an established way, and does the change use it?
- When the change and its neighbours disagree, run the `history-idiom` method: which form is the converged one, and has the history moved away from either? Never canonise the outlier, and treat a deliberate move away from a pattern as the new idiom.
- Does the change hand-roll something the standard library, the language or a dependency already in the manifest provides: a parse, a join, a retry, a path normalisation, a collection operation?
- Does it work around a type-system or framework feature instead of using it: a cast past a type the compiler could check, manual lifecycle handling the framework manages, a callback chain where the framework offers the structured form?
- Does a new name use a different word than the code around it for the same concept, or reuse an existing word for a different one?
- Is a new error, result or option shape inconsistent with how the rest of the module reports the same kind of outcome?
- A divergence whose fix needs only edits (use the existing helper, rename to the established word) is cleanup. A divergence that is wrong for a nameable input, such as a hand-rolled parse that mishandles a case the builtin handles, is a defect.

## Does not flag

- Confirmations: code that already follows the idiom is a coverage note, never a finding.
- A comment claiming a pattern is the house style, taken on trust without checking the history it summarises.
- Divergence a comment, ADR or CLAUDE.md line explains. A finding against it is advisory with `angle = challenge`, naming the decision.
- Style differences that leave readability intact, single-use helpers that aid clarity, and anything the formatter or linter settles; drop anything the `LINT BASELINE` already reports.
- Modernity for its own sake: an older form the codebase still uses consistently, unless the history shows a migration in progress.
- Code outside the scope the change did not touch.
- A rewrite to the idiom whose gain is negligible against the risk of touching working code.
- Duplicated logic belongs to `duplication`; layering and file placement to `architecture`; stringly-typed sets and illegal states to `type-design`.

## Edit forms

- `change` — replace a hand-rolled implementation with the builtin, standard-library or existing project helper, naming that helper in `edit`.
- `change` — rewrite the divergent site to the converged pattern, citing the history that shows which one converged.
- `change` — rename to the word the surrounding code uses for the same concept, with every reference in `instances`.

## Advisory angles

- `direction` — the codebase is split between two patterns with no clear winner in the history; recommend one and state the migration cost.
- `challenge` — a documented convention now works against the language or framework; name the convention in `challenges`.
- `alternative` — adopting a framework feature or library the project does not yet use would replace a recurring hand-rolled pattern.

## Severity calibration

- `critical` — a reimplementation that loses data or opens a hole the builtin closes, such as a hand-rolled path join that escapes its root.
- `major` — a hand-rolled or worked-around form that gives the wrong result for a nameable input the established form handles.
- `minor` — a divergence that is wrong only for a narrow input, such as an encoding or platform case the builtin covers.
