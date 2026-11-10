# Engine and carrier

How an apply run moves through the carriers and the execution engine. The prose that results lives in the skills; this file is what their authors write from.

## The apply carrier

`/apply-v2 <slug> | --scope <scope> [<finding ids> | --all]` is a thin carrier over a new `apply-v2-engine` skill. The skill schedules; `implement-v2-engine` runs.

1. **Pre-flight.** The `tomlctl` skill, then the capability check for the features in [verbs.md](verbs.md#capabilities) of each milestone whose surface the skill uses (milestone A's, and milestone C's once the settle path lands), naming `cargo install --path tomlctl` when one is missing. `/implement-v2`'s pre-flight gains `review_settle` with the settle drain.
2. **Resolve the home.**

| The review store | Home |
|---|---|
| `.claude/flows/<slug>/review-v2.toml` and the flow's plan is approved (`plan stage` reads `approved`, `running` or `done`) | that flow |
| the same, and the flow has a `plan.toml` not yet approved | none: stop, saying the plan must be approved before findings can be applied to it |
| the same, and the flow has no `plan.toml` (a v1 flow) | none: stop, saying a review bound to a v1 flow has no apply path |
| `.claude/review-v2/<scope>.toml` | a new flow, `<scope>-apply`, or `<scope>-apply-<n>` when that is taken |

3. **Preview** with `review schedule --dry-run`, before anything is opened or appended. When it would schedule nothing, report what it held and refused and stop: a milestone once added cannot be removed.
4. **A new flow** is opened with `plan open`, and its brief's four required sections are filled from the review run (the run's bounds, the selected findings, "apply only" as the non-goal).
5. **Milestones.** `plan milestone add` appends a settle milestone when a selected finding is blocked on evidence, then a fix milestone. The settle milestone has no gates. The fix milestone's gates come from the home: on a v2 flow, the gates of that flow's latest milestone that has any, plus the `CLAUDE.md` build, test and lint commands of any crate or package a scheduled file sits in that those gates do not name; for a new flow, the build, test and lint commands the repository's `CLAUDE.md` names for the crates or packages the scheduled files sit in. When no test command resolves, the carrier stops before scheduling and says so, since a fix milestone with no test gate proves nothing. On an approved flow each takes `--reason`.
6. **Schedule** with `review schedule`. Report what it held and refused.
7. **Commit.** For a new flow, commit the flow files (`tasks.toml`, `plan.toml`, `brief.md`) and the review store with its sidecar: `plan approve` refuses uncommitted flow files, and the engine's clean-tree check refuses a dirty store outside the flow. The carrier refuses to go on while a scheduled task's files are dirty.
8. **Approve**, for a new flow only, with `plan approve --review skipped-flag` and the reason `triage of <store> selected these findings`. The triage is the approval; the carrier asks nothing more. What the approval writes is committed at once, before the run, as `plan-v2-engine`'s approve step 7 does: every path `git status` lists under `.claude/flows/<flow>/`, and `.claude/active-flow.toml` with its sidecar, since `plan approve` registers the flow there and the engine's clean-tree check refuses that path. When `.claude/active-flow.toml` is already modified before the commit step, stop and name it.
9. **Run.** Invoke `implement-v2-engine` with the flow slug.
10. **Close.** After the engine's report, print `review stats --apply` as the close summary.

## Finding steps in the execution engine

The engine's rules for apply rows live in one new reference, `references/findings.md`, linked from new rows of the step table. A row is an apply row when `[[finding_links]]` names it. Each step is skipped when the milestone holds no apply row, so a plan run reads what it reads now.

### Anchor premises on a started row

An apply row's `review anchor` premise is rerun before every relaunch, and a row whose first worker already made the edit no longer matches its anchor. A failing anchor premise on a row whose `agent` is set is the row's own work: it does not withhold the row, and the relaunch goes ahead. On a row with no `agent`, a failing anchor premise withholds it as any false premise does.

### Reconcile

After the commit stamp and before the flow-store commit, run `review reconcile --flow <slug>` against each store the milestone's links name. A store outside the flow directory is staged in the flow-store commit with its sidecar. Because `review reproduce`, `review settle` and `review schedule` write that store mid-milestone, each store the flow's `[[finding_links]]` name, with its sidecar, is exempt like `.claude/backlog.toml` from the pre-flight clean-tree check (run-milestone.md, step 1) and from the drain's out-of-lane rule (drain.md, step 3).

### Reproduce check

Runs when every row of a milestone is `done` and at least one has `fixes`, before the gate.

1. The round is one more than the highest `round` among the reproductions of this flow and milestone.
2. Launch `review-v2-pipeline` in reproduce mode and record it as a `reproduce:<id>` run, finished with `plan run finish --step reproduce:<id>` once step 5 has read the store, abandoned when the launch returns `error`, and settled at re-entry by the open-run rule like an `implement:<id>` run. The args:

```json
{"mode": "reproduce", "run_dir": ".review-v2-runs/apply-<flow>-<milestone>-<round>", "store": {"slug": "<slug>"}, "flow": "<flow>", "milestone": "<id>", "round": 1, "units": [{"task": "<task ref>", "files": ["<path>"], "findings": ["F3"]}]}
```

   `store` holds `slug` or `scope`. The mode key and `run_dir` follow the plan-review wrapper's `mode: "recheck"` launch. The mode needs no pack and no schemas in its args: the script holds the unit's return schema itself.
3. Each unit's `review-verifier` reads its findings `--for implementer`, judges against the working tree whether each still reproduces, writes its rows with `review reproduce --ndjson`, and returns `{task, reproductions: [{finding, verdict}]}`. It changes no file.
4. The run returns `{mode, units: [{task, status, reproductions, error}], errors, script_version}`, where `status` is `ok`, `no-return` or `error`. A unit that is not `ok` is relaunched once, alone.
5. Read the store, not the return: one `review reproduce-status <id> --expect gone` per finding, and for a finding that fails it, `--expect reproduces`.

| Current reproduction | The engine does |
|---|---|
| `gone` | nothing |
| `reproduces` | mints a fix task in this milestone, its `reason` opening `reproduce <id> round <n>: `, `fixes` naming the finding, its `files` those of the task that fixed it, no `review anchor` premise, and one accept entry, `tomlctl review reproduce-status <id> <store flag> --expect gone`, `forward`, left to the gate |
| `unsettled`, or none | reads the cited code itself and records `gone` or `reproduces` with `by = orchestrator`; when it cannot tell, sets the apply row that fixes the finding to `parked`, with a note |

- Two reproduce rounds at most, counted from the `reproduce <id> round ` reasons, apart from the gate's own two. After the second, the milestone stays uncommitted and goes to the `needs_human` report.
- On a later pass, only findings whose current reproduction is not `gone`, or whose task's files a row of the latest launch holds, are checked again.
- There is no Agent-tool fallback: a launch that returns `error` stops the engine, as a worker launch does.

### Settle drain

Runs when every row of a settle milestone is `done`, before its commit.

1. Read each proposal note and the code it cites.
2. Tier it: `mechanical` when a command's output proves the fix, `cited` when a code reading with a citation does, otherwise `user`. A note opening `Refuted:` with a citation is recorded with `review settle --refute`.
3. Ask the user once, through `AskUserQuestion`, about every `user`-tier proposal together.
4. Record each with `review settle`: accepted ones `--accept`; rejected and unanswered ones `--defer`, so the finding keeps its proposal, gets no second settle round and ages toward a forced disposition.
5. Schedule the accepted findings into the fix milestone with `review schedule`.

A settle note that opens with neither word, or says more investigation is needed, is a `failed` task.

### Failure on an apply row

- A fix worker cannot reclassify. A return that says the task needs a decision or more evidence is `failed`.
- A `parked` return on an apply row is read as `failed` unless its reason is one of the handover reasons.
- **Tier.** The first such failure of a `lite` apply row moves it to `deep` with its spec unchanged, the `reason` saying so. This is the one place an executor moves alone, and it applies to apply rows only. A second failure parks the row for the user with the worker's note. A `deep` apply row's first failure is replanned by the drain's own re-spec rule, and its second parks it the same way.

## Triage hand-off

In the review engine, stage 8's export and v1 hand-off become one step: name `/apply-v2` with the store and the ids the user picked under `apply F1,F4` or `apply all`. Stage 0 keeps `review sync-v1`, described as a read-back for stores that still hold exported findings.

Triage presents blocked findings grouped by `blocked_on`:

- `decision`: asked together in one `AskUserQuestion` round, each answer recorded with `--answer-file`.
- `evidence`: offered for apply, where they become settle tasks.
- `measurement`, `alternatives`: listed as waiting for trials.
- `design`: offered the `plan` route, which names `/plan-v2` with the finding as intent. Only the user picks it.
- Deferred proposals are listed with their age. An overdue one must be given a disposition before the run can close.
