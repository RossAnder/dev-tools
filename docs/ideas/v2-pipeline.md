# Idea: The full v2 harness pipeline

> Captured: 2026-11-05
> Status: Paused 2026-11-05. The planning flow (topic 2) is decided by `docs/plans/v2-planning.md`; topics 4 to 8 remain open. Where a decision below conflicts with that plan, the plan wins (see its "What departs from the idea doc").

The goal is one v2 chain from planning through implementation, review and apply. The review commands stay untrialled until the whole chain exists. Every design is discussed before it is planned, and no v1 practice carries over by default.

## Where things stand

- Built and linked, with no run recorded in this repo: `/review-v2`, `/optimise-v2`, `/review-plan-v2`, the `tomlctl review` group, the lens catalogue and `claude/workflows/review-v2.js`. The Review step of `human-only-handovers` was skipped, because plan review was not built when that plan was authored.
- Run once for real work: `/plan-v2` (v2 planning), which planned and approved `human-only-handovers`, the proof recorded in `docs/ideas/plan-v2-probes.md`.
- Run once for real work: `/implement-v2`, the execution engine's first slice (`docs/plans/jiggly-exploring-robin.md`), which ran `human-only-handovers` end to end on 2026-11-10, both milestones gated and committed.
- Built: `/apply-v2`, native apply over the shared execution engine, with settle tasks for blocked findings and the reproduce check (see **Apply** below). Not started: trials, the `tool` and `clerk` executors, the rest of vetting, and harvest and clearance.
- v1 stays in use while v2 is built. `/tdd` runs inside v1 flows and dispatches v1 `/implement`, so it needs v1 until it is ported.

## Design topics

| # | Topic | State |
|---|---|---|
| 1 | Plan format and lifecycle | Decided |
| 2 | Planning flow | Decided by `docs/plans/v2-planning.md` (Approach) |
| 3 | Plan review | Decided |
| 4 | Execution engine | First slice built and run (`docs/plans/jiggly-exploring-robin.md`); `tool`, `clerk` and vetting open |
| 5 | Apply | Built (`/apply-v2`, see **Apply** below); trials and the `tool` executor are later slices |
| 6 | Flow state and glimpse | Open |
| 7 | Re-audit of the built review stage | Open |
| 8 | Naming, coexistence, gates, carrier shape | Open |

## Decisions

### Pipeline

- Apply is native and shares one execution engine with implement. Selected findings become task rows linked to their finding, so finishing a task resolves the finding. `export-v1` and `sync-v1` leave the v2 path.
- Workflows throughout, with no Agent-tool fallback in the new execution path. That needs the launch rules and pre-flight stop in the plan's Approach, "After approval", because a relaunch replays agents that have side effects.
- `/tdd` and `/plan-update` are not ported, and their v1 commands stay as they are. v2 is designed so nothing needs `/plan-update`.
- Backlog commands are not ported as such, but the backlog is central to the design below.

### Plan format

- A plan spans two flow directories: `docs/flows/<slug>/` holds a short `brief.md` (intent, scope, decisions, success criteria), `design/*.md` that tasks cite and `notes/` for exploration and research evidence; `.claude/flows/<slug>/` holds the stores, with tasks written directly to the store and no markdown import.
- Each role fetches a projection. People read one rendered, read-only view that never feeds back in.
- Approval freezes intent, not tasks: tasks then change only through audited verbs, with no event fold, and counts and status are computed (the plan's Approach, "Approval").
- No plan mode. Approval is an explicit gate.

### Task record

Every tier gets the same complete contract, and tiers differ in who executes and how much of the "how" binds them.

```toml
[[task]]
id = 12
title = "..."
milestone = "A"
needs = [7]
files = ["src/a.rs"]
outcome = "what must be true afterwards"
acceptance = [{ check = "cargo nextest ...", kind = "forward" }]
constraints = ["must not change the public signature"]
premises = [{ claim = "fn parse exists in src/a.rs", check = "..." }]
means = [{ text = "follow the pattern in b.rs", binding = "suggested" }]
design = ["design/store.md#locking"]
precedent = ["src/b.rs:40"]
executor = "lite"     # tool | clerk | lite | deep
```

- `files` stays a list of paths, with `(new)` and `(delete)` held in `[[file_notes]]` (the plan's Approach, "Task spec").
- `means` are `required` or `suggested`. Only breaking `required` or `constraints` is a real deviation.
- A tool checks `premises` just before dispatch. A failure sends the task up a tier or back for replanning.
- `executor = "tool"` adds literal `ops` that `tomlctl` applies with no model.
- A verb computes eligibility from file count, effort, operation type and repo history. The planner can only escalate from there.
- Verification sits outside the agent, cheap tiers get an explicit way to abort, and a text "done" never counts as proof.
- Keep TOML, chosen for tooling.

### Executor tiers

- Four executors: `tool`, `clerk`, `lite`, `deep`. Mechanical work becomes a verb before it becomes a clerk job. Rollback classification is a verb, never a clerk, because it is destructive.
- The clerk starts on Sonnet at low effort, with a short procedure-only body and no web tools. Its model lives only in frontmatter. A Haiku swap is flagged for the engine plan, not settled here (the plan's "What departs from the idea doc").

### Vetting

Vetting uses a small mode of the review engine: one cell per vetted task, with lenses chosen from the task's tier and the signals on its changed files.

| Tier | When | Checks |
|---|---|---|
| `tool`, `clerk` | Immediately, by tool | Postconditions, plus a small agent sample |
| `lite` | As each task finishes, before dependents build on it | Conformance: outcome met, acceptance real, in scope, required means followed |
| `deep` | At milestone drain, sampled | Judgement: correctness, ripple, seams, whether overruled means were justified |
| Any | Added by signal | For example `security` on auth or input-handling files |

- Findings go in the review store. Defects block the milestone, cleanup becomes a same-run task or goes to the final review, and advisories are parked.
- `/review-v2` sees what vetting already covered.
- New work: `conformance` and `deviation-judgement` lenses, a per-task proposal mode, and a per-milestone cost cap. The engine has never run, so vet cost is unknown.

### Classification

Lens selection gets a seam for a cheap classifier such as Jev, under the rules in `B-b9b7e4c1`: off by default, never in a gate or a write, `unsure` on low confidence, adopted per decision point only after measured agreement. It proposes signals, and rules or the orchestrator accept them. The classifier itself comes later. Row text leaves the machine, so it is opt-in per repo.

### Backlog

- The backlog is the primary store of ideas needing work, from the user or from agents.
- Not every item earns a flow, because flows are token and time intensive. Items route judiciously, and a flow needs real design decisions or a wide blast radius.
- A new `route` field holds `fix-now | clear | flow | human`, set by triage and later suggested by the classifier. It stays separate from `tier`, which says who may pick an item up.
- A bundle is a named set of items proposed from shared files or a common root cause. A bundle can earn a flow when its members would not.
- Optional capture fields: `shape`, `done_when`, `questions`, plus a file slot on `TANGENTIAL:` lines.
- Mapping into a plan: summary to intent, context to background, evidence to notes, `done_when` to success criteria, questions to candidates for surfacing, since there is no question stage (the plan's Approach, "Surfacing"). Decomposition stays judgement work.

### Planning entry

`/plan-v2` works freeform mid-discussion, with no plan mode and no backlog item required. It distils `brief.md` from the conversation, and any backlog ids or bundle named come with their full context and evidence.

### Harvest and clearance

After the final gate, implement and apply run one more stage:

1. Gather this run's harvested items, plus older open items whose files overlap this run's.
2. Triage each with `backlog-clear`'s logic: fix now, route elsewhere, or dismiss if its evidence no longer holds.
3. Run eligible fixes as a final sweep milestone on the normal engine, with its own gate and commits.
4. Report one digest of what was fixed, routed and dismissed.

Fixes and dismissals run without asking, because fixes are revertible commits and dismissals can be reopened. A fix must be small, need no decision, stay within a set budget and have a checkable outcome. A separate `/backlog sweep` runs the same logic with no flow attached.

### Plan review

`/plan-v2` launches a wrapper Workflow when authoring finishes, and the authoring session gets back a digest and a pointer only. The orchestrator applies edits between runs, because it cannot act during a run and a script has no shell:

1. A fresh `plan-review-lead` agent runs the survey and coverage proposal, then returns the cells.
2. `review-v2-pipeline` runs the wave as a child workflow, which uses the one nesting level `workflow()` allows.
3. Verification runs in two steps, below, and the lead audits the sample, settles contested findings, and writes the proposed edits to a file.
4. After the run returns, the orchestrator applies the edits their evidence tier allows to the plan records through their verbs (see **Edits** below).
5. A second top-level run re-checks only what changed. Leftovers show at approval.

- **Deferred milestones:** elaboration runs as its own top-level run before this review, with the orchestrator applying its proposals in between, so review reads specs that exist. An elaboration Workflow could not call this wrapper anyway, since that is two nesting levels (the plan's Approach, "After approval").
- **Blind finders:** finders see `brief.md`, `design/` and the tasks, not `notes/` or the authoring conversation, so they check claims against the code. Read still reaches `notes/`, so that exclusion is prompt-only unless a frontmatter `PreToolUse` hook is proven on Workflow agents (the plan's Approach, "Structure challenge").
- **Notes after findings:** once a finding exists, notes and the other excluded detail become available to clarify, compare and measure it. The verifier records its verdict first, then rechecks with the notes. A changed verdict needs a citation and is recorded, so `review stats` can show what blind review catches and how often notes overturn it.
- **Edits:** findings target a record (a task field, or a named section of a design file) rather than matching text. Edits are tiered by evidence: mechanical evidence applies automatically, a cited code reading applies and is disclosed at approval, anything else is an advisory even if a verifier agreed, and anything touching a decision, outcome or success criterion comes to the user (the plan's user decision 6).
- **One review, one approval:** the gate leads with agent-assumed decisions and puts counts last, and approving freezes intent (the plan's Approach, "Approval"). This replaces the merge questions, `.premerge.md` and `.revised.md`.
- **Exceptional escalation:** the gate offers a targeted deep re-review only when a critical is still unresolved or contested after the re-check and touches high-risk work (migrations, security, data loss, public API). It covers those portions alone and is never a default.
- **Opt-out:** there is no opt-out question. Review runs unless the planner judges the plan low risk and discloses that at approval, with `--review` and `--no-review` flags (the plan's user decision 5). If skipped, the approval gate still offers "review now". `/review-plan-v2` stays runnable alone.
- **Risk:** an agent composing and triaging reverses the engine's rule that the orchestrator does, and it has not been tried. If it works, the same lead could serve `/review-v2` (topic 7).

**Built (Slice 3):** record-targeted findings, computed evidence tiers, `review apply`, the `plan-review-lead` agent, the `plan-v2-review` Workflow, the re-check run, review inside `/plan-v2`, and plan outcome stats. Departures from the above:

- **Notes after findings and recorded verdict changes:** not built; blind finders stay prompt-only.
- **Exceptional targeted deep re-review:** not built; the gate shows the open critical instead.
- **Proposed edits in a file:** edits live on the findings (`target`, `expect`, `new`) and `review apply` reads them, giving one store, compare-and-set and an outcome record.
- **"Applies" before approval:** leaves no `[[changes]]` row, since `audit` is false until approval; the finding's `applied` field is the record.

### Execution engine, first slice

Decided 2026-11-09. These add to the plan's Approach, "After approval", which still binds.

- **Scope:** implement only, with the `lite` and `deep` executors, premise checks before dispatch, milestone gates, the drain replan and handovers. The `tool` and `clerk` executors, vetting, apply and harvest come in later slices, so the clerk's model is not chosen here. `human-only-handovers` is the first plan it runs.
- **Status:** each worker writes only `status` and `agent` on its own row through `tomlctl`, since those fields are unaudited; the orchestrator stamps `commit` on the milestone's rows after the milestone commit, because workers never commit. A worker ends a task `done`, `failed` (tried and missed its accept checks) or `parked` (only a human can unblock it), with the reason in its note. A run that dies leaves its state in the store, and glimpse shows progress during the run.
- **Commits:** the orchestrator commits once per milestone, after the gate passes. Workers share one tree and never commit, and this slice uses no worktrees.
- **Order inside a milestone:** the Workflow script walks the milestone's task graph in waves from its `args`, one run per milestone, not one per layer.
- **Partial attempts:** the worker procedure checks the diff of its own files before it starts, because the runtime can restart a stalled writer with its edits left in place. Task specs do not carry this.
- **Gate failure:** a failed milestone gate is a drain input. The orchestrator mints fix tasks through audited verbs and relaunches, for at most two rounds, then parks the milestone for the user.

### Apply

Decided 2026-11-10. These add to the Pipeline decision above, which still binds. One plan builds the first three parts below, as three milestones.

**Apply core**

- **Task home:** a verb mints task rows from triaged findings. With a flow, they join an apply milestone appended to that flow's plan through the audited verbs. A flow-less review mints a minimal flow at triage, so there is one task home.
- **Approval:** triage is the user gate and stands in for plan approval. Apply tasks get no brief, cold readers or plan review.
- **Spec:** the verb fills the row from the finding (anchor to `files`, `old` text or trigger to `premises`, the fix to `outcome`). The orchestrator only groups and escalates. Findings sharing a file fold into one task, so the link is many-to-one.
- **Executor:** cleanups run on `lite` until the `tool` executor exists.
- **Resolution:** a finding resolves when its milestone gate passes and the commit is stamped, not on a worker's `done`. Workers never write the review store.
- **Dropped from v1:** the freshness gate, replaced by premise checks, and the stash-and-restore rollback, replaced by the engine's gate failure, fix tasks and two rounds then park.
- **Open for the plan:** the carrier and verb names, since `review apply` already means plan-record edits.

**Blocked findings: settle, then fix**

The binary `fix_shape = needs-plan` is replaced by what the finding is blocked on.

| Blocked on | Handling |
|---|---|
| A decision | Asked at triage, batched, the answer recorded on the finding; it then becomes an ordinary fix task |
| Evidence (root cause, or which fix is right) | A settle task: an investigator writes a note with a proposed fix and touches no code |
| Measurement, alternatives | A trial task; designed into the field, built in the trials slice |
| Design | Routed to `/plan-v2` with the finding as intent |

- **Two milestones:** a settle milestone runs first. Its outputs are drain inputs, and the orchestrator mints the fix tasks through the audited verbs for the fix milestone that follows.
- **Tiers:** each settle proposal takes plan review's evidence tiers. Mechanical evidence proceeds, a cited reading proceeds and is disclosed, anything else comes to the user.
- **One user gate:** everything needing the user is collected between the two milestones. An unanswered proposal leaves its finding open with the note attached.

**Blocked is a claim with a burden of proof**

These keep the blocked routes from becoming a dumping ground for cautious or work-avoiding agents.

- **Default is fixable:** a finding is `edits` unless it names the missing thing, and the write verb refuses a blocked-on value without it. A decision needs at least two concrete options, with consequences and why the code and docs do not settle it; evidence needs the exact unknown; measurement needs the metric and the command.
- **Verifier downgrade:** refute-first extends to the classification. A blocked claim the verifier can answer returns as an `edits` row for a worker, with the verifier's fix sketch in its verdict. Verifiers stay read-only on the tree.
- **Settle outputs:** an edit-level proposal or a cited refutation of the finding. "Needs more investigation" is a failed task. One settle round per finding, then it goes to the user with the note.
- **No worker reclassification:** a fix worker that returns "needs a decision" has `failed`, and the task goes up a tier before it can reach the user. `parked` keeps its bar of a named human-only reason.
- **Design is the highest bar:** it requires a contract change across files or milestones, stated on the finding, and stays a user pick at triage.
- **Ageing:** an unanswered proposal shows its age, and after a set number of runs becomes a forced disposition at triage. Agents cannot park or backlog on their own.
- **Measured, not capped:** `review stats` reports the blocked rate per lens and agent, and how many settle tasks came back at the mechanical tier. Both show in the close summary. There is no hard cap, since a cap pushes agents to force fixes on findings that need a decision.

**Reproduce-check vetting**

Apply is vetting's first consumer: one cell per task at milestone drain, before the gate, checking that the finding no longer reproduces, with verdicts in the review store. The rest of the **Vetting** table waits for implement.

**Built:** `/apply-v2`, `tomlctl review schedule`, `reconcile` and the reproduce check. Departures from the above:

- **Milestone order:** the settle milestone is added before the fix milestone, and both are appended to the plan through the audited verbs.
- **`scheduled` status:** a finding moves to `scheduled` with `apply_flow` and `apply_task` when its task is minted, and to `resolved` only when the task is committed.
- **Reproductions:** verdicts are keyed by flow and milestone in `[[reproductions]]`, apart from `[[verdicts]]`.
- **User gate:** the one gate sits inside the settle milestone's drain, between the settle and fix milestones.

**Later slices**

- The `tool` executor, first used for grep-proven cleanups with exact `old` and `new`.
- Trials: worktree isolation, a noise policy for benchmarks and a budget cap. Until then, a perf finding whose benefit is only a measurement waits.

### Smaller fixes

- **Deviations:** the orchestrator takes them as input to a replan at each milestone drain, changing tasks through audited verbs (the plan's Approach, "After approval").
- **After Merge:** becomes a `[[handover]]` store holding only human-only steps (design/handover.md#reasons), each one step with reason, action, when and done-when. The execution engine runs every agent-performable landing step itself (gates on the last milestone, memory, backlog captures) and, at the end, reports each undone handover in that shape and nothing else.
- **Milestones:** the only checkpoint shape. Membership is a task field, and each milestone has a goal, gate commands and handover. Removing `single` and `per-batch` costs prose rather than Rust.
- **Glimpse:** tracks apply activity the way it tracks implement.

## Planning flow: decided elsewhere

The planning flow is settled in `docs/plans/v2-planning.md`, under Approach. The facts live there, not here. Where this file disagrees with that plan, the plan wins; its table "What departs from the idea doc" lists each correction. In short, the draft's stage table, its four open questions (task authors, milestone choice, abandoned plans, small plans) and the `/plan` name were all replaced: the command is `/plan-v2` while v1 coexists, and tasks are written by the orchestrator and challenged by cold readers rather than filled in by per-milestone writers.

## Evidence

Measured 2026-11-05 unless stated. The analysis scripts were scratch files and are not kept in the repo.

Tasks after the 2026-09-28 retune (lite n=462, deep n=1,241). Tier was assigned by a gate rather than at random, and deep work is unvetted, so tiers cannot be compared for reliability.

- No spec feature separated lite failures (24 events, so a real effect cannot be excluded).
- File count did. Deep retries: 3/401 at 1–2 files, 18/176 at 6+. Lite trouble: 14/106 at 3+ files, 10/356 below.
- Wrong or over-specified instructions outnumbered missing ones: lite 18 to 4, deep 24 to 10 (one unblinded coder, 90 samples).
- Safest shapes: S effort with up to 2 files (10/303 against 14/159), and replace, rename or delete actions (3/108 against 21/354). Markdown-only was not safer. Repo matters: `dev-tools` 0/136, `reportdesignkit` 20/305.
- `cross-cut` was 61 of 82 escalations.
- The spec is about 0.5% of an agent's first-turn context, so shorter records save few tokens.

External sources (fetched 2026-11-05; figures came through a fetch summariser and were not checked against the papers):

- No study tests prescription against model tier in agentic coding.
- Curated skills helped across tiers, while "comprehensive" ones gained +0.7pp against +21.5 for standard length: [SkillsBench](https://arxiv.org/html/2602.12670). Correction: the comprehensive figure rests on n=5, the standard-length gain is from a 19.8% baseline, and the study has no Haiku data.
- Format had no significant aggregate effect across 9,649 experiments: [2602.05447](https://arxiv.org/abs/2602.05447). That study used SQL generation.
- [Haiku 5.5 guidance](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-haiku-5-5): can stop early under long prompts at low effort, and can report done without running a check at low or medium.
- [Sonnet 5 guidance](https://platform.claude.com/docs/en/build-with-claude/prompt-engineering/prompting-claude-sonnet-5): reads prompts literally, especially at lower effort.

Stores and history:

- Last 12 plans: tasks 49% of the document, notes 17%, sizes 33k to 220k characters.
- `/plan-update` in main sessions since 2026-09-01: 25 `status` and 22 `deviation` calls by agents, against 1 `catchup` and about 3 direct runs.
- Markdown round trip: a set of files totalling 6,682 of 16,263 lines in `tomlctl/src/tasks/` (tests included), and 13 of 31 check classes.
- Backlog (1,637 rows, 7 repos): 84% of resolved rows never had a claim, 64% of rows are implement-origin tangents, and captures named 7 of the 29 files their first task needed (9-item sample, not random).
- Checkpoints: 41 stores use milestones, none use `single` or `per-batch`.
- `/review-plan`, measured 2026-10-12: 794 of 810 findings merged, none discarded. Correction: that rate is a product of the merge path, not evidence that the findings were right.
