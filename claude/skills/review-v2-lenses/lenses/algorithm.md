+++
id = "algorithm"
status = "stable"
families = ["perf"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["algorithmic-complexity", "redundant-work"]
activation = "floor"
slice = "per-file"
methods = ["measure", "sweep"]
v1_category = "algorithm"
research_domain = "algorithms and data structures"
+++

# algorithm

## Question

Does this code do more work than its input requires — a complexity class above what the access pattern needs, a data structure that fights how it is read, or the same result computed again where it could be reused — at an input size this code actually sees?

## Owns

`algorithmic-complexity` — super-linear work over an input that grows: a nested scan, a linear lookup inside a per-item loop, a sort or a copy repeated per element, a quadratic string or list build, and a data structure whose operations are the wrong complexity for how the code reads it, such as a list searched by key or a sorted structure rebuilt on every insert.

`redundant-work` — the same computation repeated where its result could be reused: an expensive value recomputed inside a loop whose inputs do not change, a parse or traversal done twice on one input, a derived value rebuilt on every access instead of once when its sources change, eager work whose result most paths discard, and a computed result that a memo would absorb but nobody has added.

## Probes — perf

Name the workload before reading for cost, as the measure method's first step requires: the operation, the input that drives it, and a size sourced from the code, its tests, its fixtures, the `PROFILE BASELINE` fence or a documented limit. Without a `PROFILE BASELINE`, a defect needs the complexity rung — the asymptotic cost multiplied out at that real size — or it is advisory with `angle = risk`.

- **Loops over growing input.** For each loop whose trip count follows the input, list what runs inside it. Flag a linear search, a membership test on a list, a `contains` on a vector, a regex compiled per iteration, a collection cloned or sorted per iteration, and a call whose own cost is linear in the same input. Multiply it out at the sourced size.
- **Access-pattern fitness.** For each collection the slice builds, list how it is read: by key, by position, in order, by range, by membership. Flag a structure whose read operations cost more than the alternative's for that mix — a list scanned by key where a map would serve, a map iterated in order where a sorted structure or a single sort would serve, repeated removals from the front of an array.
- **Accumulation.** Find strings, lists and buffers built by repeated concatenation or insert-at-front, and check whether each step copies what came before.
- **Repeated computation.** For each pure or effectively pure call whose arguments are loop-invariant or unchanged between calls, find whether its result is reused. Flag a parse, a traversal, a hash or a format repeated over the same input on one path.
- **Lazy versus eager.** Find work done up front whose result only some paths use, and work done lazily on every access that could be done once. Name the path that pays and how often it runs.
- **Derived state.** Where a value is derived from others and re-derived on each read or each update, check whether the derivation is cheaper than tracking its sources, and whether a change to one source re-derives everything.
- **Memoisation.** For a repeated expensive computation, check whether a memo exists, whether its key captures every input, and whether it ever hits. Flag the missing or never-hitting memo by its cost. Repeated reads of unchanged stored data are the data-access lens's caching concern, and a key that misses an input, so a stale result is returned, is a correctness defect.

When one pattern recurs across sites, the sweep method enumerates them, and the finding carries every site under `instances`. A finding names the input, its sourced size, the complexity or repeat count, and the cost at that size.

## Does not flag

- A confirmation. A loop you checked and found linear goes in the cell's coverage note, never in a finding.
- A complexity argument with no input size, or a size the code never reaches: a quadratic pass over a list bounded at ten items is not a finding. Without a sourced size and no `PROFILE BASELINE`, raise it as advisory with `angle = risk` or leave it out.
- Expressiveness and correctness of data-shape design — illegal states made representable, discriminated unions, newtypes, redundant fields. How a type models the domain belongs to the type-design lens; keep findings here framed around access-pattern fitness, complexity class or recomputation cost.
- A claim resting on a comment about cost or input size that you did not verify against the code or its callers.
- A choice a comment, benchmark, decision record or CLAUDE.md line documents as deliberate — a simple scan kept for readability on a small bounded input, an eager computation kept for predictability. A disagreement is advisory with `angle = challenge`, naming the decision.
- An optimisation that trades readability for negligible gain, or a speculative cost on a path that runs once per process, once per error, or only in a debug or one-shot migration path, unless you show it runs often enough to matter.
- Allocation, boxing and dispatch overhead on a path whose complexity is right; that is the memory-runtime lens. Serialisation and encoding cost is data-shape's, query shape is data-access's, and process and filesystem cost is io-process's.
- Anything the `LINT BASELINE` already reports, and work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.

## Edit forms

- `change` — a list membership test or keyed scan replaced by a set or map built once in the same function; a loop-invariant computation hoisted out of its loop; a repeated sort, clone or regex compile moved to where it runs once; accumulation by repeated concatenation replaced by a builder or a single join.
- `add` — a local memo or a pre-sized collection where the edit stays within the slice and the result is provably reused.
- `remove` — a duplicate traversal or parse whose result an earlier step already holds, citing where.

A fix that changes a public signature, a shared data structure's type or a cache that outlives the call is advisory, not cleanup.

## Advisory angles

- `risk` — a super-linear path whose input size you could not source, stated with the measurement or size that would make it a defect.
- `alternative` — a different algorithm or data structure for the access pattern, cited from a benchmark, a paper or the library's documented complexity, with what adopting it changes.
- `challenge` — a documented choice of a simple structure or eager computation that the change now runs at a size the decision did not weigh.

## Severity calibration

- `critical` — a complexity blow-up on a common workload that makes the operation time out, exhaust memory, or grow from interactive to minutes at a size users reach.
- `major` — a complexity class or repeated computation that costs a user-noticeable multiple of the necessary work at a sourced, realistic input size.
- `minor` — excess work that matters only at the upper end of the sourced sizes, or on a path that runs per operation but not per item.
