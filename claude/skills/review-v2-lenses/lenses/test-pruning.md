+++
id = "test-pruning"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["redundant-tests", "implementation-pinned-tests"]
activation = "signal"
apply_when = { signals = ["test-growth", "test-ratio-high"] }
slice = "per-subsystem"
v1_category = "testability"
+++

# test-pruning

## Question

Which tests in this slice could be deleted or merged with no regression going uncaught, and what coverage would each removal give up?

## Owns

`redundant-tests` — a test whose every assertion another test already makes against the same behaviour on an equivalent input, so deleting it loses no failure the suite would otherwise report; a parameterised case that repeats another case's equivalence class; a unit test whose subject is driven identically by an integration test that asserts the same outcome.

`implementation-pinned-tests` — a test that re-asserts the implementation rather than the behaviour: it mirrors the code's own computation to produce its expected value, asserts on private structure, call counts or call order a caller never observes, or pins incidental output (exact whitespace, map order, a full snapshot) so that a correct refactor fails it and a real defect might not.

## Probes — code

Every removal is argued from what the suite would still catch. Before writing a row, name the behaviour the test covers and find the test that would still fail if that behaviour regressed; when none exists, the test is not redundant.

- **Duplicates.** Group tests by the function and outcome they assert. Within a group, compare inputs: two inputs from the same equivalence class with the same assertion are one test. Search integration and end-to-end suites too; a unit test is redundant only when another test pins the same outcome as tightly.
- **Re-asserted implementation.** An expected value computed by calling the code under test, or by copying its algorithm into the test, tells you nothing when both are wrong together.
- **Pinned incidentals.** Assertions on log text, map or hash-set order, exact float formatting, whole-output snapshots or mock call sequences, where the behaviour a caller depends on is narrower than what is pinned. Name the narrower assertion that would replace it.
- **Tests of the framework.** A test that exercises only a library's or language's own guarantee, with none of the project's logic in the path.
- **Dead subjects.** A test whose subject is unreachable from production code; check with a whole-tree search before claiming it.
- When the health signals that activated this lens name a stack, start with its fastest-growing test files and work down.

Each removal row states, in `edit`, the coverage given up: the behaviour the test pinned and the test that still pins it, or that nothing does and why that is acceptable. A row that cannot name the surviving test is either a merge (`change`) or not a finding.

## Does not flag

- Confirmations: a test that earns its place is a coverage-note line, never a row.
- A regression test written for a specific fixed defect, even when another test overlaps it, unless the overlapping test fails on the same pre-fix code.
- Tests the project's conventions require, such as one contract test per public verb or a snapshot the project uses as its golden output, and tests a comment or decision record marks as deliberate; challenge them as advisory instead.
- Overlap between a fast unit test and a slow end-to-end test that protects a different failure mode, such as wiring, serialisation or process boundaries.
- Style-only rewrites of a test that leave what it asserts unchanged, and speculative redundancy you have not confirmed by reading both tests.
- Missing tests and weak assertions on behaviour the suite should cover; those belong to test-gaps.
- Tests outside the run's scope, and a pruning whose saving is negligible against the risk of losing a guard.

## Edit forms

- `remove` — delete a redundant or implementation-pinned test. `old` is the test's declaration line; `edit` names the behaviour it covered and the test that still covers it, or states that the coverage is given up and why. One pattern across many tests is one row with `instances`. Every removal is routed to the verifier, which confirms the surviving guard.
- `change` — merge duplicates into one parameterised test, or narrow an over-pinned assertion to the behaviour callers depend on. `old` is the exact assertion or case list; `edit` gives the replacement.

## Advisory angles

- `direction` — a test-suite restructuring the evidence suggests, such as replacing a family of pinned snapshots with property tests, where adopting it needs a decision.
- `challenge` — a project convention that forces duplicate tests, named with the rule it comes from.

## Severity calibration

- `critical` — none. Pruning does not write critical defects.
- `major` — a test that passes against the defect it claims to guard, because it re-asserts the implementation it tests.
- `minor` — a test whose pinned incidentals have already failed correct changes, or will on any refactor of the subject.
