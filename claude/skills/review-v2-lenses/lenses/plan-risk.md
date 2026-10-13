+++
id = "plan-risk"
status = "stable"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["rollback-adequacy", "risky-task-checkpoints", "back-compat", "scope-realism"]
activation = "candidate"
slice = "whole-scope"
+++

# plan-risk

## Question

If a task in this plan goes wrong, can the declared checkpoint policy roll it back cleanly, who outside the plan does it break, and can the tasks as sized deliver the scope at all?

## Owns

`rollback-adequacy` — a failure the declared checkpoint policy cannot undo: a task whose effects survive a revert of its commit (a migration already applied, data rewritten in place, an external system called, a file outside the repository written), or a failure that would only surface after later tasks have built on it inside the same checkpoint group.

`risky-task-checkpoints` — a migration, schema change or public-API change that no checkpoint follows directly, so the next commit boundary also carries unrelated work and the risky step cannot be bisected or reverted on its own.

`back-compat` — existing users, stored data or callers the plan breaks without a task handling them: a changed file format with no reader for the old one, a renamed command, flag or config key with no alias or migration note, a public signature changed under external callers, a stored value the new code no longer accepts.

`scope-realism` — scope or effort the plan's tasks cannot deliver: a goal no set of tasks reaches, an effort total out of proportion to what the tasks describe, or a single task carrying work the rest of the plan assumes is done.

## Probes — plan

Read the Execution Policy first and judge every risk against the cadence it declares. The cadence is the user's decision: review where checkpoints fall, never whether there are enough of them.

- For each task, ask what a revert of its commit leaves behind. Migrations, data backfills, files written under the user's home or another repository, calls to external services and published artifacts all survive a revert; a task producing one needs a rollback step, a forward-fix plan or an explicit note that it is irreversible.
- Find every migration, schema change, stored-format change and public-API or CLI-surface change. Check a checkpoint follows it directly, before unrelated tasks join the same commit. Under a `single` policy, any such task in the middle of the flow is a finding; under `milestones`, a risky task buried inside a large group is.
- Walk each checkpoint group as the unit a rollback restores to. A group whose failure would be discovered only by a task in a later group means the restore point is already past the damage.
- For each surface the plan changes — a file format, a command, a flag, a config key, an exported type, a wire shape, a stored record — find who reads or calls it today outside the plan's scope: other repositories, installed binaries, user configuration, data already on disk. A change with no compatibility task, alias, version gate or migration is a back-compat break.
- Check that the After Merge and Risks sections name the steps an existing user must take; a break the plan accepts deliberately still needs its migration step written down.
- Compare the Summary's and Success Criteria's promises with what the tasks build. A promised outcome no task produces, or a task list whose effort total is far below the work its Details describe, is a scope-realism finding; name the missing work.
- Performance and security risks the plan introduces without any task addressing them belong here only as the risk to the rollout; the defect itself belongs to the security or perf lenses.

## Does not flag

- Confirmations: a risky task with its checkpoint in place, or a format change with its migration task, is a coverage-note line.
- The checkpoint cadence itself. Never recommend per-batch commits, or more checkpoints, because the plan chose `milestones` or `single`; the cadence is a recorded user decision.
- Dependency-edge correctness and checkpoint-cut validity, which plan-dependencies owns with the `STORE CHECK` fence; this lens judges where risky tasks sit relative to the cuts, not whether the cuts are valid.
- API currency, deprecations and advisories against pinned dependencies; external-validity owns those.
- Breaks the plan records as accepted in User Decisions or Risks, with the affected users named; challenge one as advisory with `angle = challenge` only when the record is missing a migration step it needs.
- Theoretical rollback scenarios with no task that produces them, and generic "add a feature flag" or "add monitoring" advice with no named failure.
- Callers and consumers inside the plan's own scope that a task already updates.
- Effort estimates that are off but still land within the plan's own sizing rules; per-task sizing is plan-executor's.
- Risks outside the plan's scope, and anything the `FORMAT CONTRACT` fence already reports.

## Edit forms

Edits are to the plan text, with `plan_section`, `anchor_old` (occurring exactly once under that section) and `anchor_new`.

- `change` — add a checkpoint marker directly after a risky task in the Execution Policy's checkpoint list, when the cadence is `milestones` and the cut is a valid topological cut.
- `add` — a migration, alias or compatibility step to the task that breaks a surface, or an After Merge or Risks line naming the step an existing user must take.
- `add` — a rollback or forward-fix note to a task whose effects survive a revert.

A missing compatibility layer or a scope gap that needs its own task is a defect with `fix_shape = needs-plan`.

## Advisory angles

- `risk` — a rollback or compatibility hazard whose likelihood depends on a deployment fact the plan does not state, such as whether other machines run the old binary.
- `direction` — a change to the rollout order or the checkpoint cadence that would reduce risk, offered for the user to weigh, never asserted as a defect.
- `challenge` — a recorded decision to accept a break, when the evidence suggests the affected users are wider than the record assumes.

## Severity calibration

- `critical` — the plan, executed as written, loses or corrupts existing data, or leaves users with no path back: an irreversible migration with no checkpoint before it and no forward fix, or a stored format the new code cannot read with no migration.
- `major` — a real break or an unrecoverable failure with a nameable trigger: a renamed public surface with external callers and no alias, a risky task sharing a commit with unrelated work, a promised outcome no task delivers.
- `minor` — a narrow break or a recoverable rollback gap: an internal caller outside the scope, a missing After Merge note for a step the user would find, an effort total modestly short of the work.
