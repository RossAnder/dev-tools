+++
id = "plan-premortem"
status = "experimental"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["premortem-failure"]
activation = "opt-in"
slice = "whole-scope"
+++

# plan-premortem

Experimental — weak evidence. Prospective hindsight is a planning practice with little direct evidence for model reviewers; this lens runs only when a project or user opts in, and its yield is judged from `review stats` before it is promoted.

## Question

Assume this plan has shipped and failed: what is the most plausible story of how it failed, and which task, decision or omission does that story run through?

## Owns

`premortem-failure` — the most plausible ways the shipped plan fails, each argued backwards from the failure to the task, decision or omission that causes it, and grounded in the plan text and the code it changes.

## Probes — plan

Read the whole plan, then write the failure before looking for its cause.

- State the outcome the plan promises in its Summary and Success Criteria, then assume it did not happen. Write two or three distinct failure stories in one sentence each, from different directions: the build or tests went green but the feature does not work for a real user; a task could not be finished and the flow stalled; the change shipped and broke something outside the plan; the work landed but nobody uses it.
- For each story, trace backwards: which task produced the failure, which decision made it likely, or which piece of work no task does. Stop at the earliest point the plan could have prevented it.
- Ground every step of the trace in the plan text or the code: quote the task, the decision or the success criterion it passes through, and read the code the story depends on. A story you cannot ground is dropped, not reported.
- Prefer stories that cross tasks, where each task is correct alone and the plan fails anyway: a format one task writes and another task reads differently, a success criterion that every task satisfies while the goal is missed, a step that works in the plan's own repository and nowhere else.
- Ask what the Success Criteria would show if the story came true. A failure the criteria would still pass is the most valuable finding this lens makes.
- Keep the best one to three stories; more than that is a sign the stories are generic.

## Does not flag

- Confirmations: a story you traced that the plan already prevents is a coverage-note line.
- Generic failure modes any plan could suffer — scope creep, under-estimation, unclear requirements — with no task or decision in this plan that the story runs through.
- A failure another floor lens already reports from its own angle: a missing dependency edge, a file missing from a Files line, a stale citation, a missing checkpoint after a migration. Cite the existing finding in `same_as` rather than writing a second row.
- Decisions the plan records deliberately, unless the story shows a consequence the record did not weigh; then it is advisory with `angle = challenge`.
- Stories that depend on events outside the plan's control and the project's deployment, and speculative stories with no grounded step.
- Rewrites of the plan's approach offered without a failure story behind them; alternatives belong to contrarian.

## Edit forms

Edits are to the plan text, with `plan_section`, `anchor_old` (occurring exactly once under that section) and `anchor_new`.

- `add` — a success criterion that would fail if the story came true, beside the criterion it slips past.
- `add` — a Risks entry naming the failure story and its mitigation, when the plan has no line for it.

A story whose prevention needs a new task, or a change to a task's design, is a defect with `fix_shape = needs-plan` when it is grounded end to end, and advisory otherwise.

## Advisory angles

- `risk` — the default for a grounded story whose likelihood rests on a fact the plan does not settle; name that fact and the cheapest step that would settle it.
- `challenge` — a recorded decision the story runs through, naming the decision in `challenges`.

## Severity calibration

- `critical` — a grounded story in which the plan, executed exactly as written, ships broken or loses data, and the Success Criteria still pass.
- `major` — a grounded story with a nameable trigger in which the plan misses its stated goal or breaks something outside its scope.
- `minor` — a grounded story with a narrow trigger, or one the plan would catch late but before release.
