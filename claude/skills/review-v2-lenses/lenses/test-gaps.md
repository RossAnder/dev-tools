+++
id = "test-gaps"
status = "stable"
families = ["code", "plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["missing-tests", "weak-assertions"]
activation = "candidate"
slice = "per-subsystem"
v1_category = "testability"
+++

# test-gaps

## Question

Which regression risk does this change carry that no test would catch, and which of its tests would still pass if the code were wrong?

## Owns

`missing-tests` — a behaviour, boundary or regression risk the change introduces or alters with no test aimed at it, and in a plan, a task that changes behaviour with no task writing or naming the test that pins it.

`weak-assertions` — a test that runs the changed code but would pass if it were wrong: it asserts only that no error occurred, only an ordering both old and new values satisfy, only a mock's own return value, or a value too loose to tell right from wrong.

## Probes — code

- Name the regression risk first: what input or state would this change most plausibly get wrong, and what would break for a caller if it did? Then look for the test aimed at exactly that. Coverage of the function in general is not coverage of the risk.
- A fixed bug with no test that fails on the pre-fix code is a gap; the fix can regress silently.
- A new branch, error path or boundary (empty, maximal, malformed, a limit reached) the diff adds: is any test driving it, and does that test assert the branch's distinct outcome?
- For each changed test, apply the falsifier question: if the line under test reverted, which assertion would fail? An assertion no plausible revert would trip is weak.
- Assertions that compare against a value computed by the same code under test, snapshot a whole output without pinning the field the change touched, or only check that a call returned `Ok` or did not throw.
- Tests that mock the collaborator whose behaviour the change altered, so the change never runs under test.
- Non-determinism that makes a test unable to catch the regression reliably: wall-clock time, random order, shared temp paths, network.
- A test named for something other than the behaviour it covers, so its failure would mislead.
- Where the gap needs a seam the code lacks before a test can be written, record the gap here and leave the seam to the seams lens.

## Probes — plan

- For each task that changes behaviour, find where the plan writes or names the test that would fail without it: the task's Acceptance, a sibling task, or an existing test the plan cites. None is a missing test.
- An Acceptance that names a test file no task's Files line creates or edits.
- A task fixing a defect with no regression test task, or with an Acceptance that would pass before the fix.
- Behaviour the Success Criteria promise that no task's test covers.
- Tests the plan schedules after the code they cover in a way that leaves a checkpoint commit with the behaviour changed and untested.

## Does not flag

- Confirmations: a risk that is tested is a coverage-note line, never a row.
- Coverage for its own sake: trivial accessors, plain data structs, generated code, re-exports, and wiring with no branch.
- A test the project's conventions put elsewhere and that exists there; search the tree, including integration test directories, before calling a test missing.
- Speculative risks with no nameable input or state, and style-only preferences about test layout or naming that leave the test's failure meaning intact.
- A gap a comment, ADR or project rule records as deliberate; raise it as advisory with `angle = challenge` if it still seems wrong.
- Untested paths outside the run's scope, and tests the change did not touch for code the change did not touch.
- A test whose cost to write is out of proportion to the risk it pins, such as a full end-to-end harness for a one-line message change.
- Redundant or implementation-pinned tests; removing tests belongs to test-pruning.
- Hidden globals and hard-wired collaborators; seams owns those.

## Edit forms

- `add` — the missing test. `file` and `line` anchor the code under test; `target_file` names the test file, new or existing; `edit` names the input, the action and the exact assertion that would fail on a regression. `old` is the adjacent anchor text in an existing test file, omitted for a new one.
- `change` — strengthen a weak assertion. `old` is the exact assertion text; `edit` gives the assertion that binds the changed behaviour.
- Plan family: `change` on the task text, with `plan_section`, `anchor_old` and `anchor_new`, adding the test file to the task's Files line and the failing-then-passing check to its Acceptance.

## Advisory angles

- `risk` — a regression risk whose test needs infrastructure the project does not have, such as a concurrency harness or a fault-injecting filesystem.
- `direction` — a test strategy shift the evidence suggests, such as property tests for a parser whose defects recur at boundaries.

## Severity calibration

- `critical` — the change touches data loss, security or persistence paths and nothing tests the failure it could cause, or a plan task changes such a path with no test anywhere in the plan.
- `major` — a fixed bug or new branch whose regression would reach users, with no test that fails on it, or a changed test that would pass against the pre-change code.
- `minor` — a gap on a narrow or internal path, or an assertion that is loose but still catches the most likely regression.
