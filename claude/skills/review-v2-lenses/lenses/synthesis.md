+++
id = "synthesis"
status = "stable"
families = ["code", "perf", "plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["emergent-defects", "coverage-blind-spots", "cross-cutting-concurrency"]
activation = "opt-in"
after_wave = 1
slice = "whole-scope"
methods = ["trace"]
v1_category = { code = "quality", perf = "concurrency" }
+++

# synthesis

## Question

Looking across every cell this run has sealed, which defects appear only when two parts are taken together, and what did every cell miss?

## Owns

`emergent-defects` — a defect no single cell could see because it lives between slices: two changes that are each correct and wrong together, a contract one subsystem changed and another still assumes, findings in separate cells that are one root cause, an ordering between components that no cell held both ends of.

`coverage-blind-spots` — what every cell missed, judged from the run's digest against the scope: a changed file no cell read closely, a risk the change carries that no composed concern or probe owned, a cell sealed as covered whose note shows it never reached the part of its slice that changed.

`cross-cutting-concurrency` — concurrency effects across subsystems that no single-subsystem review sees: lock ordering across acquisitions in different modules, the combined task or process count of several spawn points under load, independent I/O spread across files that could be joined at a higher level, and shutdown order across components.

## Probes — code

Read the run first, then the scope. Fetch the run's digest with `review digest` for the run your assignment names, and the finder projection of the run's rows with `review list` over the whole scope. Read the cells' coverage notes and the findings' summaries; do not re-verify a finding another cell raised, and do not re-run a concern a cell already covered.

- Group the run's findings by root cause. Two or more findings in different cells that one change would fix are one emergent finding naming them all; the individual rows stay as they are.
- For each pair of slices that share an interface (a type, a file format, a store, a command line, a wire shape), did the change alter one side? Trace the other side and check it still holds; no cell had both ends in its slice.
- Does a finding in one cell imply a defect in another cell's slice that the other cell did not report? Follow it there.
- Read the scope's diff against the digest's coverage: which changed files and hunks does no cell's note mention? Read those first; a defect found there is a blind-spot finding.
- Which risk does the change carry that no composed concern owns, given the signals the survey recorded? Name the concern or the probe question that should have covered it.
- Is a cell marked covered whose note, read against its slice, never reached the changed code or ran no sweep?

## Probes — perf

The cross-cutting concurrency review: look for what no single perf cell could see, once their findings are merged.

- Lock ordering across modules: list every lock acquired while another is held, across the slices, and check that every path takes them in one order. A path that inverts the order is a deadlock risk to trace end to end.
- Spawn points: count the tasks, threads or child processes each spawn point can start, and what bounds them. Do several bounded spawn points multiply into an unbounded total under the workload the tool sees?
- Independent I/O spread across files that a single handler awaits in sequence: could it be joined at the caller, and does the profile or a complexity argument show it matters?
- Shutdown order: do components stop in dependency order, so nothing writes to a closed channel, a dropped store or an exited child?
- Perf findings in separate cells that share a root cause, such as one repeated read feeding several slow paths: one emergent finding at the shared cause.

## Probes — plan

- Do findings from different cells, such as a grounding finding and an executor finding, trace to one gap in the plan? Raise the gap once, at the plan section that causes it.
- Do two tasks each look sound in their own `plan-executor` cell but conflict together: the same file edited with incompatible intents, a premise one task establishes and another task's Detail contradicts, an ordering the dependency graph allows but the data flow does not?
- Which success criterion, risk or approach subsection did no cell examine? Read it against the tasks.
- Does the digest show a plan-wide pattern, such as every executor cell reporting the same undecided choice, that one plan-level decision would settle?

## Does not flag

- Anything a single cell's slice and concerns could have found and did: a re-statement of another cell's finding is never a finding, and agreement with one is a coverage note.
- Confirmations: a cross-cell interface traced and found sound goes in the coverage note.
- A cell's coverage note taken as proof that code is clean, or as proof that it was read; check the note against the slice.
- Disagreement with another cell's verdict or severity; contested findings belong to the audit, not to this cell.
- Gaps a waiver in the digest already records with a reason, unless the reason no longer holds.
- Speculative interactions with no traced path between the two parts, and concurrency hazards with no shared state or ordering the code establishes.
- Interactions outside the scope the change neither touched nor relied on.
- A consolidation of findings whose only gain is a tidier report.

## Edit forms

- `change` — one edit that fixes a shared root cause, listing every site from the cells' findings in `instances`, when the root cause is behaviour-preserving cleanup the individual cells reported piecemeal.

## Advisory angles

- `risk` — an interaction between parts that the traced path suggests but a runtime fact the cell cannot see decides, such as the real concurrency level; name the fact.
- `direction` — a blind spot that recurs because the catalogue has no concern for it; name the concern or probe question a later run should compose.
- `alternative` — a structural change, such as moving the shared interface or merging two components, that removes a class of emergent defect but needs a decision.

## Severity calibration

- `critical` — an emergent defect that ships broken behaviour or loses data, such as two writers that corrupt a shared store together, or a deadlock reachable on a common path.
- `major` — an interface mismatch between slices with a nameable trigger, a blind spot holding a real defect, or a spawn total that exhausts a resource under the tool's real workload.
- `minor` — an emergent defect with a narrow trigger, or a concurrency interaction reachable only under an unusual load or shutdown sequence.
