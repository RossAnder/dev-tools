# human-only-handovers

## Intent

The post-landing steps a plan hands to the user ("After Merge" in v1, `[[handover]]` in v2) are vague, hard to action and buried in noise. Many of them are work an agent could have done during the run. From now on, a step reaches the user only when no agent could perform it, and every step that does reach the user can be acted on without re-reading the plan. Everything an agent could perform is done by the run itself. This holds for v2 plans and the v2 implementations still to be built, and for any plan written from here on.

## Non-goals

- Rewriting the After Merge sections of plans already written or already executed.
- Building the v2 execution engine; this plan sets the contract it must honour, not the engine.
- Automating the steps that genuinely need a person (restarting sessions, elevation, attended trials).

## Scope budget

One plan of small file-disjoint tasks, roughly 10-15, across the plan contracts, the carriers and agents that write or read the post-landing section, and the `tomlctl plan` handover record.

## Success criteria

- A plan cannot reach approval carrying a post-landing step that does not say why only a person can do it.
- Every post-landing step that reaches the user says, on its own, what to do, when, and how to tell it is done.
- Steps an agent can perform (installing a binary, running a check, editing a memory note, deleting a stale file) are performed during the run, not handed to the user.
- Follow-on work, trials and next plans no longer appear among the post-landing steps.
- Plan review flags a post-landing step an agent could have performed, rather than asking for more of them.

## Background

v1: `flow-contract-plan-output-format` defines `## After Merge` as "the steps someone must take once the work lands", listing installs, cache resyncs, restarts, memory-note updates and predicted criteria run by hand; `/plan-new` Phase 5 drafts it, `/implement` Phase 4 repeats it verbatim. Recent plans (`review-system-v2`, `we-ve-just-implement-the-shimmying-kettle`, `proud-leaping-conway`) mix agent-runnable commands, memory edits, file deletions, follow-on plans and multi-week trials into the list, with one multi-sentence bullet per step.

v2: `plan.toml` carries `[[handover]]` rows (`command`, `source_task`, `done`), written by `plan handover add`, shown by `plan gate` under `stops`. The engine's Design and Write steps never say when a handover is warranted. `docs/ideas/v2-pipeline.md` describes it as a store "an agent can be told to run", which contradicts a human-only list. No v2 implement engine exists yet.

Review: the `plan-risk` and `plan-structure` lenses in `review-v2-lenses` ask plans to add After Merge lines and treat predicted criteria scheduled there as covered.

## Unknowns

- What share of past After Merge bullets were human-only, and what classes the rest fall into.
- The full code footprint of extending the `[[handover]]` record: tomlctl verbs, checks, views, glimpse, tests.
