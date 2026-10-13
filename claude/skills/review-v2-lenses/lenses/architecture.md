+++
id = "architecture"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["layering", "placement", "evolvability"]
activation = "candidate"
slice = "whole-scope"
v1_category = "architecture"
+++

# Architecture

## Question

Does the change put its logic in the layer that owns it, its files where the tree's layout says they go, and leave the codebase no harder to change next time?

## Owns

`layering` — logic in the wrong layer, and coupling that runs against the dependency direction the tree establishes: a domain module reaching into transport, a library module reading process state, a lower layer importing an upper one.

`placement` — new or moved files placed or named against the module boundaries and naming the tree already uses, judged by inspecting the neighbouring directories rather than by assuming a convention.

`evolvability` — a change that makes the next change harder: a seam closed that callers relied on, a concern scattered across modules that one module owned, a single type taking on roles that will diverge.

## Probes — code

- Read the module tree around every new or moved file first. Name the directory that holds its siblings by role, and flag a placement only when an existing file contradicts it.
- For each new import or call edge, name the layer of both ends. Is the edge in the direction the other edges between those two layers already run?
- Does the change put decision logic in an entry point, handler or CLI dispatch layer that the tree keeps thin, or I/O in a module the tree keeps pure?
- Does a module now know a detail another module owned before the change: a file path, a format, a status vocabulary, a wire shape?
- Does the change widen a public surface that only one caller needs, or make a private detail part of a contract other modules now depend on?
- Is one concern now spread across several modules where the tree had one owner, so the next edit to it has to find every copy?
- Would the next likely change (another variant, another caller, another backend) need an edit in one place or in several? Name that change from the code or the plan, not from speculation.
- When a fix needs only edits (move a function to the module that owns its data, route a call through the existing repository layer), it is cleanup. When it needs a decision about direction, it is advisory.

## Does not flag

- Confirmations: a layer, placement or boundary that is fine is a coverage note, never a finding.
- A comment or doc describing the intended structure, taken on trust without reading the code it describes; check the code, and report a disagreement between the two to `doc-drift`, not here.
- A documented pragmatic shortcut: a comment, ADR or CLAUDE.md line explaining the coupling. A finding against it is advisory with `angle = challenge`, naming the decision.
- Coupling confined to one module, and a placement no existing file contradicts.
- Speculative evolvability: a future change no code, plan or issue names, and layering preferences of style alone.
- Structure outside the scope the change did not touch, unless the change made it worse.
- A restructuring whose gain is small against the churn and risk of moving the code.
- Duplicated logic and duplicated facts belong to `duplication`; persistence and domain-model boundaries to `domain-model`; types that admit illegal states to `type-design`.

## Edit forms

- `change` — move a function, type or constant into the module that owns its data or its layer, updating the callers in `instances`.
- `change` — route a call through the existing layer the tree uses for it, such as the repository, the I/O module or the shared client, instead of reaching past it.
- `change` — rename or move a new file to match the naming and placement of its siblings.
- `remove` — drop a dependency edge that runs against the layer direction once the code it served has moved.

## Advisory angles

- `direction` — the change sets or follows a structure the codebase would be better without; state the target structure and its adoption cost.
- `challenge` — a documented decision about layering or module boundaries now costs more than it saves; name the decision in `challenges`.
- `risk` — a coupling that is fine today but that a named, likely next change will break.
- `alternative` — a re-architecture of the changed area, when the fix needs more than edits.

## Severity calibration

- `critical` — a layering break that ships broken behaviour, such as a lower layer now bypassing a check the upper layer enforced.
- `major` — the change makes a named, imminent next change need edits in several modules, or breaks a boundary other modules rely on.
- `minor` — a misplaced or misnamed file, or a coupling against the direction that one caller exercises.
