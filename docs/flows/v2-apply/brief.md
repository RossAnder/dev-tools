# v2-apply

## Intent

Findings from a v2 review are fixed inside the v2 chain. After triage, the findings the user selected are carried out by the same execution engine that runs approved v2 plans, and a finding counts as resolved only when its fix has passed a gate and been committed.

Findings that cannot be fixed as written are handled by what they are blocked on, not sent wholesale to a new plan: a missing decision is asked once, missing evidence is investigated and comes back as a concrete proposal, and only a real design problem goes to planning. These routes must not become a place where cautious or work-avoiding agents leave work: a blocked claim has to name what is missing, it is challenged like any other claim, and how often agents use it is measured and shown.

Each applied fix is checked against the finding it answers, so "the gate passed" is not the only evidence the finding is gone.

## Non-goals

- Trials: measured or alternative-comparing fixes, worktree isolation, benchmark noise policy and budgets. The blocked-on value for them is defined, and nothing runs them.
- The `tool` and `clerk` executors. Cleanups run on `lite` or `deep` by the executor rule.
- The rest of the vetting table (conformance for `lite`, sampled judgement for `deep`, signal-added lenses) and vetting of implement runs.
- Harvest and clearance after the final gate.
- Changing or removing the v1 commands (`/review-apply`, `/optimise-apply`, `/review`, `/optimise`) or the v1 ledgers.
- Plan-family findings, which `tomlctl review apply` already writes to plan records.
- A classifier for lens or route selection.

## Scope budget

One plan, three milestones: apply core; blocked findings with settle-then-fix and the burden-of-proof rules; reproduce-check vetting. Work lands in `tomlctl/src/` (the `review` group, and the `plan` or `tasks` groups where apply needs them), `claude/commands/`, `claude/skills/`, `claude/agents/`, `claude/workflows/`, the link scripts and `CLAUDE.md`. Glimpse changes only if apply activity does not already show through the engine's task rows.

## Success criteria

- After a `/review-v2` or `/optimise-v2` triage, the user can have selected defect and cleanup findings applied without any v1 ledger, v1 carrier or `export-v1` step, both for a review bound to a v2 flow, whose plan takes the apply milestones, and for a flow-less one, which gets its own flow.
- Each selected finding is traceable to the task that fixes it, and the task to its findings; several findings in one file can share a task.
- A finding becomes resolved only after its milestone's gate passes and the commit is recorded; a failed or parked task leaves its finding unresolved: it stays `scheduled` until its task is committed or removed.
- A finding whose anchor no longer matches the code is caught before a worker is dispatched.
- A finding marked blocked without naming what is missing is refused when written. A blocked claim a verifier can answer comes back as an ordinary fixable finding, and the verifier changes no code.
- Decision-blocked findings are asked together at triage; evidence-blocked findings each get one investigation that ends in a concrete proposal or a cited refutation, and everything from those that needs the user is asked at one point between the settle and fix milestones.
- A fix worker cannot turn its task into a blocked finding; that outcome is a failure that goes up a tier first.
- An unanswered proposal stays visible with its age and is forced to a disposition after a set number of runs.
- The close summary of an apply run shows the blocked rate per lens and agent and how many settle tasks turned out to be mechanical.
- After a fix milestone drains, each fixed finding has a recorded verdict on whether it still reproduces, and one that still does blocks the milestone.
- Triage in the v2 review engine hands off to the new apply path, and `export-v1` is no longer part of it. `sync-v1` stays at the start of a review run only as a legacy read-back for stores that still hold exported findings.
- A review bound to a v1 flow is refused by the apply path with the reason named.
- A finding blocked on design reaches planning only when the user picks the `plan` route at triage, which names `/plan-v2` with the finding as intent; no agent sends a blocked finding to the backlog or to a plan.

## Background

- Decisions: `docs/ideas/v2-pipeline.md`, sections **Pipeline** and **Apply** (decided 2026-11-10), with **Vetting**, **Task record** and **Execution engine, first slice** as the surrounding design.
- The execution engine is `/implement-v2` over the `implement-v2-engine` skill and the `implement-v2-milestone` Workflow; it has run one plan, `human-only-handovers`.
- The review store, its verbs and routing are in `tomlctl/src/` under the `review` group, with flag tables in `claude/skills/tomlctl/references/review.md`; triage and close are `claude/skills/review-v2-engine/references/triage-and-close.md`.
- Today a defect needing decisions carries `fix_shape = "needs-plan"` and goes through the backlog to `/plan-new`; selected findings leave through `review export-v1` to the v1 apply carriers.
- `tomlctl review apply` already exists and writes plan-record edits, so the code path needs its own verb and carrier names.
- v1's apply contract, for what is deliberately not carried over (freshness gate, stash-and-restore rollback, lite vet): the `flow-contract-apply-*` skills.

## Unknowns

- finding-to-task: Which fields of a triaged code or perf finding (defect and cleanup) can fill a v2 task row's `files`, `outcome`, `premises`, `accept`, `constraints` and `executor` mechanically, and which task fields have no source on the finding?
- task-home: How can apply task rows and a new milestone be added to an already approved v2 plan, and what is the least a flow-less review needs (flow, brief, approval, gates) before `/implement-v2`'s pre-flight and engine will run its tasks?
- fix-shape-reach: Every reader and writer of `fix_shape` and `needs-plan` across `tomlctl/src/`, the catalogue, skills, agents and Workflows, and what replacing the binary with a blocked-on field that carries required detail would touch, including existing stores.
- engine-settle-fit: Can the implement-v2 engine and its milestone Workflow run a task that changes no code and ends in a note with a proposal, mint follow-on fix tasks from it at drain, and hold one user gate between two milestones, and what in the worker procedure, gate or commit steps assumes a code change?
- reproduce-check: How can a per-task check that a finding no longer reproduces run at milestone drain before the gate, reusing the review store, the verifier and `review-v2-pipeline`, and what do run, cell and verdict records need so a verdict on an already-verified finding is distinct from its original one?
- export-v1-exit: What in the v2 review path depends on `export-v1`, `sync-v1` or the v1 apply carriers (engine stages, carriers, tests and gates such as `command_lint`), and what must change for triage to hand off to a native apply instead?
