+++
id = "plan-grounding"
status = "stable"
families = ["plan"]
mode = "judgement"
produces = ["defect", "cleanup", "advisory"]
owns = ["plan-alignment", "enumeration-drift", "behavioural-premises", "unmentioned-consumers", "cross-cutting-concerns", "config-migration-build"]
activation = "floor"
slice = "whole-scope"
methods = ["sweep", "trace"]
+++

# plan-grounding

## Question

Does the tree the plan will change look the way the plan says it does — its cited files, symbols, lists and behaviours — and does the plan reach every file, consumer and supporting change its tasks will actually need?

## Owns

`plan-alignment` — a file, symbol, path or line number the plan cites that does not exist, or no longer sits where the plan says or looks the way the plan assumes. A missing filename is never low-severity, because nothing downstream re-derives it; line drift alone is.

`enumeration-drift` — an enumeration, allowlist, count or table the plan transcribes from the tree that no longer matches what the tree yields when the list is re-derived.

`behavioural-premises` — a claim about what the code currently does, as distinct from what exists: "X already transitions", "Y is read by Z", "this path is unreachable today", which the code or a command contradicts. These premises decide whether a task's Action is needed and whether its Detail is right.

`unmentioned-consumers` — files, components and consumers the change will affect that no task names: callers of a changed signature, readers of a changed output shape, code that depends on behaviour a task alters.

`cross-cutting-concerns` — logging, error handling, authorisation and cache invalidation that the planned change needs and no task addresses, judged against how the surrounding code handles each one.

`config-migration-build` — configuration, migration, build, manifest, deployment or registration changes the planned work needs to run, compile or ship, which no task schedules.

## Probes — plan

Read the plan in full, then the tree. Each probe is an explicit procedure, not an impression: report one coverage-note line per probe even when it finds nothing, such as "premises (behaviour): 6 checked, 1 contradicted".

- Alignment: for every file, directory, symbol, path and line number the plan cites, confirm it exists and reads as the plan assumes. Search for a missing file by its name anywhere in the tree before calling it missing, since a move is a different finding from a deletion. Summarise line drift across the plan in one enumerated row, and give each non-existent file or symbol its own.
- Enumerations: for every list, allowlist, count or table the plan transcribes — files under a directory, variants of an enum, callers of a function, rows of a registry, a "today: N" baseline — re-derive it with a recorded command, run the `sweep` method's way, and report the delta together with that command.
- Behavioural premises: for every sentence that asserts current behaviour rather than current structure, find the code that settles it with the `trace` method, or run a cheap read-only command that measures it. Report each one as plan-assumes against code-shows, citing the `file:line` or the command output. A premise stated with confidence is still a premise; the ones nobody hedges are the ones nobody checked.
- Consumers: for every signature, type, output format, file layout, status value or default a task changes, find who reads it, and check that each reader lands in some task's Files line or is shown to be unaffected. A reader no task names is a row against the task that makes the change.
- Cross-cutting concerns: for each new path, verb, endpoint or writer the plan adds, read how its nearest siblings log, report and propagate errors, check authorisation and invalidate caches, and ask whether a task gives the new one the same treatment or says why it does not need it.
- Configuration, migration and build: a new dependency with no manifest edit, a new file the build, packaging, deploy script or registration table must list, a schema change with no migration task, a setting read with no default and no task that writes it, a generated file with no regeneration step.

## Does not flag

- Confirmations: a citation, list or premise that checks out is a coverage-note line, never a row.
- A drift or premise claim settled from a comment, a doc or the plan's own prose; read the code or run the command, and treat a stale comment as no evidence either way.
- Choices the plan records as deliberate — in User Decisions, an ADR or a CLAUDE.md line — such as a consumer it leaves unchanged on purpose or a concern it scopes out; that is `advisory` with `angle = challenge` at most.
- Speculative consumers with no reference in the tree, and style-only remarks on how the plan words a citation.
- Renamed or removed names still present in the tree, registration seams with no call site, removed justifications and same-pattern sites a search string finds; those are ripple's sweeps. This lens owns what a search string cannot find.
- Missing tests, which belong to test-gaps; a cited third-party API against its pinned version, which belongs to external-validity; and an injection or authorisation vector at a trust boundary, which belongs to security.
- An edit target a task names in its own body but omits from its Files line; that parity is plan-executor's.
- Areas outside the plan's scope, and supporting changes whose gain is negligible against the churn they add.

## Edit forms

- `change` — on the task text, with `plan_section`, `anchor_old` and `anchor_new`: correct a stale path, symbol or line citation; replace a transcribed list or count with the re-derived one; rewrite a premise to what the code shows; add an unmentioned consumer to a task's Files line and Action.
- `add` — a sentence or bullet in an existing task that schedules a missed configuration, migration or build step, or gives a new path the logging, error handling or invalidation its siblings carry, when the step is small and the task that needs it is clear. A missing piece of work large enough to need its own task is a defect with `fix_shape = needs-plan`.

## Advisory angles

- `risk` — a premise the cell could not settle without building or running something it may not run; name the command that would settle it.
- `challenge` — a documented scoping decision that leaves a consumer or a cross-cutting concern unaddressed and now looks wrong; name the decision in `challenges`.
- `direction` — a consumer set or supporting change large enough that the plan's scope may need to grow, when adopting it needs a decision.

## Severity calibration

- `critical` — a task cannot be executed as written: it edits a file that does not exist, rests on a behavioural premise the code contradicts so its Action does the wrong thing, or ships with a migration or build step missing so the result does not compile or run.
- `major` — a consumer, cross-cutting concern or configuration change no task covers, with a nameable breakage once the plan lands; or a transcribed list whose drift would make a task miss real sites.
- `minor` — line drift, a stale citation an executor would trivially re-find, or a list delta that touches nothing the tasks act on.
