+++
id = "plan-dependencies"
status = "stable"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["missing-edges", "edge-correctness", "checkpoint-coherence"]
activation = "floor"
slice = "whole-scope"
+++

# plan-dependencies

## Question

Does the task graph carry every ordering the work actually needs, does each stated edge point the way the data flows, and is each checkpoint group a coherent increment that builds on its own?

## Owns

`missing-edges` — a task that consumes what another task builds, or edits a file another task also edits, with no dependency path between the two in either direction. The executor schedules from the edges, so parallel means no path, not a different prose wave; a pair the prose separates but the graph does not is still a missing edge.

`edge-correctness` — a stated dependency edge that is the wrong edge: it names the wrong task, runs against the data flow, or orders two tasks that need no order and so serialises work for nothing.

`checkpoint-coherence` — a checkpoint group that is not a buildable, logically coherent increment, a set of markers that does not form a valid cut of the graph, or a task left outside every marker's closure and so committed only by the final train.

## Probes — plan

Start from the `STORE CHECK` fence in `pack.md`. It carries the output of the task-store dry-run import and of the store's invariant check over the edge set as written: cycles, dangling and duplicate references, same-file pairs with no path, checkpoint cuts that are not downward-closed, tasks outside every checkpoint, and symbols one task introduces that an unlinked task names. Read those classes as ground truth for the edges the plan states, and do not re-derive them. Turn each error-class entry into a defect, citing the class and the task ids. A clean fence is evidence about the edges present, never that none is missing, so spend the cell on what a graph check cannot see. When the fence is absent it says why; walk the graph and the checkpoint closures by hand and say so in the coverage note.

- Data flow: for each task, list what it consumes — a type, a function, a file, a table, a flag, a fixture, a command another task adds — and find the task that produces it. A producer with no path to its consumer is a missing edge. The store's symbol heuristic reads only names the Action introduces; a dependency carried by behaviour, a schema or a generated artifact is invisible to it.
- Shared files: tasks whose Files lines intersect, or whose bodies edit the same file, with no path between them, including pairs the fence did not report because one side omits the file from its Files line.
- Edge direction and necessity: for each stated edge, name what flows along it. An edge with nothing flowing is unneeded serialisation; an edge whose producer is actually the consumer is reversed; an edge naming a sibling task instead of the one that builds the artifact is misdirected.
- Checkpoint groups: with an Execution Policy present, judge broken-state hazards at checkpoint boundaries, not at dependency levels. For each group, ask whether the tree would build and its tests pass if the run stopped right after that group's commit: a type added in one group and first used in a later one is fine; a caller changed in one group and its callee in a later one is not.
- Checkpoint coverage: from the fence's orphan entries, or by walking each marker's closure, report every task outside all of them as one enumerated row.
- Without an Execution Policy, judge broken-state hazards per dependency level instead, since every level is then a commit.

## Does not flag

- Confirmations: an edge or group that holds is a coverage-note line, never a row.
- Anything the fence reports, restated as a judgement of your own; cite the fence entry once and add only what it cannot know.
- Transient breakage inside one checkpoint group; intermediate commits between checkpoints are not required to build.
- The checkpoint cadence itself (single, milestones, per-batch), which is a user decision, and Execution Policy fields as fields; plan-structure judges those. Whether a risky task such as a migration or a public-API change is followed by a checkpoint is plan-risk's.
- An edge the plan documents as deliberate, such as an ordering kept for review convenience; raise it as `advisory` with `angle = challenge` if it costs real parallelism.
- Speculative orderings with no named artifact flowing between the tasks, and style-only remarks on how edges are written.
- Tasks or files outside the plan, and edge rewrites whose parallelism gain is negligible against the risk of reordering.

## Edit forms

- `add` — a missing dependency added to the consuming task's Depends-on line, with `plan_section`, `anchor_old` and `anchor_new`, naming in `edit` what flows along the new edge.
- `change` — a misdirected or reversed edge corrected on the task's Depends-on line, or a checkpoint marker moved to a task that closes a coherent group.
- `remove` — an edge with nothing flowing along it, when removing it changes no other task's closure in a way the plan relies on.

## Advisory angles

- `alternative` — a regrouping of checkpoints or tasks that would give coherent increments with more parallelism, when it reshapes the plan beyond single-edge edits.
- `challenge` — a documented ordering or grouping decision the graph argues against; name the decision in `challenges`.

## Severity calibration

- `critical` — an error-class entry in the store check (a cycle, a dangling or duplicate reference, an invalid cut), or a missing edge that lets a consumer run before the task that builds what it needs, so the run fails as scheduled.
- `major` — a same-file pair with no path, a reversed edge, or a checkpoint group whose commit leaves the tree unbuildable.
- `minor` — an unneeded edge that only costs parallelism, or orphan tasks committed late without breaking any checkpoint's build.
