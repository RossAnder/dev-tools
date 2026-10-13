+++
id = "type-design"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["illegal-states", "untyped-primitives", "stringly-typed"]
activation = "candidate"
slice = "per-subsystem"
apply_when = { signals = ["public-api-change", "new-seam"] }
methods = ["trace"]
v1_category = "architecture"
+++

# type-design

## Question

Do the types in this slice make invalid states unrepresentable and carry domain meaning across boundaries, or do they leave correctness to every caller's discipline?

## Owns

`illegal-states` — combinations of booleans, optionals and loosely related fields that let a value exist in a state the domain forbids: two flags that must never both be set, an optional that is required whenever another field has a value, a status field contradicted by the data beside it.

`untyped-primitives` — domain-meaningful values crossing module or process boundaries as bare strings or integers, so an id, a path, a unit or a validated value can be confused with another of the same primitive type; fields that are redundant or derivable from others and can drift from them.

`stringly-typed` — a closed set of values carried as free strings, compared by literal at each use, where an enum or union would let the compiler enumerate the cases.

## Probes — code

Start from the types the change introduces or alters, then follow each one to its construction sites and its readers.

- **Illegal states.** For each struct, record or object type, list its fields and ask which combinations the domain forbids. Find whether anything stops a caller building one: a constructor that validates, a sum type that separates the cases, or nothing. When nothing does, find a construction site or deserialisation path that can produce the forbidden combination.
- **Derived fields.** For a field computable from others, find the sites that write it and check each one updates it in step; one site that forgets is the defect, and the field itself is the cleanup.
- **Primitives at boundaries.** For each function signature, wire shape and storage column the change touches, mark the parameters that carry a domain identity, unit or validated form as a bare primitive. Look for two such parameters of the same type adjacent in one signature, where a swapped argument compiles.
- **Closed sets.** Search for a string or integer compared against several literals across more than one site. Collect every literal and every site with the trace method; a site whose literal set differs from the others, or a typo the compiler cannot catch, is a defect, and the set itself is the cleanup or advisory.
- **Validation placement.** Where a value is parsed or validated, check whether the result carries proof of that — a distinct type — or whether later code must trust that validation happened earlier.

A finding names the forbidden state or the confusable pair, and the site where it arises or could. A type that permits a bad state no code path can produce is cleanup at most.

## Does not flag

- A confirmation that a type is sound; put it in the coverage note.
- Primitives inside one function or one private module where no boundary is crossed and no confusion is reachable.
- A loose type a comment, decision record or CLAUDE.md line justifies — wire compatibility, a generated binding, a third-party schema. A disagreement is advisory with `angle = challenge`.
- Style differences that leave readability intact, single-use helpers that aid clarity, and newtype or enum proposals justified only by taste.
- Representation choices made for access-pattern or performance reasons; those belong to the perf family.
- Persistence and aggregate modelling; that is the domain-model lens.
- A rework of a stable public type whose gain is negligible against the cost to every caller; raise it as advisory with `angle = direction` if it matters.

## Edit forms

- `change` — replace a closed string set with an enum or union in a slice that owns every use; give a confusable parameter its own type where the edit stays within the slice; replace a pair of flags with one sum type.
- `remove` — a redundant or derivable field whose every reader can compute it, listing each reader under `instances`.
- `add` — a constructor or parse function that enforces an invariant existing callers already respect.

A type change that ripples beyond the slice or across a public API is advisory, not cleanup.

## Advisory angles

- `direction` — a type redesign whose adoption touches callers outside the scope or a public contract.
- `alternative` — a different representation, such as a state machine type in place of a status field plus optionals, cited against the forbidden states it would remove.
- `challenge` — a documented choice of a loose type that the change now leans on in a way the decision did not weigh.

## Severity calibration

- `critical` — a forbidden state is constructed on a reachable path and is persisted or sent over the wire.
- `major` — a forbidden state or confused value is reachable from a nameable input and produces wrong output.
- `minor` — a forbidden state is reachable only through an unlikely construction path, or a literal mismatch affects a rarely used value.
