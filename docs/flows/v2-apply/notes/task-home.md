Verdict: Rows and a milestone append to an approved plan through the existing audited verbs (`plan milestone add --reason`, `tasks add-many` with `reason` and a full `spec`), and stage stays `approved` or `running` (tomlctl/src/plan/stage.rs:167-175). A flow-less review needs a new v2 flow: `plan open`, the four required brief sections, a milestone, specced rows, the flow files committed, then `plan approve --review skipped-flag --reason` (tomlctl/src/plan/approve.rs:197-289). After that the reviewed code and the review store must be committed, or the pre-flight's clean-tree check refuses the run.

## Answer

**Adding to an approved plan.** No new verb is needed. `plan milestone add --id <X> --goal .. [--gate ..] --reason ..` appends a checkpoint and a milestone row. On an audited store it requires `--reason` and records a `[[changes]]` row. Then `tasks add-many` mints rows with `checkpoint = "<X>"`, a `reason` on every row and a `spec` holding `outcome`, at least one `accept` with `check` and `kind`, and `executor` set to `lite` or `deep`. That is the same row shape the drain already uses for fix tasks (drain.md:162-188). After approval, `plan stage` reads only the approval and the row statuses. Adding pending rows turns `done` into `running` and leaves `approved` and `running` as they are. The only blockers it raises then come from the brief, so the engine's pre-flight still passes, provided `plan check` and `tasks check` are clean. Nothing in the code requires `plan approve --amend`. Whether one should be required, to record the user's consent to the extra work and move the drift baseline to HEAD, is a design choice for the planner.

**Ordering limit.** A new milestone always goes last (milestone.rs:75-90). The engine runs the first checkpoint that still has a row not `done`. It refuses `--milestone` while an earlier checkpoint is unfinished (implement-v2-engine/SKILL.md:38; run-milestone.md:57). So on a plan with milestones still to run, the apply milestone waits behind all of them.

**Flow-bound review on a v1 flow.** Such a flow cannot host these rows. Every `plan` verb refuses a flow without `plan.toml`, and `plan open` refuses a flow whose `plan_path` is not its own `brief.md` (open.rs:121-140). A v1-bound review therefore takes the same path as a flow-less one.

**Least a flow-less review needs**, in order:
1. `plan open --slug <s>`. This creates the flow in `draft` and writes the brief skeleton.
2. Fill the four required brief sections: Intent, Non-goals, Scope budget and Success criteria (check.rs:33-34). Each empty one is an error-class finding that blocks approval.
3. One `plan milestone add`. A gate is optional: an empty `gates` list is only a `plan/milestone-no-gate` warning.
4. The rows through `tasks add-many`, each with its `spec`. Decisions, contracts, design files, premises and probes are not required: `plan/probe-absent` is a warning, and with no surfaced decision there is nothing unanswered.
5. Commit `tasks.toml`, `plan.toml` and `brief.md` (approve.rs:358-390).
6. Run `plan approve --review skipped-flag|skipped-low-risk --reason ..`. `approve` never checks whether the `review` stage is settled, and `--review ran` needs a plan-family run (approve.rs:230-240), so a code review must use a skip value. Approval sets `audit = true`, moves the flow to `in-progress`, registers it and stamps HEAD. The writes after the stamp land under the flow directories, which the pre-flight never lists.

**Working-tree trap.** The pre-flight requires a clean tree apart from the backlog and the `files` of started rows (run-milestone.md:37-44). The only directories excluded are the flow's own two (drift.rs:196-209). Apply rows start with no `agent`, so any dirt in their files counts as someone else's edit, and the run stops. A review of uncommitted work therefore cannot be applied until that work is committed. The same check catches a flow-less review store at `.claude/review-v2/<scope>.toml`: it is neither git-ignored nor inside the flow, so the store has to be committed too, or moved into the flow.

## Findings

- **Library/API or file**: tomlctl/src/plan/stage.rs:167-175 `assess`
- **Source**: file read; `tomlctl plan stage --slug human-only-handovers` returns `{"stage":"done",...,"blockers":[]}` with checkpoints A and B
- **Evidence-grade**: high
- **Finding**: After approval, stage depends only on the newest approval and the row statuses, so appending rows keeps the plan runnable (`done` becomes `running`).
- **Details**: The approved branch returns early with only brief blockers (stage.rs:176-190). `plan check` errors no longer change the stage, but the pre-flight runs `plan check` and `tasks check` on its own and stops on any error (run-milestone.md:19-26). `plan check` on human-only-handovers currently returns only a `dag/unreachable-claim` warning and exits 0.
- **Counter**: Some step outside `assess` that refuses rows minted after the approval stamp would invalidate this. `approve.rs` and `drift.rs` contain no such check.
- **Impact on plan / report**: The apply path can reuse `plan milestone add` and `tasks add-many` and needs no new `tomlctl` verb to add rows or a milestone.

- **Library/API or file**: tomlctl/src/plan/milestone.rs:53-104 `add`; tomlctl/src/tasks/add.rs:132-140,203-210
- **Source**: file read; claude/skills/tomlctl/references/tasks-write.md:119,156
- **Evidence-grade**: high
- **Finding**: On an approved (audited) store, a milestone needs `--reason`, `tasks add` is refused outright, and every `add-many` row needs a non-blank `reason`.
- **Details**: Each call appends a `[[changes]]` row with `field = "*"`. The new checkpoint is pushed to the end of `checkpoints`. `add-many` validates `checkpoint` against the declared checkpoints, so the milestone has to exist before its rows are added.
- **Counter**: A store without `audit = true` would invalidate this, and only an unapproved plan has one (approve.rs:259-264).
- **Impact on plan / report**: The apply carrier stages a `.toml` record file of rows that carry `reason` and `spec`, as drain.md:162-188 already does.

- **Library/API or file**: claude/skills/implement-v2-engine/SKILL.md:38; references/run-milestone.md:57
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: An apply milestone added to a plan that still has unrun milestones cannot run before them.
- **Details**: The pick is the first checkpoint in store order that still has a row not `done`. `--milestone` is refused while an earlier checkpoint is unfinished. `milestone add` only appends (milestone.rs:75).
- **Counter**: A verb that reorders `checkpoints` would invalidate this. None is documented in plan.md or tasks-write.md.
- **Impact on plan / report**: The planner should state the policy: apply runs after the bound plan's remaining milestones, or it goes to a separate flow.

- **Library/API or file**: tomlctl/src/plan/open.rs:121-140 `refuse_foreign_plan`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: A review bound to a v1 flow cannot have apply tasks in that flow. It needs a new v2 flow, like a flow-less review.
- **Details**: `plan open` refuses a flow whose `context.toml` `plan_path` is not `docs/flows/<slug>/brief.md`. Every other `plan` verb refuses a flow without `plan.toml` with `kind=not_found` (plan.md:40-50).
- **Counter**: If `/review-v2` flow binding only ever resolves to v2 flows, the v1 case cannot arise. That was not checked here, since it belongs to the export-v1-exit lens.
- **Impact on plan / report**: "Bound to a flow" splits into a v2 flow (append) and anything else (a new flow).

- **Library/API or file**: tomlctl/src/plan/approve.rs:197-289 `approve`; tomlctl/src/plan/check.rs:33-34 `REQUIRED_SECTIONS`
- **Source**: file read; plan.md:466-482, 555-559
- **Evidence-grade**: high
- **Finding**: The minimum approvable flow-less plan is `plan open`, the four brief sections filled, one milestone, specced rows, the three flow files committed, and `plan approve --review skipped-flag --reason ..`.
- **Details**: Approval refuses open runs, unanswered surfaced decisions, any `plan check` error (`task-no-spec`, `spec-no-outcome`, `spec-no-accept`, `accept-no-check`, `brief-section-empty`), a status other than `draft` or `in-progress`, and uncommitted `tasks.toml`, `plan.toml`, `brief.md` or `design/` (approve.rs:358-390). The `review` stage is not gated, and `--review ran` needs a finished plan-family run, so a code review uses a skip value with a reason.
- **Counter**: A `plan check` error class that fires on a minimal plan, for example a `policy/*` default on a fresh `plan open` store, would add a requirement. Settling that needs a dry run on a scratch flow, which this read-only lens could not do.
- **Impact on plan / report**: The flow-less apply path needs one extra commit of the flow files before `plan approve`, and an attended user step for the approval.

- **Library/API or file**: claude/skills/implement-v2-engine/references/run-milestone.md:37-44; tomlctl/src/plan/drift.rs:174-209 `uncommitted_files`
- **Source**: file read; `git check-ignore -v .claude/review-v2/x.toml` prints nothing (not ignored)
- **Evidence-grade**: high
- **Finding**: The pre-flight stops when the reviewed code is uncommitted, or when a flow-less review store outside the flow is dirty.
- **Details**: Only `.claude/flows/<slug>/` and `docs/flows/<slug>/` are skipped. A fresh apply row has no `agent`, so dirt in its `files` is someone else's. `.claude/review-v2/<scope>.toml` is tracked territory outside the flow, so any write the apply path makes there blocks the next invocation until it is committed.
- **Counter**: If `/review-v2` only reviews committed ranges, and the apply path writes resolution status only at flow-store commit time, the trap cannot occur.
- **Impact on plan / report**: The apply carrier must refuse, or ask for a commit, when the reviewed code is dirty. It must also either relocate or copy a flow-less store into the new flow, or include it in the flow-store commit (drain.md:356-362 stages only the flow dirs and the backlog).

Coverage: read in full claude/commands/implement-v2.md, implement-v2-engine/SKILL.md, references/run-milestone.md, tomlctl/references/plan.md, and the brief. Partial reads: drain.md 160-374, tasks-write.md 100-269, plan/stage.rs 100-219, plan/approve.rs 197-500, plan/open.rs 39-140, plan/milestone.rs 53-112, and greps of drift.rs, store.rs and tasks/add.rs. Checks run: `plan stage`, `plan check` and `plan drift` on human-only-handovers, and `git check-ignore` on the review store paths. Ruled out: plan-family filtering, since `plan_review_state` reads only `family = "plan"` (store.rs:300-330), so a code review's findings in a flow's review-v2.toml do not touch approval; and flow archive or move, since no such verb exists under `tomlctl/src/flow/`. Left to sibling lenses: how a finding fills the spec (finding-to-task), and committing or resolving the review store at the commit step (reproduce-check).
