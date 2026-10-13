+++
id = "ripple"
status = "stable"
families = ["code", "plan"]
mode = "mechanical"
produces = ["cleanup"]
owns = ["stale-references", "unwired-seams", "removed-justification", "incomplete-sweep"]
activation = "floor"
slice = "whole-scope"
methods = ["sweep"]
v1_category = "completeness"
+++

# ripple

## Question

What else in the tree names, depends on or repeats the thing this change renamed, removed, added or fixed, and was not changed with it?

## Owns

`stale-references` — a renamed or removed symbol, flag, file, message or config key still named somewhere the change did not touch: callers, comments and doc comments, prose docs, configuration, migrations, help text and user-visible string literals.

`unwired-seams` — a new registration seam (a provider, plugin, registry, hook, handler table or verb) with no production call site that reaches it, so everything behind it passes its tests and never runs.

`removed-justification` — a removed consumer that something else still cites as the reason it exists: a barrel's publication bar, an allowlist's membership rationale, a comment saying a field is kept for that consumer.

`incomplete-sweep` — sites elsewhere that share the pattern the change fixed or altered and need the same edit, where the change reached only some of them.

## Probes — code

Run each probe as the `sweep` method describes: a recorded search string, the whole tree as the search space, every hit read at its line. Report one line per probe in the coverage note even when it finds nothing.

- For every symbol, file, flag, error message or config key the diff renames or removes, search the whole tree for the old spelling: source, comments and doc comments, `.md` and other prose, configuration, test fixtures and string literals. Search the bare name, not only a call-shaped `name(` spelling, and search the old file name without its extension.
- A copied literal of a deleted or reworded message is a stale reference even when a test still passes on it; the test may only ever feed a mock.
- For every seam the diff adds — a trait or interface others implement, a registry entry point, a hook, a dispatch table — find the production entry point that calls it. A seam reached only from tests is unwired.
- For every consumer the diff removes, search for prose or code that names it as a reason: "kept for", "used by", "required by", allowlist comments, publication rules. Each hit still citing the removed consumer is a row.
- For the pattern the diff fixes (a guard added, a call replaced, an argument added), search for the same shape at other sites. List every unfixed site in `instances` of one row, never one row per site.
- A renamed public item: check re-exports, barrel files, generated bindings and documentation tables as well as direct callers.

## Probes — plan

Run the same `sweep` discipline against the tree the plan will change, reading the plan's tasks as the diff. Report one line per sweep in the coverage note even when it finds nothing.

- Sweep 1: for every symbol, file, flag or message a task renames or deletes, search the whole tree for the old name, comments, doc comments, prose and user-visible string literals included. Each hit in a file no task's Files line names is a row against the task that does the rename.
- Sweep 2: for every registration seam a task introduces, find the task, or the existing code, that calls it from a production entry point. No such call site anywhere in the plan or the tree is a row.
- Sweep 3: for every consumer a task removes, search the tree for anything citing that consumer as justification. A documented inclusion criterion is a dependency the plan must update or retire.
- Same-pattern sites: when a task fixes or changes a pattern at named sites, search for the remaining sites of that pattern the plan leaves untouched.

## Does not flag

- Confirmations: a sweep that came back clean is a coverage-note line, never a row.
- A hit whose surrounding text is itself out of date for other reasons; ripple reports the stale name, and judging whether the passage is otherwise right is doc-drift's call.
- Historical text that names the old symbol on purpose: changelogs, release notes, migration files already applied, decision records and commit-message examples.
- Matches that only share a substring with the old name, or a same-named symbol in an unrelated module, proven by reading the hit.
- Defensive checks the framework already guarantees, and TODOs tracked elsewhere.
- Tests the change did not write; missing tests belong to test-gaps.
- Files the change affects in a way grep cannot show — a behavioural dependency, a consumer of changed output shape — which need judgement; escalate them rather than writing a row.
- In the plan family, files, components and consumers the change affects beyond what a sweep's search string finds; those are plan-grounding's unmentioned consumers.
- Out-of-scope paths the run's scope excludes, and generated or vendored files.

## Edit forms

- `change` — replace the stale spelling with the new one. `old` is the exact text at the anchor; `instances` lists every other file carrying the same stale text.
- `remove` — delete a justification sentence or allowlist entry that cites a removed consumer, when nothing else depends on it. `old` is the exact sentence or entry.
- An unwired seam in code is never written as cleanup: adding a production call site changes behaviour. Escalate it with the search strings that found no call site.
- Plan family: `change` on the task text, with `plan_section`, `anchor_old` and `anchor_new` — adding the missed file to the task's Files line and Action, the missed site to its sweep, or the missing call site to the task that introduces the seam.

## Advisory angles

None. A mechanical cell writes cleanup only; a candidate that needs a decision is escalated.

## Severity calibration

None. A mechanical cell writes no defects; a stale reference that breaks a build or a runtime lookup is escalated so a judgement cell can write it as a defect.
