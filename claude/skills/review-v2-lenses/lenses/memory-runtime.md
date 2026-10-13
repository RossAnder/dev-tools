+++
id = "memory-runtime"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["allocations", "memory-retention", "runtime-overhead"]
activation = "candidate"
slice = "per-subsystem"
methods = ["measure"]
v1_category = "memory"
research_domain = "compilers, codegen and memory management, plus the vendor microarchitecture manuals"
+++

# memory-runtime

## Question

On the paths this slice runs per unit of input, what does the code allocate, copy, retain or make the runtime do that the work does not need, and what does that cost at a size the project really sees?

## Owns

`allocations` — heap allocation and copying on a path the workload runs per item, per request or per file: a clone or owned copy where a borrow or view would serve, a buffer or collection rebuilt each iteration instead of reused, a string built by repeated concatenation, a collection grown without the capacity the code already knows, a temporary materialised only to be iterated once.

`memory-retention` — memory held longer than its use: a cache or map with no bound and no eviction, a buffer kept alive by a long-lived owner after its last read, a closure or callback that captures more than it uses and outlives the call, listeners and subscriptions never released, a whole input held in memory where the code reads it once in order.

`runtime-overhead` — work the language runtime or framework does on the code's behalf on a hot path: boxing and dynamic dispatch where the concrete type is known, reflection or runtime type inspection per item, generic or virtual calls that block inlining or specialisation, reactivity or change tracking over objects far larger than what the view reads, and shipped code that defeats dead-code elimination where the stack sends code to a client.

## Probes — perf

Start from the workload, as the measure method requires: name the operation, the input that drives it and a size this repo really sees, then read the slice along that path. Climb the measure method's ladder and record the rung; without a PROFILE BASELINE fence or an existing benchmark, a defect needs an allocation count or a complexity argument tied to the sourced size, and anything less is advisory with angle risk.

- **Per-item allocation.** Inside each loop or per-request handler, count the allocations and copies per iteration at the cited lines: clones and owned conversions of values only read, formatting or concatenation into a fresh string, a collection collected only to be iterated again, a buffer allocated inside the loop that could be hoisted and cleared. Multiply by the sourced item count.
- **Capacity and growth.** Where the final size is known or bounded before a collection is filled, check whether the code reserves it; where a collection is cleared and refilled each iteration, check it is reused rather than rebuilt.
- **Retention.** For every cache, map, list or registry that lives longer than one operation, find what bounds it and what removes entries. For every long-lived owner, check it does not keep a large input, buffer or response alive after the last read. For closures, callbacks and listeners, check what they capture and when they are released.
- **Streaming versus whole-input.** Where an input is read once in order, check whether the code loads it whole first, and what that costs at the largest size the project handles.
- **Runtime work per item.** On the hot path, find boxing, dynamic dispatch, reflection, runtime type checks and per-call setup the runtime repeats. Note where a concrete type or a one-time setup would remove it, and where the code's shape stops the compiler or JIT from inlining or specialising.
- **Hot versus cold.** Check that the cost sits on a path the workload runs often. Allocation in start-up, error handling, a one-shot migration or a debug branch is not a finding without its own frequency argument.

The facets bound to the cell carry the stack's own pitfalls; apply them alongside these probes rather than instead of them.

## Does not flag

- A confirmation. A path you read and found lean goes in the cell's coverage note, never in a finding.
- An allocation or copy off the hot path, or one whose count multiplied by its frequency gives a saving the workload would not notice.
- A hypothesis without a sourced size or a count, raised as a defect; it is advisory with angle risk, naming the measurement that would settle it, or nothing.
- A claim resting on a comment about cost or ownership you did not verify against the code.
- A deliberate choice the code, a decision record or CLAUDE.md documents — a clone kept for clarity, an unbounded cache sized by an external limit. A disagreement is advisory with angle challenge, naming the decision.
- Async runtime topology, locks and blocking calls; those belong to the concurrency lens. Super-linear algorithms and repeated computation belong to the algorithm lens, and process spawns, filesystem walks and start-up cost to the io-process lens.
- How a type models the domain rather than how it performs; that is the code family's type-design lens.
- A micro-optimisation that trades readability or safety for a gain negligible at the sourced size, or a speculative future scale with no evidence in the scope.
- Anything the LINT BASELINE already reports, and work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

Cleanup here is the small, safe win the measure method's allocation rung supports when the magnitude does not reach a defect.

- `change` — borrow or view instead of an owned copy on a read-only path; hoist a buffer out of a loop and clear it each iteration; reserve a known capacity; swap repeated concatenation for one builder.
- `remove` — a clone, conversion or intermediate collection whose result is only read or iterated once.
- `add` — a bound or eviction on a cache whose growth is shown, when the bound is already stated elsewhere in the code or its documentation.

## Advisory angles

- `risk` — a cost the code makes plausible but no rung above hypothesis supports: no PROFILE BASELINE, no sourced size, or a path whose hotness you cannot show. The rationale names the measurement that would settle it.
- `alternative` — a different ownership or memory strategy for the subsystem, such as pooling, arenas or a streaming reader, whose adoption reaches beyond the slice.
- `challenge` — a documented choice, such as an intentionally unbounded cache, that a sourced workload now strains.

## Severity calibration

- `critical` — unbounded growth that exhausts memory on a workload the project runs, or allocation that turns a user-facing operation from interactive to timed-out at the sourced size.
- `major` — a measured or complexity-argued cost a user would notice at the sourced size, such as allocation proportional to input on every request or memory retained across operations without a bound.
- `minor` — a real, counted cost on a hot path whose magnitude at the sourced size is small but not negligible.
