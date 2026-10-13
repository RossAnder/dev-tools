+++
id = "two-control-probe"
status = "stable"
families = ["plan"]
executor = "cell"
produces = ["defect", "cleanup", "advisory"]
activation = "floor"
+++

# Two-control probe

## Runs

Every runnable acceptance in the plan, probed against the current tree. Static reading endorses broken acceptance commands, because their defects are tool semantics that read as correct on the page, so this check runs them. It covers each task's **Acceptance**, every bullet under `## Success Criteria`, and every read-only `success:` key under `## Verification Commands`. It runs inside the whole-plan `plan-checks` cell beside `falsifiability` and `acceptance-contradiction`, and reads the plan document itself, `## Tasks` included.

1. **Load the method.** The probing method, its traps, the probe helper and the verdict table are the `flow-contract-plan-output-format` skill's `references/acceptance-probing.md`; this check never restates them. Locate it the way `format-contract` locates that skill: take this check's bound path, remove its last three parts (`review-v2-lenses/checks/two-control-probe.md`) and add `flow-contract-plan-output-format/references/acceptance-probing.md`. When that file is missing, invoke the skill and read the reference from the base directory the load reports. Read it in full before probing.
2. **Classify polarity first.** For each criterion decide whether it is forward (describes the tree after the task lands), falsifier (describes the tree before it: "fails on the old values", "goes red until task N") or guard. A `forward:`, `falsifier:` or `guard:` label declares polarity; check the label against the text and probe under the polarity the text describes. Treat any "test X passes" acceptance as carrying an implicit falsifier.
3. **Sort what can run.** Probe only read-only commands. Do not run anything that builds, runs a test suite, writes build output, or writes the working tree (`--fix`, `--write`, a formatter without its check flag, a generator, a migration, `tasks import-plan` without `--dry-run`). Judge a test-delegating criterion by reading the named test instead: the positive control for a test assertion is a read of the test's assertions against the values the task changes.
4. **Build the batch.** Write the helper definition and one `p <task> neg|pos '<command>'` line per control with the Write tool to `payloads/<cell id>.probes.sh` in the run directory. A positive control pipes the input a correct implementation would produce (`sed -n` the real region, apply the described edit in the pipeline, or `printf` the hunk) in place of the file read; it never applies the edit to a file. Label `success:` key probes with the key name in the task column.
5. **Run it once.** One call, `bash <that path>`, from the repository root, stderr not redirected. Every probe prints its own tab-separated line and nothing short-circuits. Never run a probe one call at a time, and never through the `verification` agent, which stops at the first non-zero exit while a healthy negative control exits 1.

## Interpreting the result

Read each line's stdout and stderr columns, not its `rc`; a pipeline's exit status is its last stage's, so a fatal first stage still reports `rc=1` like a healthy miss. The exception is a `success:` key, which is judged by `rc` alone because that is all the `verification` agent judges it by. Apply the reference's verdict table under the polarity from step 2:

- **healthy** — forward: negative control does not hold, positive holds; falsifier: holds today.
- **vacuous** — a forward criterion that already holds today.
- **unsatisfiable** — a forward criterion whose positive control also fails: no correct implementation can pass it.
- **permanently green** — a falsifier that does not hold today, or a "test X passes" whose test asserts nothing the task changes (the suite asserting only ordering, which both the old and the new values satisfy).
- **broken** — `No such file`, `command not found` or `fatal` in the stderr column, or a `success:` key exiting 0 while its criterion does not hold (the bare-count shape).

Pitfalls that change a verdict:

- A `No such file` naming a path a task in the plan marks `(new)`, or a `command not found` naming a binary a task builds or installs, is the expected pre-state of a forward negative control, not a broken command. Judge such a criterion by its positive control.
- A quoting slip in the batch silently runs a different program: inside double quotes an unescaped `$1` expands in the calling shell. When a verdict surprises you, re-read the batch line before believing it.
- A probe the reviewer's own environment cannot run — a tool installed only on CI, a platform-specific binary — is not evidence about the command.
- A criterion that names a working directory needs `cd <dir> &&` inside its probe; the subshell discards any other.

Also check that each runnable Success Criterion is repeated as a `success:` key under `## Verification Commands`, and that each criterion that cannot be probed read-only is labelled **predicted, unverified**.

## Fence or finding mapping

This check writes findings, no fence. Every row has `lens = "check:two-control-probe"`, `file` the plan path, `line` the heading line of `plan_section`, `symbol` the task ref (`success-criteria` or `verification-commands` outside `## Tasks`), and `anchor_old` quoting the criterion's text exactly once under its section. `evidence` carries the batch lines that decided the verdict, both controls where both ran. Severity follows `/review-plan`'s verdict mapping:

- **unsatisfiable** — `critical` defect; `evidence` holds both observed values, and `fix` and `anchor_new` give the corrected command.
- **permanently green** — `critical` defect naming the assertion that would bind the changed values. A verdict reached by reading a test rather than running it is graded `medium`, with `counter` naming the narrow run that would settle it.
- **broken** — `critical` defect, a broken `success:` key included; the fix rewrites the command to fail when its criterion fails.
- **vacuous** — `critical` defect, unless the line opens `guard:` or carries `(regression guard)` and a discriminating `forward:` or `falsifier:` criterion sits beside it. A guard with no discriminating companion is still vacuous: it cannot tell success from a no-op.
- **healthy** — not a finding; a coverage-note line. When the criterion's recorded `today:` baseline is missing or differs from the observed value, write a `change` cleanup adding or correcting it.
- A runnable Success Criterion with no matching `success:` key is an `add` cleanup anchored on the adjacent key. A criterion that cannot be probed read-only and lacks the **predicted, unverified** label is a `change` cleanup.
- A probe the environment could not run is `advisory` with `angle = "risk"`, naming what the executor's machine needs; never a defect.

Defects are `fix_shape = "edits"` when the corrected command or assertion is known, `needs-plan` when binding the change needs a test the plan does not yet have. A per-task acceptance that a `plan-executor` cell may also raise keeps the task ref in `symbol`, so the anchor matcher pairs the two rows. The coverage note lists the batch path, the count of criteria probed, judged by reading and left unprobed, and each healthy baseline.
