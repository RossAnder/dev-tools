# glimpse-v2-visibility

## Intent

Glimpse shows everything a v2 flow knows about its plan and its execution, and lets the user explore it. A v2 task shows at least the detail a v1 task did. The plan's records, its research notes with their source references, and what happened during the run are all readable and navigable from glimpse, without opening files by hand.

## Non-goals

- No change to how v1 flows display.
- No editing of plan records from glimpse; these views are read-only.
- No backfill of structured data into flows that ran before the new contracts; they show what is on disk.
- No change to the review surfaces beyond links into and out of them.
- Harvest and clearance, and the `route` backlog field, are separate work.

## Scope budget

Three slices in one plan, by the user's decision on 2026-11-12: reading what exists, structured output contracts, and insight views. It spans tomlctl's library, glimpse, the worker and investigator agents, the milestone and investigate Workflows and the implement engine's drain. Output contracts may change to a structured form wherever that serves a view better than parsing prose.

## Success criteria

- A v2 task's detail shows its outcome, constraints, means, executor, design and precedent links, accept checks and premises with their probe verdicts, file notes, the changes made to it after approval, its linked finding or backlog row, and its run note.
- A Plan surface shows the brief, decisions with their rejected options, evidence and bound tasks, contracts, handovers, milestones with their gates, and the timeline of step runs.
- Research notes are listed by kind with their verdict, readable in full, and each source reference can be selected to see the source it cites: as it was at the note's recorded commit where the cited path was clean then, and otherwise the working tree with a notice saying why.
- From a decision, note, task, commit or finding the user can reach the others it relates to.
- One list shows every place a worker departed from the plan in a flow, with what and why.
- The user can see how the plan changed after approval: tasks added, retitled or widened, and the reason.
- Each milestone shows its gate history: every command's outcome and duration, the kept log of each command that did not pass, and the fix rounds taken.
- Everything waiting on the user in a flow (parked tasks, open handovers, items reported as needing a person) is in one place.
- Tokens and time are totalled by milestone and by executor.
- Before launch, the user can see which premises and accept checks are unprobed or unhealthy.

## Background

`docs/ideas/v2-pipeline.md`, section "Flow state and glimpse", records the agreed design. Glimpse's task model holds only the v1 row fields; a v2 task's content lives in `[[specs]]`, which glimpse does not read, and a v2 row's `acceptance` is empty. Glimpse reads nothing from `plan.toml`, `brief.md`, `design/` or `notes/`. On `v2-apply`, that leaves unseen 56 specs, 152 accept checks, 64 premises, 46 changes, 14 decisions and 56 run notes. Glimpse links tomlctl as a library and takes its data from the snapshot. Run notes and investigator notes are free markdown: deviations sit under a "Deviations and choices" heading and source references are inline `path:line` text.

## Unknowns

- snapshot-path: How does glimpse obtain and refresh a flow's data from tomlctl's library (the snapshot, its inputs and the file watch), and what would carrying the spec and plan records, and the files under `docs/flows/<slug>/`, through that path require?
- surface-nav: How are glimpse's surfaces, list rows, details pane and key handling built, and what does adding a seventh surface with sub-lists, jumps between related records and a source preview pane take?
- gate-record: Where, if anywhere, are a milestone's gate results (each command's outcome, duration and log path), its fix rounds and the drain's `needs_human` entries stored durably today?
- worker-note: Which files define, write and read the worker's run note and return object (agents, the milestone Workflow, the drain), and what would a machine-read block opening the note, recorded into the task store by a verb at the drain, have to change?
- research-note: Which files define, write and read the investigator, challenger and cold-reader notes and their `Verdict:` first line, and what would a machine-read block of sources (path, line, claim, commit read at) have to change without breaking those readers?
- agent-metrics: What does a flow's `agents.toml` record per agent (tokens, timings, task and executor attribution), and are totals by milestone and by executor, and a probe-health summary, computable from the stores as they are?
