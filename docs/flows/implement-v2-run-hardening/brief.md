# implement-v2-run-hardening

## Intent

Running a v2 milestone should need no step the orchestrator does by eye or by hand where a tool can do it, and should leave a record of every exception the user grants. Eight gaps seen on partnerhub_pages milestone C are closed:

- A milestone commit holds every changed file its rows name, wherever the file lives.
- A gate command that deploys or installs is marked as such in the plan, and the tool says which commands to hold.
- A plan in which a task depends on an effect only the gate produces is flagged while the plan is being written.
- Gate results reach the run record without being retyped.
- The worker audit reports a script run by an indirect route.
- The user can accept a failed visual pass, and the acceptance is recorded, reported and followed up.
- A worker fixing a visual defect sees a capture of its own states before it returns.
- The structural-accept warning fires only for rows that can change the look.

## Non-goals

- The v1 carriers and the `verification` agent are not changed.
- `plan drift` keeps skipping the flow's own directories.
- The engine does not ask before running a held external command; held commands still run automatically once the rest pass.
- The audit does not resolve `package.json` scripts.
- The original UI rows of a milestone do not run the capture; only fix tasks minted by the visual pass do.
- Nothing is replayed against partnerhub_pages.

## Scope budget

`tomlctl/src/plan/` (emit, check, run records, schema), `tomlctl/src/tasks/` (the accept and milestone schema, stamped schema_version 4 when a mark is held), `tomlctl/src/cli/types/`, `tomlctl/src/agents/audit.rs`, their tests, the `tomlctl` skill's references and README; the `implement-v2-engine` skill and its references `drain.md`, `visual.md`, `report.md`, `apply-rows.md`, `questions.md` and `run-milestone.md`; the `ui-capture` skill; the two `plan-worker-*` agents; the `plan-v2-engine` references where authoring gains the effect mark; the `/implement-v2` carrier's pre-flight. About three milestones' worth of small file-disjoint tasks.

## Success criteria

- Following the drain's commit step on a milestone with a changed row file under `docs/flows/<slug>/` stages that file, from one command's output, with no hand comparison.
- `plan emit gate-list` prints the held commands apart from the rest, from a mark the plan carries on a gate or an accept entry.
- `plan check` warns on a task that needs a row of the same milestone whose external accept check is run by the gate.
- `plan run gate` records a gate run from the verification agent's output as it was returned.
- `agents audit` reports a `bun -e` or `node -e` command that names a file under `scripts/`.
- After a visual pass fails its last round, one command records the user's acceptance with the states and criteria waived; the commit step proceeds on it, the report prints it, and one backlog item is captured for the waived defects.
- A visual fix task's worker runs the capture for its own states into its own directory and reads the result before returning done; a capture failure naming no file of its own is noted and returned, not repaired.
- `plan/ui-accept-structural` is silent for a row whose files match none of the project's capture paths.
- The tomlctl suite and the dispatch gates pass, the binary is reinstalled, and the `/implement-v2` pre-flight refuses a binary without the new verbs.

## Background

**B-cbda2a4f.** Seen 2026-11-20 on partnerhub_pages milestone C: task 34's deliverable was docs/flows/<slug>/notes/preview-cpu.md, a row file the drift list omits by design, so following step 10 literally would have committed the milestone without it (caught by diffing the rows' files against the drift list by hand). The recipe also leaves the intersection to the orchestrator: tasks list --get files prints each row's array as one JSON line, so it needs --get 'files.*' plus sort/comm. A verb such as plan emit commit-paths (rows' files that git status reports changed, the flow's own directories included) would remove both traps.

Evidence: `notes/backlog-B-cbda2a4f.md`

**B-d1b276a6.** drain.md step 7 says a command that changes something outside the build is held for a second verification call, but gate-list carries no signal: on partnerhub_pages milestone C, bun run preview:deploy (an accept check, run_by gate) was spotted by name. A field on gates and accept entries (for example effect = "external") and a held flag or separate list in gate-list output would make holding mechanical and auditable.

Evidence: `notes/backlog-B-d1b276a6.md`

**B-0f9fc8b5.** Seen 2026-11-20 on partnerhub_pages milestone C: task 34 measured CPU on the deployed drafts preview and needed task 33, but task 33's deploy accept check was run_by gate, so no deploy of the finished tree could exist before task 34 ran; it failed, and a replan had to make task 34 deploy itself. A plan check or cold-read question (does any task's outcome rest on an effect only the gate produces?) would surface the ordering at authoring time.

Evidence: `notes/backlog-B-0f9fc8b5.md`

**B-2a33255a.** On partnerhub_pages milestone C the gate ran four times plus two held-command reruns; each needed the 11 blocks rewritten as [[record]] rows (command, outcome, exit, duration_s, summary, failed_ids, log) by hand, which is slow and error-prone (a command string with nested quotes must be re-escaped). plan run gate could accept the verification agent's block output directly, or the verification agent could also write the record file.

Evidence: `notes/backlog-B-2a33255a.md`

**B-dcecc52a.** Seen 2026-11-20 on partnerhub_pages: the drain's audit passed --gate 'bun run preview:deploy', but the worker ran the same script through bun -e import(...), so nothing was reported and the live deploy was known only because the worker's note confessed it. Resolving each gate command's package.json script to the file it runs and matching that path too (or matching any bun -e/node -e that names a script under scripts/) would close the gap.

Evidence: `notes/backlog-B-dcecc52a.md`

**B-22eca7ef.** Seen 2026-11-20 on partnerhub_pages milestone C: the banner failed visual round 2 on two cosmetic defects and the user chose to commit as is. The orchestrator had to improvise the commit, the stamp and the report wording, and nothing in the store records that the look criteria were waived. A documented override, for example a decision row or plan run visual --accept naming the states and criteria waived, with the commit step reading it, would keep the waiver auditable.

Evidence: `notes/backlog-B-22eca7ef.md`

**B-799bfc99.** Seen 2026-11-20 on partnerhub_pages milestone C: round 1's fix for clipped descenders (padding-block with a negative margin on the editor slot) passed its tests, but round 2 found the long address no longer ended in an ellipsis, with only round 2 left. The capture command is read-only and writes under an ignored directory, so letting a look-fix worker run it for its own states and read the result before returning done would catch such regressions inside the round.

Evidence: `notes/backlog-B-799bfc99.md`

**B-d09117ec.** Seen 2026-11-20 on partnerhub_pages milestone C: tasks 34, 44, 45, 46 and 47 each drew the warning though none touches a file matching ui-capture.toml's paths. Exempting rows whose files match none of the declared capture paths would leave the warning for rows that can actually change the look.

Evidence: `notes/backlog-B-d09117ec.md`

## Unknowns

- commit-paths: What exactly must a `plan emit commit-paths` leaf print so drain.md's two commits (the milestone's work, then the flow stores, run notes and backlog) each stage from its output alone, how does `plan drift --uncommitted` compute row files and uncommitted paths today, and where does a new `plan emit` leaf register (clap, dispatch, capabilities, output_options test, skill references)?
- effect-mark: Milestone gates are stored as plain command strings and accept entries as tables; where are both defined, parsed and emitted (`milestone add --gate`, `--gate-probe`, the accept schema, `plan emit gate-list`, `covered_by`), and what is the smallest way to carry an optional external-effect mark on each so `gate-list` can print held commands separately and `plan check` can flag a task whose `needs` reach a row with an external accept that is `run_by = "gate"`?
- gate-blocks: What exact text does the `verification` agent return per command (claude/agents/verification.md), what record shape does `plan run gate` read today (tomlctl/src/plan/run_gate.rs), and can every record field be parsed from the blocks without loss, nested quotes in commands included?
- visual-accept: How are visual rounds recorded and counted today (plan run rows, visual.md, drain.md's commit and stamp steps, `plan run report`), and what must a `plan run visual --accept` record hold, and which readers must change, for the commit step to proceed on a waived failure and the report to print it?
- fix-capture: How does the visual pass mint a fix task and what does its worker receive (visual.md Fix tasks, the worker view, the ui-capture skill's `--only` and `--out` contract), what would the worker need to run the capture for its own states into a per-task directory, and does any tomlctl code read `.claude/ui-capture.toml` today so `plan/ui-accept-structural` (tomlctl/src/plan/check.rs) could match a row's files against its `paths`?
