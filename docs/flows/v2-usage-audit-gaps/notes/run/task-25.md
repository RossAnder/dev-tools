Status: done — write-and-prove teaches baseline gates and proofs, blocking handovers, sourced outside facts with pin existence checks, the self-edit order and the per-milestone UI call

```toml run-note
files_touched = ["claude/skills/plan-v2-engine/references/write-and-prove.md"]

[[deviations]]
what = "A version or image-tag pin's existence check is steered to a --gate-probe on the gate that consumes the pin, and to a premise only when its check reads locally"
why = "run-milestone.md section 3 refuses a premise line that makes a network call, so a docker manifest inspect premise would stop the milestone at its pick; a gate probe runs at Prove under the orchestrator's own rights"
means = "detail (3): a version or image tag pin gets an existence check as a premise or gate probe"

[[deviations]]
what = "The null-control proof is recorded as '<gate> :: null-control :: <task-id>', naming the task"
why = "milestone A shipped --baseline-proof with the null-control verdict carrying its task id (claude/skills/tomlctl/references/plan.md, plan milestone update table), and plan check treats a null-control proof without a task as missing"
means = "detail (1): the proof is null-control"

[[checks]]
command = "grep -q -- '--baseline-gate' claude/skills/plan-v2-engine/references/write-and-prove.md && grep -q 'ready_check' claude/skills/plan-v2-engine/references/write-and-prove.md && grep -q 'manifest inspect' claude/skills/plan-v2-engine/references/write-and-prove.md"
result = "pass"

[[checks]]
command = "tomlctl items list .claude/flows/v2-usage-audit-gaps/tasks.toml --array milestones --select id,elaboration,baseline_gates,baseline_proofs --lines"
result = "pass"

[[checks]]
command = "cargo test --manifest-path tomlctl/Cargo.toml --lib -- cli::dispatch::tests"
result = "not-run"
```

## What changed

`claude/skills/plan-v2-engine/references/write-and-prove.md` (429 to 496 lines, under the 600-line ceiling):

- Section 1, Milestones: a **Self-edits** paragraph ordering engine reference, agent and Workflow edits into a milestone `--after` the one whose external gate installs the binary, citing run-milestone.md's snapshot section and stating that linked agent and SKILL.md edits take effect from the next session. A baseline-gate paragraph: mark comparison or snapshot gates with `--baseline-gate`, proved at Prove on the unchanged tree or by a null-control accept entry on the task that creates the tool, with an example entry; `plan/baseline-gate-unproved` named.
- UI milestones: the UI or not-UI call of every milestone is recorded as a decision and set with `--ui yes|no` on `plan milestone add` or `update`; `plan/ui-capture-missing` named.
- Handovers: a **Blocking handovers** paragraph with `blocks` and a read-only `ready_check`, an example record, the launch-args withholding and the pick-time check cited from run-milestone.md section 2, and the rule that neither `note` nor `ready_check` holds a secret's value.
- An **Outside facts** paragraph: an outside fact the orchestrator writes into a row, a handover action or a hand fix cites a fetched source covering the same object, or is fetched or sent to an investigator first; pins get an existence check, for example `docker manifest inspect <image>:<tag>` as a gate probe.
- 4a: the baseline and UI warnings and how they clear.
- 4d: a **Baseline proofs** step after the gate probes: list the milestones' baseline gates, record null-control proofs naming their task, dispatch the `verification` agent once (one-shot, no name or model, keep_going) for every other baseline gate, record `--baseline-proof '<gate> :: <verdict>'`, put a `fail` to the user before approval, and on acceptance record a user-confirmed or user-stated decision whose text contains the gate's exact command, with the record and its `plan decide` call.

Every verb and flag was checked against `tomlctl plan milestone add --help`, `update --help` and claude/skills/tomlctl/references/plan.md and plan-store.md. The dispatch-tests check is marked Run by: gate, so this worker left it to the gate.
