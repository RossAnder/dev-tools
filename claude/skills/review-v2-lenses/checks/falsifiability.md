+++
id = "falsifiability"
status = "stable"
families = ["plan"]
executor = "cell"
produces = ["defect", "cleanup"]
activation = "floor"
+++

# Falsifiability

## Runs

A static sweep of every acceptance in the plan, and the precondition judgement behind every predicted break set. It runs inside the whole-plan `plan-checks` cell beside `two-control-probe` and `acceptance-contradiction`, reads the plan document itself, `## Tasks` included, and builds nothing.

1. **Name the failing input.** For each task's **Acceptance** and each `## Success Criteria` bullet, name the input that makes it fail: the tree, file content or test outcome under which it reports failure. Then name the wrong implementation — a no-op, a partial edit, an edit to the wrong file — and ask whether the acceptance still passes on it. An acceptance that passes for the wrong implementation as readily as for the right one does not discriminate.
2. **Hunt the vacuous shapes.** Each reads as a real check on the page:
   - both sides of a comparison `undefined`, null or empty, so equality holds trivially;
   - an optional key or field the type system never requires, so adding it produces no error anywhere and a "compiles" acceptance proves nothing;
   - an empty match set: a `for` loop, `find` or glob over paths that match nothing, so the body never runs and the command exits 0;
   - a path filter matching nothing (`git diff -- <path>`, `rg <pat> <dir>`, a test filter naming no test) that exits 0 on empty output;
   - a count or existence check satisfied by text the plan itself, a fixture or an unrelated file already contains;
   - "test X passes" where X asserts nothing the task changes, or asserts only an ordering both the old and the new values satisfy.
3. **Judge the break-set preconditions.** For every acceptance or criterion predicting what a compiler, type-checker, linter or test runner will report ("errors only in these four files", "goes red until task N", "clippy reports nothing in X"), read the `break-set-baseline` log the assignment names for its command, then check statically:
   - each file the prediction names is inside the tool's reach: a `tsconfig.json` `include`, `exclude` or `files`; a crate member or target the command builds; an ESLint ignore pattern; a project the `dotnet` command loads;
   - the rule that would fire is enabled: `noUnusedLocals` and `noUnusedParameters` are off unless set; a clippy lint outside the default groups fires only when a `[lints]` table, `clippy.toml` or crate attribute enables it; an `#[allow]` on the module silences it;
   - the construct produces a diagnostic at all: `dead_code` does not fire on a `pub` item of a library crate; a doc-comment reference is not a use; adding an optional key requires nothing of any call site; `cargo clippy` without `--all-targets` never sees test code;
   - the baseline is not already red where the prediction needs it green, nor already reporting files the prediction does not name.

   When the log entry is `none — <reason>`, judge the preconditions statically and say so in the coverage note.

## Interpreting the result

An acceptance passes this check when you can name both a failing input that a missing or wrong implementation produces and a passing input a correct one produces. Where one of the two cannot be named, the acceptance is vacuous for that reason; quote the shape from step 2 it falls under.

A break-set prediction passes when every precondition holds. **Never diff the baseline's output against the prediction.** A break set describes the tree after its task lands, which does not exist yet; that comparison would fire on every forward-looking prediction in every correct plan. Read the baseline only for what the tool sees today and whether it is red today.

Defer to `two-control-probe` where it ran: a read-only command whose controls it observed takes its verdict from that probe, and this check adds a row only for a shape the two controls cannot see, such as a wrong implementation that would also pass.

## Fence or finding mapping

This check writes findings, no fence. Every row has `lens = "check:falsifiability"`, `file` the plan path, `line` the heading line of `plan_section`, `symbol` the task ref (`success-criteria` outside `## Tasks`), and `anchor_old` quoting the criterion or prediction exactly once under its section.

- **Failed break-set precondition** — `critical` defect, as `/review-plan` assigns: an over-predicted break set silently removes a later task's only signal, and an under-predicted one sends an agent chasing another task's breakage. `evidence` cites the config line or baseline log line that fails the precondition; `fix` says whether to correct the prediction, the command or the config.
- **Vacuous acceptance** — a defect. `critical` when no other criterion on the same task or goal discriminates, so the task can land a no-op green; `major` when a discriminating companion exists and the vacuous line only adds false confidence. `failure_scenario` names the wrong implementation that passes; `fix` and `anchor_new` give a criterion that fails on it.
- **Missing perturbation** — a "test X passes" acceptance whose test does bind the change but whose plan text states no falsifier is an `add` cleanup writing the `falsifier:` line that names the perturbation turning X red.

Defects are `fix_shape = "edits"` when the replacement criterion is known, `needs-plan` when no existing test or command can bind the change and one must be designed. The coverage note lists every acceptance swept, each break-set prediction with the baseline log it was judged against or `judged statically`, and the shapes ruled out.
