# milestones-by-dependency

## Intent

A v2 flow's milestones run in the order their dependencies allow, not the order they happen to be written in. Work that is ready to run is not held behind a milestone that is deferred or still being elaborated. A milestone whose rows have started still runs alone until its commit, since its gate tests the whole tree, and a serial chain inside one milestone stays the milestone-cut rule's concern. Elaborating a deferred milestone is an owned step with a written procedure, rather than something improvised by hand while the rest of the flow waits. The calls on milestone and dependency shape rest on what recent v2 sessions actually did, measured from their stores, commits and transcripts.

Seeded by:

- B-0ee4f204: implement-v2 picks milestones in store order, so a deferred or unfinished milestone holds independent later milestones
- B-c2d75b04: implement-v2 pre-flight checks the whole flow, so a milestone being elaborated blocks milestones that are ready to run
- B-4222c464: Deferred-milestone elaboration has no owned procedure and could run as a task in the implement-v2 pool
- B-a505bedd: plan-v2 orders checkpoints by narrative, placing a deferred milestone ahead of upfront milestones that do not depend on it

## Non-goals

- Dispatching rows of a later milestone into the running milestone's pool (lookahead). B-1fa8d53f settled that at plan time through the milestone-cut rule, and this plan does not reopen it unless the measurements show the cut rule failing.
- Worktree isolation for workers or for a second session.
- Any change to `/implement`, `/plan-new` or other v1 carriers.
- Running two milestones' Workflow launches at once from one `/implement-v2` session.
- Elaboration as a task in the implement-v2 worker pool (the B-4222c464 proposal). The user accepted dropping it on 2026-11-16, and the measured elaboration put 2.1 of its 20.5 minutes in agent-only reading.
- Gating a milestone on a handover being done. Today's engine checks no handover between milestones, and this plan keeps that.

## Scope budget

The `implement-v2-engine` and `plan-v2-engine` skills and their references, the `tomlctl` verbs they call for the pick, the pre-flight and the milestone checks (`plan stage`, `plan check`, `tasks check`, `tasks ready`, `plan emit launch-args`), their tests and flag references, the `/implement-v2` and `/plan-v2` carriers, the `plan milestone add` and `update` verbs that set `after`, apply-v2-engine's ordering of a fix milestone after its settle milestone, and the CLAUDE.md and tomlctl README lines that describe them. Glimpse only where it renders milestone order. Up to roughly 20 tasks.

## Success criteria

- With a milestone deferred or still being elaborated, `/implement-v2` runs any other milestone whose rows' needs and coupling outside it, and whose `after` milestones, are all done and committed, unless another milestone holds a started or unstamped row, and reports the deferred one rather than stopping on it.
- A plan can say that one milestone must run after another even when no task needs the other's output, and `/implement-v2` honours it; independence the plan does not qualify is free to run in any order.
- A milestone being elaborated, in another session or earlier in this one, does not make the pre-flight refuse a milestone that does not depend on it, and its half-written rows are never launched or committed as runnable.
- Elaborating a deferred milestone on an approved or running flow follows a written procedure that names who does it, which store writes it makes, when its cold read and probes run, and when it becomes runnable.
- Before approval, `plan check` warns when a milestone is stored ahead of one that does not depend on it through needs, coupling or `after`, so an order the planner intends is recorded with `after` rather than left to store order.
- The plan's decisions on milestone and dependency shape cite measurements from the recent v2 flows: how often independent milestones existed, how long ready work waited, and why.

## Background

**B-0ee4f204.** Seen 2026-11-12 on the reportdesignkit repo's `package-design-tools` flow. Its real shape is A -> {B, C} -> D: every C row needs only A rows or other C rows, C's files (apps/server, apps/demo, docs) are disjoint from B's (packages/shell-react, the test budget, one ADR, the a11y ledger), and `tomlctl tasks ready` listed C's first row beside B's. The stores hold it as the line A, B, C, D, so the whole flow waited while B was elaborated. run-milestone.md step 2 picks the first checkpoint in store order with a row not done, refuses `--milestone` while an earlier checkpoint is unfinished, and stops on a deferred milestone. Proposed: a milestone is runnable when it is not deferred and every `needs` of its rows outside it is done and committed; a deferred one is skipped and reported while another is runnable, and the engine stops only when none is. This differs from B-1fa8d53f, which was answered at plan time by how milestones are cut: elaboration writes no code, so the gate-tests-a-mixed-tree cost named there does not arise for the wait itself, though it does for two code milestones run out of order. Needs the pre-flight scoping in the related row to be usable.

MEASURED 2026-11-12, same flow, from the orchestrating session (times local, UTC+2; plan.toml `[[run]]` rows hold the UTC originals). Milestone A committed at 14:53 (dbbfb4bd). From then C's 12 rows (17 to 28) were all pending with no need outside A or C. B's elaboration ran 14:53 to 15:17 (design file, five decisions, six rows, one 4-minute cold read, commit 08f7671c): 24 minutes with no worker running. B's three implement runs then took 15:18 to 16:03, 45 minutes, and after the first 7-minute round B was a single chain (29 -> 31 -> 32), so rounds two and three ran one worker at a time in a pool of four. C was therefore held about 70 minutes before B's gate had even started, with three of four pool slots idle for about 38 of them. The user noticed it from the glimpse DAG view while one agent worked task 32 and asked why the C section was on hold. Nothing in the run contradicts the proposal: no C row gained a need on a B row during B's elaboration or its replans, and B's replans widened only B's own files.

Confirmed 2026-11-16 from that flow's `tasks.toml`: C's rows (17 to 28, 51, 52) need only A and C rows, though C's stored defer reason named "the new controls milestone A and B produce". D (33 to 37) is still deferred and pending, needing rows of A, B and C, so the flow is not complete.

Evidence: `notes/backlog-B-0ee4f204.md`

**B-c2d75b04.** Seen 2026-11-12 on the same flow: with milestone B's cold read open, `tomlctl plan stage` listed `open run: step cold-read` under `blockers`, and the engine's open-run rule says how to settle only `implement:<id>` and `reproduce:<id>` runs, so what a planning run does to an implement invocation is unspecified. `plan stage`, `plan check` and `tasks check` in pre-flight are all flow-wide, so half-written rows of one milestone stop every other. For a second attended /plan-v2 session to elaborate one milestone while /implement-v2 runs another: scope the pre-flight checks to the picked milestone, treat a planning run on a different milestone as not blocking, and keep a milestone `deferred` until its cold read passes. On that flow B read `elaborated` while its cold read was still open, and the flow-store commit after another milestone's gate would commit those unproved rows as runnable.

Evidence: `notes/backlog-B-c2d75b04.md`

**B-4222c464.** run-milestone.md says elaborating a deferred milestone is out of the engine's slice, and plan-v2's Prove step only says a deferred milestone is read when it is elaborated; no skill owns the steps between. On `package-design-tools` it was done by hand in a planning session while nothing else ran. Proposed, after apply-v2's settle milestone: an elaboration row whose `needs` are the tasks it waits on and whose only file is a proposal note, dispatched in the rolling pool beside other milestones' workers; at the drain the orchestrator asks the design questions in one gate, mints the rows and runs the cold read. The smaller alternative is the two related rows (pick by dependencies, milestone-scoped pre-flight) plus a second attended session.

MEASURED 2026-11-12 for milestone B's elaboration (24 minutes). Reading milestone A's output and the flow's notes was agent work and took most of the time; the user's part was ONE AskUserQuestion call of three questions, giving D29 to D31, with D32 and D33 agent-assumed; then a design file, two added rows and four updated ones, premises, probes, and a 4-minute cold read that returned 7 points (4 false premises, 3 unactionable), all fixed by row updates without a second cold read. Two things an owned procedure would have caught: (1) no step exists for elaboration on a `running` flow, so the session improvised from write-and-prove.md's first-Write rules, and `tasks update` rows needed `action` and `detail` under `set`, found only at the refusal; (2) a planned file pair `ColourField.tsx` beside `colourField.ts` broke the target repo's case-collision boundary rules and was found only when a worker's `bun run boundaries` went red, costing one failed round.

Evidence: `notes/backlog-B-4222c464.md`

**B-a505bedd.** Checkpoints on that flow were stored A, B, C, D with B deferred, though C needed nothing from B. write-and-prove.md says when to defer but nothing about where a deferred milestone sits. Proposed: at Write, place a milestone after every milestone it does not depend on that is further along in elaboration, or have `plan check` warn when a deferred milestone precedes an independent non-deferred one. This removes the idle wait without an engine change but not the serial order.

Evidence: `notes/backlog-B-a505bedd.md`

**Recorded user stance (2026-11-16, session c372393f, answered 04:59:34Z).** After a first analysis showed that `package-design-tools`' store order was the user's deliberate de-risking choice (review finding F10 put B ahead of C to test the D2 panel-fit premise first), the user chose:

- **Ordering: explicit `after`.** Milestones get an `after = [ids]` field for risk or intent ordering. The engine picks any milestone whose cross-milestone `needs` and `after` are done and committed, preferring store order. A deferred milestone is skipped and reported rather than stopping the engine. `plan check` warns when a deferred milestone sits ahead of an independent one.
- **Elaboration: full scope, owned and concurrent.** An owned procedure for elaborating a deferred milestone on a running flow, which sets `elaborated` only after the cold read passes, plus a milestone-scoped pre-flight, lane-check digest and open-run blockers, so a second session can elaborate while `/implement-v2` runs.
- **Pool-slot lookahead stays out**, as B-1fa8d53f settled.

**Investigation corrections.** Two measured claims above do not survive the transcripts and stores (`notes/milestone-dag-measure.md`, `notes/elaboration-procedure.md`): `package-design-tools` ran a pool of six, not four; and B's elaboration spent 2.1 of 20.5 minutes on agent reading, the rest on the user's answer, record writing and the cold read, so "reading took most of the time" is wrong.

**Related, resolved.** B-1fa8d53f (rows of later milestones idle while an earlier one works a serial chain) was resolved by the `milestone-cadence` flow through the `How many` cut rule in write-and-prove.md: default to one milestone, and cut only for a gate, a replan point or a deferral.

**The corpus.** Seven v2 flows: `v2-harness-tuning` (A to E, all upfront), `milestone-cadence` (A), `tomlctl-test-and-gate-cost` (A, B, C with C deferred then elaborated), `glimpse-v2-visibility` (A to C), `human-only-handovers` (A, B), `v2-apply` (A, B, C, apply-1) in this repo, and `package-design-tools` (A to D, B and C elaborated, D deferred) in `../reportdesignkit`. Session transcripts are under `~/.claude/projects/C--Users-rossa-dev-dev-tools/` and `~/.claude/projects/C--Users-rossa-dev-reportdesignkit/`.

## Unknowns

- milestone-dag-measure: settled, `notes/milestone-dag-measure.md`
- session-friction: settled, `notes/session-friction.md`
- pick-and-preflight-sites: settled, `notes/pick-and-preflight-sites.md`
- out-of-order-commit: settled, `notes/out-of-order-commit.md`
- concurrent-elaboration: settled, `notes/concurrent-elaboration.md`
- elaboration-procedure: settled, `notes/elaboration-procedure.md`
