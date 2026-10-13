+++
id = "dry-run"
status = "stable"
+++

# Dry run

Execute one plan task in your head, holding exactly what its implementer will hold, and record every point where the task as written makes the executor guess, overreach or fail. Models judge underspecification poorly by inspecting a plan; they find it reliably by trying to carry a task out.

## Procedure

1. **Hold an implementer's context and no more.** Read the plan's preamble sections — Summary, Context, Scope, User Decisions, Approach, Success Criteria, Verification Commands, Execution Policy, Risks and After Merge — and fetch the task with `tomlctl tasks show <id> --slug <slug> --with body,files,deps`, the call an implementer's dispatch makes. Do not read the plan's `## Tasks` section, its Exploration Notes or its Research Notes unless the task's Detail names a heading there. A task that only works with context its implementer does not get is the finding.
2. **Walk the task in order.** Take the Action, then each sentence of the Detail, and for each step work out what you would do next: which file you would open, what you would write, which command you would run. Read the code the step reads. Do not edit anything and do not build.
3. **Record four kinds of fault as you go.**
   - **Decisions left to the executor.** Anything you would have to decide that neither the task nor the preamble settles: a name, a signature, a field or enum value, an error shape, an ordering, a default, a choice between two readings of a sentence. Write down both readings when the text has two. A decision the preamble settles is not one.
   - **Files touched against the Files line.** List every file the work would create, edit or delete, then compare that list with the task's **Files** line. A file the work needs that the line omits, a listed file the work never touches, a `(new)` file that already exists and a file a sibling in the same dependency batch also edits are each a fault. Count the call sites, imports and tests a signature change drags in; they are where the line usually falls short.
   - **Acceptance not runnable as written.** Read each acceptance command as the executor will type it: a path that will not exist after the task, a flag the tool does not take, a test name the task does not create, a `forward:` probe that already passes today, a `falsifier:` that cannot fail. Run a command only when it is cheap and read-only; judge one that builds, runs a suite or writes by reading it.
   - **Unconfirmed premises.** Every claim the task makes about current behaviour — a function's signature, a file's location, a type's fields, what a command prints, which caller exists — that you check and find false or cannot find. Cite the `file:line` you read. A premise you confirmed is a coverage-note line, not a finding.
4. **Check the size.** A task whose dry run spans clearly more than its effort tag, or that could not land in one focused session, is a sizing fault; name the split.
5. **Classify each fault.** A task that fails or cannot be executed as written is a `defect`, and a step the executor could only complete by inventing a load-bearing decision counts as failing. A fault fixed by editing the plan text alone — a missing path on the Files line, a wrong flag in an acceptance command — is a `cleanup`. A sequencing or scope question that needs someone to decide is `advisory`. Anchor each row to the task's section in the plan, with `anchor_old` quoting text that occurs exactly once there.

## Coverage

For each task, the coverage note names the task ref, the files you read, the premises you confirmed and the acceptance commands you ran or judged by reading. A task you dry-ran end to end with no fault is one line saying so.
