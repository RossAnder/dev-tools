+++
id = "plan-structure"
status = "stable"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["goal-coverage", "approver-sections", "format-conformance", "execution-policy"]
activation = "floor"
slice = "whole-scope"
+++

# plan-structure

## Question

Read as one document, does this plan prove its own goal, carry what its approver reads, and follow the format its executor parses?

## Owns

`goal-coverage` — a goal the `## Context` section states that no Success Criterion verifies, and a criterion that checks only that the tasks ran rather than that the goal holds.

`approver-sections` — a house-format plan missing `## Summary` or `## Success Criteria`, the two sections an approver reads before the tasks.

`format-conformance` — structure that breaks the plan-format contract the `FORMAT CONTRACT` fence carries, and a prose-format plan whose work an executor cannot dispatch until it is restructured into tasks.

`execution-policy` — an `## Execution Policy` field that is missing, holds a value outside the set or range the contract allows, or contradicts the task graph the plan declares.

## Probes — plan

Read the whole plan first, then the fences: `FORMAT CONTRACT` is the only source for section order, sizing, file caps, parallelism and the Execution Policy fields, and `STORE CHECK` is ground truth for what the importer and `tasks check` computed. Judge structure against the fence, never against conventions remembered from other plans. When the `FORMAT CONTRACT` fence is absent or reads `foreign-format`, the plan is foreign-format: say so in the coverage note and judge only whether an executor could act on it.

- Goal coverage: list each goal `## Context` and `## Summary` state, one line each, and map every one to the Success Criteria that verify it. A goal with no criterion is a row naming the goal and the sentence that states it.
- For each Success Criterion, ask what a pass would prove. A criterion satisfied by every task merely having run — "all tasks complete", "the new files exist", "the build passes" with no behaviour named — verifies execution, not the goal; name the goal it should have checked.
- A criterion marked predicted or unverified that nothing in `## After Merge` schedules to be run by hand is a goal left unproven.
- Approver sections: in a house-format plan, confirm `## Summary` and `## Success Criteria` are present and non-empty. Read the header first: a `**Status**` line marks a plan that predates the format, which sets the severity below.
- Format conformance: compare the section order, the header fields and each task's bullet shape with the fence. Report one row per kind of departure, listing every task it affects, never one row per task.
- A plan that lays its work out as prose — phases or paragraphs with no numbered tasks, Files lines or acceptance — is a row suggesting restructuring, naming the sections an executor could not dispatch from.
- Execution Policy: check each field the fence names is present, holds an allowed value and sits inside the stated range. A max-parallel figure above the fence's ceiling is a row; one the task graph can never fill is an advisory, not a defect.
- Compare the policy with the graph: a checkpoint cadence the declared checkpoint markers cannot realise, a commit granularity that names checkpoints when the plan declares none, or a policy the plan's own prose contradicts elsewhere.
- A house-format plan with no `## Execution Policy` runs in per-batch legacy mode; report the absence only when the plan's prose assumes milestone or single-commit behaviour.

## Does not flag

- Confirmations: a goal that is covered, a section that is present and a policy that is valid go in the coverage note, never in a row.
- A foreign-format plan missing house sections, Execution Policy fields or task bullets; with a foreign-format `FORMAT CONTRACT` fence, the only structural row is the restructuring suggestion.
- A departure the plan's `## User Decisions` or an ADR records as deliberate; that is `advisory` with `angle = challenge`.
- A cadence chosen coarser than per-batch. The checkpoint cadence is the user's decision; this lens checks the policy is consistent, not that it is cautious.
- Anything `STORE CHECK` already reports, such as an invalid checkpoint cut, an orphan task or a marker mismatch; those findings are plan-dependencies' to write. Cite the fence in the coverage note.
- Per-task executability: Files-line parity, acceptance that cannot run, task sizing and decisions left to the executor belong to plan-executor.
- Dependency edges and checkpoint placement belong to plan-dependencies; sections or tasks that contradict each other belong to feasibility.
- Cited files, symbols and behavioural claims that do not match the tree belong to plan-grounding.
- Wording, tone and heading style that change nothing an approver or executor does; speculative restructurings of a plan that already conforms.
- Sections outside the reviewed plan, and research or exploration appendices, which the format leaves free-form.

## Edit forms

- `add` — a missing Success Criterion for an uncovered goal, a missing `## Summary` or `## Success Criteria` section, or a missing Execution Policy field. Anchor on the adjacent heading or bullet, and write the criterion as a check that fails today.
- `change` — a criterion that checks only that tasks ran, rewritten to check the goal; a policy value outside its allowed set or range; a section heading out of the contract's order or spelling. `anchor_old` is the exact text under `plan_section`.
- Never write an edit adding a `Checkpoint after` bullet: `tomlctl tasks render` derives it from the checkpoint markers.
- A prose-format plan is never restructured by cleanup; the restructuring is a defect with `fix_shape = "needs-plan"` or an advisory, because it needs decisions about task boundaries.

## Advisory angles

- `direction` — a foreign-format plan that would gain from conversion to the house format; name what the conversion would buy the executor.
- `risk` — a goal that only an After Merge step can verify, or a policy setting the graph cannot fill, where the cost is lost bisectability or idle parallelism rather than a broken run.
- `challenge` — a documented decision that departs from the format; name it in `challenges`.

## Severity calibration

- `critical` — a stated goal of the plan that no criterion, task acceptance or After Merge step checks at all, so the plan can finish green with the goal unmet.
- `major` — an uncovered secondary goal, a criterion that checks only that tasks ran, a missing approver section in a plan with no `**Status**` header line, a format departure that silently changes how a flow reads the plan, such as a dropped `## Dependency Graph` heading that downgrades every later review to foreign-format, or an Execution Policy value outside its allowed set or range.
- `minor` — a missing approver section in a plan whose header still carries a `**Status**` line, a format departure an executor reads past, or a policy field absent where the default matches the plan's intent.
