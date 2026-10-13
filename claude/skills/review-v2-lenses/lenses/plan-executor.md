+++
id = "plan-executor"
status = "stable"
families = ["plan"]
mode = "executor"
produces = ["defect", "cleanup", "advisory"]
owns = ["executor-decisions", "files-parity", "runnable-acceptance", "task-sizing"]
activation = "floor"
slice = "per-task"
methods = ["dry-run"]
+++

# plan-executor

## Question

Holding only what its implementer will hold, can each task in this slice be carried out as written, without guessing a decision, touching a file its Files line omits, or meeting an acceptance check that cannot run or cannot fail?

## Owns

`executor-decisions` — a choice the task leaves to whoever executes it that the task and the plan's preamble do not settle: a name, a signature, a field or enum value, an error shape, an ordering, a default, an architectural placement, or a sentence with two readings that lead to different code.

`files-parity` — a mismatch between the files the task's work creates, edits or deletes and its **Files** line: an edit target the Action, Detail or Acceptance names that the line omits, a listed file the work never touches, a file marked new that already exists, or a file a task with no dependency path to this one also edits.

`runnable-acceptance` — an acceptance command that cannot run as the executor will type it, or cannot fail: a path that will not exist after the task, a flag the tool does not take, a test name the task does not create, a forward probe that already passes today, a falsifier that does not hold on the current tree.

`task-sizing` — a task whose dry run spans clearly more than its effort tag, that could not land in one focused session, or whose per-task file count breaks the plan-format contract without the parts being inseparable.

## Probes — plan

Run the `dry-run` method once per task in the slice, from the context an implementer's dispatch carries: the preamble sections and the task fetched from the store. Read the code each step would read; never edit and never build.

- Walk the Action, then every sentence of the Detail, and say what you would do next: the file you would open, what you would write, the command you would run. A step where you would have to pick between alternatives the text does not choose between is an executor decision; quote both readings.
- A decision counts only when it is load-bearing: a different choice would change behaviour, an interface another task consumes, or what the acceptance checks. A local variable name is not one.
- Build the list of files the work touches from the dry run, not from the task's prose: count the call sites, imports, re-exports, tests and fixtures a signature or schema change drags in. Compare that list with the **Files** line, both ways. A test file the Acceptance names and the line omits is the classic omission, and it matters because the scheduler, the lite-eligibility gate and the rollback all trust the line verbatim.
- For each file the task touches, check whether a task with no dependency path to this one also lists it; parallel scheduling would collide there.
- Read each acceptance command as the executor will type it. Run it only when it is cheap and read-only; judge one that builds, runs a suite or writes by reading it. Check its polarity: a `forward:` line that already holds today is vacuous unless it is a labelled guard, and a `falsifier:` must hold on the current tree.
- An "X passes" acceptance carries an implicit falsifier: name the change to the code that would make it fail, and check the named test would actually bind it.
- Every claim the task makes about current code that the step depends on — a signature, a file's location, a type's fields, what a command prints, which caller exists — is checked at its `file:line` before the step is judged. A false premise that breaks the step is a finding here; a confirmed one is a coverage-note line.
- A task that only works with context its implementer does not receive — the plan's `## Tasks` section, its Exploration or Research Notes, another task's body — when its Detail does not name that heading.
- Size: compare the dry run's breadth with the effort tag and the plan-format contract's per-task cap. Name the split when you flag one: which files and which steps go in each part.

## Does not flag

- Confirmations: a task that dry-runs clean, a premise that holds and an acceptance that probes healthy are coverage-note lines, never rows.
- Decisions the preamble settles — in User Decisions, Approach or Execution Policy — even when the task does not restate them; the implementer receives the preamble.
- Implementation detail any competent executor resolves the same way from the surrounding code's idiom; a decision is a finding only when two reasonable executors would diverge on something that matters.
- Whole-plan concerns: missing or wrong dependency edges and checkpoint coherence (plan-dependencies), approver sections, goal coverage, format and Execution Policy conformance (plan-structure), plan-wide alignment, enumeration drift and unmentioned consumers (plan-grounding), and cross-task acceptance contradictions or the whole-plan two-control sweep (the plan-checks cell). A single task's own Files line and acceptance stay here.
- Line-number drift in a citation the task does not depend on to act; that is plan-grounding's alignment sweep.
- A task's design choice the plan records as deliberate; raise it as advisory with `angle = challenge` if it still seems wrong.
- Speculative failures with no step of the dry run that produces them, and style-only preferences about how a task is worded.
- Effort tags that are off by one step but still land in one session.
- Tasks outside the cell's slice, and anything the `STORE CHECK` or `FORMAT CONTRACT` fence already reports.

## Edit forms

All edits are to the plan text, with `plan_section` naming the task's heading, `anchor_old` quoting text that occurs exactly once under it, and `anchor_new` the replacement.

- `change` — add the missing file to the task's **Files** line (and to its Action when the Action should name it), drop a listed file the work never touches, fix a path, flag or test name in an acceptance command, or relabel an acceptance's polarity.
- `change` — settle an executor decision when the plan's own preamble or the code already implies the answer: write the chosen name, signature or value into the Detail.
- `add` — a falsifier or a discriminating `forward:` line beside a vacuous one, anchored on the adjacent acceptance text.

A decision the plan has not made and that needs its author is a defect with `fix_shape = needs-plan`, or advisory when the task still executes either way.

## Advisory angles

- `direction` — a split, merge or reordering of tasks the dry run suggests, when the plan's author has to weigh it against the dependency graph.
- `risk` — a step that executes as written but depends on an environmental fact the dry run cannot confirm, such as a tool version on the executor's machine.

## Severity calibration

- `critical` — the task cannot be executed as written: an acceptance no correct implementation can pass, a step that fails on a false premise, or a load-bearing decision with no basis in the plan or the code.
- `major` — the task executes but lands wrong or unsafely: a file missing from the Files line, an acceptance that cannot fail, a decision two executors would resolve differently in a way another task depends on.
- `minor` — friction that a careful executor works around: a sizing overrun inside one session, a stale flag spelling with an obvious correction, a decision with one sensible answer the text forgot to state.
