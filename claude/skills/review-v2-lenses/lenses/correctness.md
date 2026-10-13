+++
id = "correctness"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["logic", "edge-cases", "missing-handling", "error-paths", "races-ordering", "resource-lifetimes", "cross-layer-invariants"]
activation = "floor"
slice = "per-subsystem"
methods = ["trace", "sweep"]
v1_category = "quality"
+++

# correctness

## Question

For which nameable input or state does this code produce the wrong output, crash, leak, or leave the system inconsistent?

## Owns

`logic` — output that is wrong for an input or state the finding can name: an inverted condition, an off-by-one, a wrong operator, a branch that returns the value of its sibling, arithmetic that overflows or truncates on a reachable value.

`edge-cases` — boundary inputs the code accepts but mishandles: empty, single, maximal and duplicate collections, malformed and partial input, CRLF line endings, Windows paths and drive letters, non-ASCII text and byte-versus-character offsets.

`missing-handling` — an input, state or branch the code does not handle at all: an enum variant that falls through, an `Option` or `null` that is never checked, a status or error kind nothing matches. Always a defect, never cleanup.

`error-paths` — failures that are swallowed, mapped to a default, logged and continued, or surfaced as a panic on user-controlled data; partial writes with no rollback, so a failure halfway leaves state no reader expects.

`races-ordering` — output that depends on timing: shared state read then written without holding what the read assumed, lock order that differs across modules, a check-then-act on the filesystem, events or callbacks assumed to arrive in order.

`resource-lifetimes` — files, handles, locks, temp directories and child processes that leak or outlive their owner on an early return, an error branch or a cancellation, and resources used after they were closed or moved.

`cross-layer-invariants` — a change in one layer whose counterparts did not change with it: a field added to a type but not to its decoder, query, wire mirror, serialiser or construction sites; a new variant a match elsewhere does not cover; a renamed key one reader still uses.

## Probes — code

Establish the contract first — tests, types, doc comments, the public surface and any plan or decision record the pack names — then work the boundaries of the slice rather than its middle.

- **Inputs.** For each entry point in the slice, list what it accepts and walk the empty, maximal, malformed and duplicate cases. Check path handling for separators, drive letters, `..`, trailing slashes and case, and text handling for CRLF, a missing final newline, a BOM and multi-byte characters split at a byte offset.
- **Branches.** For every `match`, `switch` and `if` chain on a closed set, name the case it does not handle. For every value that can be absent, find where absence is checked, or show it cannot be absent.
- **Error paths.** Follow each fallible call to where its error lands. Flag an error converted to a default or an empty value, a `catch` that continues as if nothing failed, a panic or `unwrap` on data the user controls, and a sequence of writes whose failure halfway leaves no rollback and no marker.
- **Concurrency and ordering.** Name the shared state and who writes it. Flag a read whose result is acted on after another writer could have run, two locks taken in different orders on different paths, and code that assumes completion order of concurrent work.
- **Resource lifetimes.** For each file, handle, lock, temp directory or child process opened in the slice, trace every early return, `?`, `throw` and cancellation point to where it is released.
- **Cross-layer invariants.** When the change adds, renames or removes a field, variant or key, search the whole tree for its siblings — decoders, queries, wire mirrors, serialisers, construction sites, tests and fixtures — and name each one left behind. The trace method covers the search; the sweep method covers reporting every site of one defect as one finding.

A defect states its `failure_scenario` as the concrete input or state, the path it takes, and the wrong result. When you cannot name the input, it is not a defect: either keep reading until you can, or raise it as advisory with `angle = risk`.

### Context-free variant

A `review-finder-cf` cell runs this lens with no pack over the diff alone. Then read only the changed hunks and the lines they need to be understood; do not load the project's conventions, prior findings or plan. Report only defects whose failure follows from the diff text itself — a broken condition, an unhandled case, an error dropped, a resource leaked — and skip anything whose truth depends on project context you were not given. The point of the variant is an independent read, so do not consult the store's prior findings for the slice before writing.

## Does not flag

- A confirmation. A path you checked and found correct goes in the cell's coverage note, never in a finding.
- A defensive check the language, the framework or a caller already guarantees, or a missing check for a state the types make unreachable — show the guarantee rather than assume it.
- A claim resting on a comment you did not verify against the code: comments go stale, and a finding built on one is refuted by reading.
- Behaviour a comment, test, decision record or CLAUDE.md line documents as deliberate. A disagreement with it is advisory with `angle = challenge`, naming the decision.
- A speculative failure with no reachable input, or a style preference with no wrong output.
- Missing tests for a behaviour — that belongs to the test-gaps lens — and anything the `LINT BASELINE` already reports.
- Work tracked elsewhere: a TODO with an owner, a deferred or dismissed finding in the store.
- A hardening whose gain is negligible against the risk of changing working code.

## Edit forms

Correctness writes defects first; its cleanup is the small edit that accompanies one or stands in for one where behaviour is already right.

- `change` — a misleading condition or name that reads as a bug but behaves correctly; an error message on a handled path that names the wrong case.
- `add` — an assertion or comment stating an invariant a reader needs and the code relies on, where the invariant holds today.
- `remove` — an unreachable branch the types prove dead, cited with the proof.

## Advisory angles

- `risk` — a failure that needs a state you cannot construct from the slice, such as a race whose window depends on deployment, stated with what would make it real.
- `challenge` — a documented deliberate behaviour that produces a wrong result for an input the decision did not weigh.

## Severity calibration

- `critical` — data loss or corruption, a crash on a common input, or a persisted state no later run can recover from.
- `major` — wrong output or a crash on a nameable input users will plausibly send; an error swallowed on a path that writes.
- `minor` — wrong output only on a narrow or unlikely input, or a leak bounded by process exit.
