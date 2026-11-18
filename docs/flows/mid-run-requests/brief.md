# mid-run-requests

## Intent

A plan worker running under `/implement-v2` (plan tasks and `/apply-v2` fix tasks alike) can ask the orchestrator, while it is still running, for the two things that today make it fail and wait for a second attempt: a file its task needs but does not hold, and a correction to its own spec. The orchestrator answers these by rule, without the user, so the worker finishes in one attempt whenever the rule allows; anything that changes what the work is for still goes to the user as a question. Planning also stops writing the two most common such gaps into a plan in the first place: a task that edits a generator's input without holding its generated output, and a new file whose name differs from a neighbour only by letter case.

## Non-goals

- No change to user questions: a `question` is still answered only by the user's pick, and the orchestrator never answers one itself.
- No live notices to the user for orchestrator-answered requests; they appear in the milestone report only.
- No write to `tasks.toml` while a milestone run is live; the lane check's store digest stays as it is.
- No change to how the milestone Workflow chooses which ready task to start; it keeps reading files only at task start.
- No change to v1 `/implement`, `/review-apply` or `/optimise-apply`.
- No new agent types.
- No project-declared naming checks; case collisions are caught by one generic check.

## Scope budget

tomlctl (the request records and their verbs, the `plan check` classes, the `.claude/generated.toml` reader, `plan emit gate-list`, `plan emit lane-check` if it must know granted paths), the implement-v2 engine and its references, the plan-worker shared procedure, the user-inputs contract where it names who writes what, apply-rows.md, the plan-v2 Prove step, and the cold reader if a check moves there. glimpse only if the request records surface in its inbox and must render or hide there. Tests for each tomlctl verb or class added.

## Success criteria

- A worker that needs a file no other row of its running milestone holds asks for it, gets it, and finishes in the same attempt; no second worker run for that cause.
- A worker whose spec carries an accept clause the code shows is wrong, a file name a project rule rejects, or a stale detail proposes the correction and finishes in the same attempt when the orchestrator grants it. An accept clause may be dropped or loosened by the orchestrator when the worker cites the code that shows it wrong and the entry's behaviour still holds.
- A request that would change a task's outcome, a success criterion or a decision reaches the user as an ordinary question.
- A denied request leaves today's path unchanged: `failed` with `needs_files`, or a replan at the drain; the one change is D2's, under which a denied `/apply-v2` fix row is widened by drain.md's `needs_files` rules instead of moving up a tier.
- Every grant and denial is listed in the milestone report and in the run record; nothing interrupts the user while the run is live.
- A project can list its generated files, their inputs and their regenerate and check commands in `.claude/generated.toml`. `tomlctl plan check` flags a task that holds an input but not the output; at run time a request for a declared output of a held input is granted by rule, and the gate list carries the check command of each generator whose input the milestone touched.
- `tomlctl plan check` warns, in every project, when a task creates a file whose stem differs only by case from an existing or planned file in the same directory.
- The package-design-tools retries are reproduced by tests: the task-130 shape (a declared generated output also held by a dependent row) is granted by tomlctl in the plan request tests, and the task-29/33 shape (`ColourField.tsx` beside a new `colourField.ts`) is warned by plan check. Tasks 122, 6 and 1 are the orchestrator's rule cases (an unheld test file, a README line held only by a dependent, an evidence-backed accept loosening) that requests.md covers.

## Background

The question loop landed in `v2-ui-fidelity`: a worker posts a `question` input on ledger `plan`, sends `QUESTION <id> task <n>` to the orchestrator, waits up to 600 s on the record's status, and parks if unanswered (`claude/skills/implement-v2-engine/references/questions.md`). The orchestrator relays the user's pick and binds it with `plan decide`. That loop is the one thing that writes during a live run, and it writes only `.claude/inputs.toml` and `plan.toml`.

In reportdesignkit's `package-design-tools` flow (about 137 tasks) eight tasks ran twice. Tasks 130, 122 and 6 lacked a file (`canvas.generated.css`, regenerated from `tokens.css`; a browser test; a README line), and the drain widened them mechanically by "Widening a row from `needs_files`" in `drain.md`. Tasks 29 and 33 were re-specced with renamed modules after `bun run boundaries` rejected `ColourField.tsx` beside `colourField.ts`. Task 1 was re-specced to drop a wrong accept clause. Each retry costs a second full worker run, which re-reads and re-checks the first attempt's edits, plus a drain round boundary. The run notes are under `~/dev/reportdesignkit/docs/flows/package-design-tools/notes/run/`.

Decided with the user before this brief: both `scope` and `amend` requests in the first cut; the orchestrator is the granting authority; grants visible in the milestone report only; an evidence-backed accept loosening is grantable; `/apply-v2` fix tasks included; generators declared in `.claude/generated.toml` and used at plan and run time; case collisions caught by a generic `plan check`.

## Unknowns

- request-record: Where should a worker's scope or amend request and the orchestrator's answer be stored — a new kind in `.claude/inputs.toml` (where `request` is already the user's change-request kind and glimpse renders the inbox) or a flow-local store — given which stores the orchestrator may write while a milestone run is live, what the lane check and the pre-flight clean-tree check exempt, and what tomlctl's inputs schema and writer rules would have to change?
- live-run-holders: While a milestone Workflow run is live, how can the orchestrator decide from the stores alone which rows of that run hold a path, pending or in flight, from what `plan run start --tasks`, `plan emit launch-args` and the Workflow's held-file and siblings logic record, and can a widening granted under the any-row-of-the-milestone rule ever let two concurrent workers edit one file?
- drain-integration: Where in implement-v2's drain (drain.md steps 1 to 6, `plan emit lane-check`, the needs_human report, `plan run report`) must granted scope and amend requests be applied so the lane check never flags a granted path, the audited `tasks update` carries each change with its reason, and an amend's new accept, means or constraints are in the store before the gate list is emitted, and how does apply-rows.md's failure handling for `/apply-v2` fix rows change?
- worker-protocol: How should the plan-worker shared procedure (the `plan-worker-procedure` shared block) be extended for scope and amend requests, covering the one-question-per-run limit, the 600 s wait loop, the SendMessage header, what the worker does while it waits, a denied answer, and how `verify-shared-blocks.sh` and the `cli::dispatch::tests` gates constrain the edit?
- generated-map: What should `.claude/generated.toml` look like (outputs, inputs as paths or globs, regenerate and check commands), modelled on existing per-project configs such as `.claude/ui-capture.toml` and `.claude/review-lenses/config.toml`, and where in tomlctl would `plan check` (a task holding an input but not its output), `plan emit gate-list` (the check command of each touched generator) and the run-time grant rule read it?
- case-collision: Can `tomlctl plan check` tell which files a task creates rather than edits (the `authored_files` feature, spec fields, existence at HEAD), and where would a generic warning for a created file whose stem differs only by case from an existing or planned file in the same directory sit among its check classes and tests?
