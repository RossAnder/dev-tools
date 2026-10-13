+++
id = "feasibility"
status = "stable"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["feasibility", "coherence"]
activation = "candidate"
slice = "whole-scope"
methods = ["trace"]
+++

# feasibility

## Question

Can the current architecture carry the changes the plan describes, as written, and do the plan's sections and tasks agree with one another about what is being built?

## Owns

`feasibility` — a change the current architecture cannot support as the plan writes it: a type, trait or module boundary that will not admit the planned shape, a layer that has no access to the data a task needs, a lifetime, ownership, threading or transaction model the design contradicts, a constraint of the runtime or tool the plan builds on that rules the approach out.

`coherence` — two parts of the plan that contradict each other: an Approach rule a task breaks, a User Decision the design ignores, a field, name, path or format spelt one way in one section and another way elsewhere, two tasks that each assume they own the same behaviour, or a Success Criterion the planned design cannot meet.

## Probes — plan

Read the whole plan before the code, then test each load-bearing design move against the code it lands in, with the `trace` method: read the types, call sites and invariants the move depends on before judging it.

- For each structural change — a new type, field, trait, verb, table, layer or seam — read the code it attaches to and ask whether it fits: the visibility it needs exists, the data it reads is reachable from where it runs, the ownership and concurrency model allow it, and nothing it must stay compatible with forbids it.
- For each mechanism the plan relies on, such as a lock, a transaction, an atomic rename, an event or a hook, find the existing code that provides it, and check that it provides it under the conditions the plan uses it in.
- Ask what the plan's own constraints rule out: a "no new dependency" rule beside a task that needs one, a "byte-untouched" file a task must edit, a size or count limit the design exceeds.
- Cross-read the sections: every rule in Approach against the tasks that implement it, every User Decision against the design, every name and format against each place it appears, every Success Criterion against what the tasks build.
- Cross-read the tasks: two tasks that describe the same behaviour differently, one that undoes another's work, or one whose Detail contradicts its own Action.
- For a contradiction, quote both sides with their sections, and say which one the code or a User Decision supports; when neither does, the choice is the user's.

## Does not flag

- Confirmations: a design move that fits is a coverage-note line, never a row.
- An infeasibility argued from memory of how such systems usually work, rather than from the code it lands in; cite the type, signature or invariant that rules it out.
- A third-party API used against its pinned version, which belongs to external-validity; a missing dependency edge, which belongs to plan-dependencies; a missing consumer or a wrong behavioural premise, which belongs to plan-grounding; and a decision a single task leaves to its executor, which belongs to plan-executor.
- A design the plan records as deliberate with its rationale; argue against it as `advisory` with `angle = challenge`, never as a defect.
- Speculative scaling or future-proofing concerns with no requirement in the plan behind them, and style-only preferences about how a section is written.
- An inconsistency of wording that cannot change what an executor builds.
- Work outside the plan's scope, and redesigns whose gain is negligible against the risk of changing course.

## Edit forms

- `change` — on the plan text, with `plan_section`, `anchor_old` and `anchor_new`: align a name, field, path or format to the spelling the rest of the plan and the code use, or bring a task's Detail into line with the Approach rule it implements, when the code or a User Decision settles which side is right.
- A contradiction neither side of which is settled, and an infeasible design, need a decision: write them as defects with `fix_shape = needs-plan`, or as advisory when the plan still works either way.

## Advisory angles

- `alternative` — a design that the current architecture would support where the planned one strains it, citing the code that makes it fit.
- `challenge` — a documented decision the code shows to be costly or unworkable; name it in `challenges`.
- `risk` — a design move whose feasibility turns on a runtime fact the cell cannot settle by reading, such as an operating-system or tool behaviour; name the experiment that would settle it.

## Severity calibration

- `critical` — the plan cannot be executed as written: a design the architecture rules out, or two tasks whose contradiction no execution order can satisfy.
- `major` — a design move that needs reworking before its task can land, or a contradiction an executor would resolve one way while its sibling resolves it the other.
- `minor` — a contradiction an executor would resolve correctly from context, or a strain the design survives at extra cost.
