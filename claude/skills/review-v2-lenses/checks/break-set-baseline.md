+++
id = "break-set-baseline"
status = "stable"
families = ["plan"]
executor = "verification"
fence = "BREAK-SET BASELINE"
activation = "floor"
discover = [
  "predicted: every command a task Acceptance or a Success Criterion names while predicting what a compiler, type-checker, linter or test runner will report (\"errors only in these four files\", \"goes red until task N\", \"clippy reports nothing in X\"), run as written; identical commands once",
  "plan-key: for a prediction that names a tool but no command, the plan's `## Verification Commands` key for that tool (`lint:`, `build:` or `test:`)",
  "none — the plan predicts no break set, or every predicted command writes the working tree",
]
+++

# Break-set baseline

## Runs

Each command behind a predicted break set, run once against the current tree. A break set is a plan's claim about tooling behaviour: which files a compiler, type-checker, linter or test runner will report once a task lands, or that a check stays red until a later task. Such claims are wrong often enough to be worth the seconds, and a command that is already red today pollutes every prediction made downstream of it.

The surveyor reads the plan's task Acceptance lines and its Success Criteria, collects the commands behind every prediction of that shape, and walks `discover` for each. A forward-looking prediction with no command of its own takes the plan's Verification Commands key for its tool. The digest lists each resolved command on its own `verification:` line, keyed `break-set-baseline` for the first and `break-set-baseline-2` and so on for the rest, in plan order. The orchestrator dispatches the `verification` agent once per line, one at a time, and records each log path under its key in the run's `baselines`.

The baseline observes the tree; it never creates the post-task one. A command that would write the working tree is not run: one carrying `--fix` or `--write`, a formatter without its check flag, a code generator, a migration, a `tasks import-plan` without `--dry-run`. Its line resolves to `none — <command> writes the working tree`, and the precondition judgement goes ahead without a baseline for it.

## Interpreting the result

Each log is one command's output against the tree as it stands before any task runs. Read two things from it:

- **Red or green today.** A prediction of the form "goes red until task N" or "errors only in these files" assumes a known starting state. A command already red at baseline, or one already reporting files the prediction does not name, masks the signal the prediction promises a later task.
- **What the tool actually sees.** The files the diagnostics name, the crate or project the command built, and any line saying a path was excluded, skipped or not matched. This is the evidence for whether a file the prediction names is inside the tool's reach at all.

Locate diagnostics by each tool's own format, as the `lint-baseline` check describes for clippy, tsc, ESLint and `dotnet build`, matching paths by suffix.

**Never diff observed against predicted.** A break set describes the tree after its task lands, which does not exist yet. Comparing today's output with the prediction would flag every forward-looking prediction in every correct plan.

## Fence or finding mapping

`pack.md` carries a `BREAK-SET BASELINE` fence holding only `see the run's recorded baselines`. `review show <cell> --for finder` carries the recorded log paths.

This check writes no findings. The whole-plan `plan-checks` cell owns the precondition judgement and reads these logs for it: whether each named file is inside the config's `include` and `exclude`, whether the rule that would fire is enabled, whether the construct the prediction relies on produces a diagnostic at all, and whether the baseline is already red under a prediction that needs it green. A failed precondition is a `critical` defect in that cell, because an over-predicted break set silently removes a later task's only signal and an under-predicted one sends an agent chasing another task's breakage. A `none — <reason>` entry leaves the preconditions to be judged statically, and the cell's coverage note says so.
