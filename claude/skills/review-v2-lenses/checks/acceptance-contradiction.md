+++
id = "acceptance-contradiction"
status = "stable"
families = ["plan"]
executor = "cell"
produces = ["defect"]
activation = "floor"
+++

# Acceptance contradiction

## Runs

A cross-task scan for acceptances that another task makes unsatisfiable. A cell partitioned to one task cannot see a contradiction spanning two, so this check runs over the whole plan inside the `plan-checks` cell, beside `two-control-probe` and `falsifiability`. It reads the plan document itself, `## Tasks` and `## Dependency Graph` included, and runs nothing but Read, Grep and read-only commands.

1. **List what each acceptance presumes.** For every task's **Acceptance**, every `guard:` line and every `## Success Criteria` bullet, write down the state it needs to hold when it is checked: a file, section, symbol, test name, store row, string or count. A count presumes every other source of the counted text stays put.
2. **List what each task removes or changes.** From each task's Action, Detail and Files line: files deleted (`(delete)`), symbols, tests and sections renamed or removed, rows a migration or import drops, strings rewritten, and occurrences added to or removed from a file another task counts.
3. **Pair them.** For each presumed state, find every other task that removes or changes it. Place each pair on the dependency graph, read from the `## Dependency Graph` section or the `STORE CHECK` fence: the remover upstream of the presumer, the presumer upstream of the remover, or the two unordered.
4. **Check the end state.** A Success Criterion, a `guard:` line and a `success:` key are checked after every task has landed, so the state they presume must survive every task, whatever the order.

## Interpreting the result

A pair is a contradiction when no execution order satisfies both: the remover is upstream of the presumer; the presumed state is checked at the end and some task removes it; or two tasks each presume state the other removes. A pair whose two tasks are unordered, so one of the two possible orders fails, is a contradiction the scheduler may or may not trip; the edge that settles it puts the presumer first.

Not a contradiction:

- the presumer is upstream of the remover and its acceptance is checked only when the presumer lands, never re-checked at the end;
- the remover's own acceptance asserts the removal, and the presumer is a falsifier describing the tree before it;
- the remover restores the state in a form the acceptance still matches, such as a rename the acceptance's pattern also matches.

Two tasks whose intents conflict, rather than their acceptances, are feasibility's coherence concern; a dependency edge missing for any reason other than an acceptance's presumed state is plan-dependencies'.

## Fence or finding mapping

This check writes findings, no fence. Every row has `lens = "check:acceptance-contradiction"`, `file` the plan path, `line` the heading line of the presuming task's section, `symbol` the presuming task's ref (`success-criteria` for a Success Criterion), and `anchor_old` quoting the presuming acceptance exactly once under its section. `failure_scenario` and `evidence` name both tasks, the presumed state and the line of the other task that removes it.

- **No order satisfies both** — `critical` defect, as `/review-plan` assigns: it is unresolvable at runtime and costs a repair task. `fix` rewrites the presuming acceptance to assert state that survives, or the remover to preserve it.
- **Unordered pair** — `major` defect: one scheduling order fails. `fix` adds the edge that puts the presumer first, or rewrites the acceptance so neither order fails.

Defects are `fix_shape = "edits"` when one acceptance or edge rewrite settles it, `needs-plan` when both tasks genuinely need the contested state. The coverage note lists the pairs examined and why each non-contradiction cleared.
