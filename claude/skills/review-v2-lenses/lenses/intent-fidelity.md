+++
id = "intent-fidelity"
status = "stable"
families = ["code"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["criteria-fidelity", "decision-fidelity"]
activation = "opt-in"
slice = "whole-scope"
methods = ["trace"]
v1_category = "completeness"
+++

# intent-fidelity

## Question

Does the work this flow built do what the user asked for: what its brief says the work is for, what it must not touch and how success is judged, and every decision the user stated or confirmed?

## Owns

`criteria-fidelity` — built code that works against the brief's Intent, does something the brief's Non-goals rule out, or leaves one of its Success criteria unmet where the criterion names behaviour the scope's code produces. The yardstick is the brief as the user approved it, not a lens's taste.

`decision-fidelity` — built code that departs from a decision whose provenance is user-stated or user-confirmed: a different option from the one the user picked, an option the decision recorded as rejected, or a decision applied to some of the files and tasks it binds and not to the rest.

## Probes — code

This lens runs only on a review bound to a v2 flow: the flow is the one your assignment's `--slug` names, and its records are `docs/flows/<slug>/brief.md` and `.claude/flows/<slug>/plan.toml`. When either is missing, say so in the coverage note and write no rows.

- **Read the yardstick first.** Read `docs/flows/<slug>/brief.md` in full and list, one line each, its Intent, every Non-goal and every Success criterion. Then read the decisions the user owns:

  ```bash
  tomlctl items list .claude/flows/<slug>/plan.toml --array decision --where-in provenance=user-stated,user-confirmed
  ```

  Skip any row marked `vetoed` except to read what the user chose in its place. Each row's `decision` is the instruction, `rejected` the options turned down, and `binds` what it applies to: a path, a directory reaching every file under it, or an all-digit task id.
- **Follow each decision's binds into the code.** For a task id, read the task's files and the commit that landed it:

  ```bash
  tomlctl tasks show <ids> --slug <slug> --select id,title,files,commit
  ```

  Read the bound files inside the scope, and the task's commit with `git show` when the scope is a range that holds it. Trace, with the `trace` method, the behaviour the decision governs to the code that produces it, and judge whether that code does what the decision says, in every place the decision binds. A bound file outside the scope is read for context only, never anchored on.
- **Hold each criterion against the code.** For each Success criterion that names behaviour this scope builds, find the code that delivers it and ask whether it does, for the inputs and states the criterion names. A criterion that rests on an attended step, an observation or a later milestone is out of reach: name it in the coverage note.
- **Check the Non-goals.** For each Non-goal, search the scope for work it rules out: a file, setting, dependency or behaviour the brief said this flow would leave alone.
- **Tell a divergence from a stale decision.** When the code departs from a user decision, read the commits, notes and code around it for a reason. Code that drifted from a decision still sound is a defect against the code. Code that departed because the decision met something it did not foresee, such as a constraint the build hit or a premise the code shows false, is the sign that the decision itself is now wrong: that is an advisory question for the user, never an edit to the decision or the brief.

Every finding cites both sides: the brief line or decision id it holds the code against, and the `file:line` of the code. A defect's `failure_scenario` says what the user will see or get that differs from what they asked for.

## Does not flag

- A confirmation. A criterion the code meets and a decision it honours go in the coverage note, never in a row: "decisions: 9 read, 9 honoured; criteria: 6 in scope, 6 met".
- Decisions whose provenance is agent-assumed: arguing for or against those is the contrarian lens's decision-challenges.
- Whether the plan's criteria cover its goals, judged at plan time: that is the plan-structure lens's goal-coverage. This lens holds the built code against the criteria as written.
- How the UI looks or behaves in a state, which is the ui-visual and interaction lenses' to judge from the captures; this lens asks only whether it is the layout or interaction the user chose.
- Correctness, security or test gaps in code that does what the user asked; those belong to their own lenses.
- Work the brief schedules for a later milestone that this scope does not reach, and a criterion the flow records as deferred.
- A divergence the user approved after the decision: a later user-stated decision, a recorded veto, or a handover the user accepted. Cite it in the coverage note.
- Wording and naming in code that the brief and the decisions leave open.

## Edit forms

intent-fidelity writes defects and advisories; its cleanup is rare and never changes behaviour.

- `change` — a user-visible string, label or help text that names an option the user rejected while the behaviour behind it is the one the user chose.
- `remove` — dead code left behind by an option the user rejected, which no path reaches, cited with the search that shows it.

Never write an edit to the brief, to `plan.toml` or to a decision: a decision that looks wrong goes to the user as an advisory.

## Advisory angles

- `challenge` — the code departed from a user decision for a reason the decision did not foresee, so the decision itself may now be wrong; `challenges` names the decision by `.claude/flows/<slug>/plan.toml` and its id, and `rationale` gives the evidence and the question the user must answer.
- `probe` — a decision or criterion whose wording admits two readings, where the code follows one and the user may have meant the other; name both readings.
- `risk` — a criterion this scope's code cannot be held to without running or observing something the cell may not run; name what would settle it.

## Severity calibration

- `critical` — the built work does the opposite of a user decision or crosses a Non-goal in a way the user would have to undo, such as a dependency, process or setting the brief ruled out.
- `major` — a Success criterion unmet by code that claims to deliver it, or a user decision honoured in some of the places it binds and not in others.
- `minor` — a departure the user would notice but that leaves the chosen behaviour intact, such as a label or default naming the rejected option.
