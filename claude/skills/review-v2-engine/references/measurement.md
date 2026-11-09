# Measurement and tuning

v2 is trialled in use rather than compared against v1 run for run, so its quality is judged by signals that come from outside the reviewers: what happened to exported findings, what later runs found in places earlier runs called covered, what the audit overturned, and what the orchestrator spent. This reference names those signals, the v1 baselines they are read against, and the numbers that count as better or worse.

No lens, facet or activation tier is retuned on fewer than **five runs** of evidence for it. Below that, record what you see and wait.

## Signals

| Signal | Read with | What it measures |
|---|---|---|
| Apply outcomes | `review stats --outcomes` | Exported findings joined to their v1 status through `v2_ref` — fixed, wontfix with reasons, deferred, verified-clean — per lens and kind |
| Escapes | `review stats --escapes` | Findings minted in a file and concern that an earlier run's cell had sealed `covered`: the recall proxy for composed coverage |
| Refute rate | `review stats --by lens` | Share of a lens's verified rows its verifiers refuted |
| Audit overturns | `review stats --by lens`; `review stats --by model` | Overturn rate on CONFIRMED and REFUTED defects and on checked cleanup, per lens; and per agent model, grouped by the model the run's `models` records for the verifying role (`verifier` and `panel` read the verifier's model, `checker` the checker's), `unknown` when the run records none |
| Dismissals | `review stats --by lens` | Share of a lens's rows the user dismissed at triage, with reasons |
| Coverage | `review stats --by run` | Uncovered and thin cells, waiver rates, per run |
| Probe recurrence | `review stats --by probe` | Probes recurring under one concern tag across runs — a promotion candidate at three or more |
| Plan outcomes | `review stats --plan-outcomes` | Per plan-family run of a v2 flow and in total: tier by disposition (applied, applied-disclosed, user-accepted, user-vetoed; advisory open, dismissed, parked or converted), the user-veto rate, growth from applied edits, re-check yield and cost |
| Calibration | `review stats --calibration` | Exemplars drawn only from user dismissals, v1 wontfix reasons and audit overturns |
| Cost | the run record (`agents`, `waves`) and the context measurement below | Agents and waves per run; orchestrator context per run |

`review sync-v1` runs at every run's stage 0, so the store is current before any of these is read, and a re-found fixed issue resolves as a **regression**, not a repeat. `stats` writes nothing.

The review across all stores, after five or more runs:

```bash
tomlctl review stats --all --outcomes --escapes --calibration
tomlctl review stats --all --by lens
tomlctl review stats --all --by probe
tomlctl review stats --all --by model
tomlctl review stats --all --family perf --by lens
tomlctl review stats --all --plan-outcomes
```

`--plan-outcomes` reads plan-family runs only and refuses `--scope`: name the flow with `--slug`, or pass `--all`, which skips flow-less stores.

Calibration learns from user dismissals, v1 wontfix reasons and audited refutations only, never from a verifier's refutation alone, so a verifier that over-refutes cannot teach finders to stop raising a class.

## v1 baselines

Each v1 figure below was computed on 2026-10-12 over every v1 ledger in this repo: the review ledgers `.claude/flows/*/review-ledger.toml` and `.claude/reviews/*.toml`, the optimise ledgers `.claude/flows/*/optimise-findings.toml` and `.claude/optimise-findings/*.toml`, and the plan-review ledgers `.claude/flows/*/plan-review-findings.toml`. Each is one `tomlctl items list` call per ledger, summed across ledgers; the per-ledger commands are:

```bash
tomlctl items list <ledger> --count-by status
tomlctl items list <ledger> --count-by severity
tomlctl items list <ledger> --where category=<category> --count-by status
tomlctl items list <ledger> --array vet_events --select lens,sampled_count,dropped_count,downgraded_count --ndjson
```

Re-running them gives today's figures, which drift as v1 keeps running; compare a v2 window against v1 figures from the same date.

### Outcomes

| Family | Items | Outcome counts |
|---|---|---|
| review (code) | 950 | 842 fixed, 24 wontfix, 28 verified-clean, 10 deferred, 46 open |
| optimise (perf) | 109 | 81 applied, 7 wontapply, 21 open |
| review-plan (plan) | 883 over 40 plans | 867 merged, 0 discarded, 16 open; 129 critical (15%) |

**Resolved precision** is `fixed / (fixed + wontfix + verified-clean)` per v1 category (`applied` and `wontapply` for perf). **Suggestion share** is the share of a category's rows at severity `suggestion`, v1's nearest equivalent of v2's `cleanup`.

| v1 category | Resolved precision | Suggestion share |
|---|---|---|
| quality | 94.5% (207 / 219) | 65% |
| testability | 97.8% (177 / 181) | 58% |
| completeness | 95.9% (117 / 122) | 58% |
| architecture | 89.0% (105 / 118) | 48% |
| package-quality | 98.9% (92 / 93) | 57% |
| db | 94.1% (48 / 51) | 57% |
| security | 93.2% (41 / 44) | 49% |
| query (perf) | 96.4% (27 / 28) | 61% |
| concurrency (perf) | 100% (20 / 20) | 46% |
| algorithm (perf) | 92.9% (13 / 14) | 87% |
| memory (perf) | 82.4% (14 / 17) | 76% |
| serialization (perf) | 77.8% (7 / 9) | 62% |

Categories outside these twelve are ad hoc and too small to baseline. **Post-verification leak** — resolved items that turned out not worth fixing — is `(wontfix + verified-clean) / resolved`: 5.8% for review (52 / 894) and 8.0% for optimise (7 / 88).

A v2 lens is compared against the baseline of its `v1_category`, the category `export-v1` writes for it. Lenses without one (the plan family, `contrarian`) have no outcome baseline.

### Vet drops

v1's orchestrator vet sampled each lens's output after it returned and dropped what failed. `dropped / sampled` per lens, from the `vet_events` rows carrying `lens`, `sampled_count` and `dropped_count` (14 older review-ledger events on a different shape are excluded):

| v1 lens | Sampled | Dropped | Vet-drop | Downgraded |
|---|---|---|---|---|
| package-quality | 77 | 18 | 23% | 2 |
| completeness | 119 | 20 | 17% | 1 |
| architecture | 104 | 16 | 15% | 11 |
| security | 68 | 8 | 12% | 3 |
| testability | 125 | 11 | 9% | 5 |
| quality | 105 | 7 | 7% | 3 |
| review-plan, all lenses | 369 | 8 | 2.2% | 10 |
| optimise, all lenses | 154 | 0 | 0% | 4 |

`dropped` mixes pre-persist pruning with spot-check failures, so it can exceed what a sample would predict. A v2 lens reads against the vet-drop of the v1 lens owning its `v1_category`; for `db`, which v1's architecture lens raised, use architecture's.

### Orchestrator context

| Command | Peak context |
|---|---|
| `/review` + `/review-apply` | 401k tokens |
| `/review-plan` | 329k tokens |

Measured 2026-10-12 over the 60 most recent flow transcripts. The scripts that produced these were scratch and never committed, so the method is recorded here and is the method of record: for each session, take its top-level transcript `projects/<project dir>/<session>.jsonl` in the Claude config dir (`CLAUDE_CONFIG_DIR`, default `~/.claude`; subagent transcripts live under `<session>/subagents/` and are excluded), sum the characters of every main-thread tool input and tool result, and divide by 4 for tokens. The peak is the largest such total among the sessions that ran the command. Measure a v2 run's orchestrator context the same way, over the session that ran it, and record it with its date.

## Thresholds

Each threshold applies per lens over at least five runs, and to an outcome rate only once the lens has at least 20 resolved outcomes; below either, the result is `insufficient`, not better or worse.

| Measure | Better than v1 | Worse than v1 |
|---|---|---|
| Resolved precision per lens | at or above its `v1_category`'s precision | more than 5 points below it, or below 85% |
| Post-verification leak (wontfix + verified-clean share of exported outcomes) | ≤ 5.8% code, ≤ 8.0% perf | > 8.7% code, > 12.0% perf (1.5× v1) |
| Verifier refute rate per lens, against its v1 vet-drop | at or below the v1 lens's vet-drop | above twice it, or above 30% |
| Cleanup share of a judgement lens's defect and cleanup output | 48% or more (v1's lowest suggestion share) | below 40%: small details are being lost |
| Orchestrator context, `/review-v2` + `/review-apply` | ≤ 300k (25% under v1) | > 401k |
| Orchestrator context, `/review-plan-v2` | ≤ 250k (25% under v1) | > 329k |
| Seeded recall (below) | 4 or 5 of 5 hits | 2 or fewer of 5 |

Thresholds with no v1 counterpart, because v1 never measured them:

- **Audit overturn on CONFIRMED defects**: up to 10% is healthy; above 20% means the verifier concedes too easily, and its procedure, not the finder, is what gets tuned.
- **Audit overturn on REFUTED defects**: above 20% is recall loss — the verifier is refuting true findings. Tune the verifier before any finder.
- **Audit overturn on checked cleanup**: above 15% means the checker's predicates or the routing globs let behaviour-changing edits reach Haiku.
- **Escapes**: a concern with escapes in two or more of its last five runs is under-sliced or under-bundled; look at how compose slices it before touching the lens.
- **Audit overturn per agent model** (`review stats --by model`): the same 10% and 20% bands, read per model. A model above them while another model in the same role sits inside them points at the model assignment in the agent's frontmatter before the verifier's procedure.
- **Plan family**: v1 merged 98% of its plan findings and discarded none, so the merge measured nothing. A v2 plan lens is worse when its audit overturn on CONFIRMED defects exceeds 20%, or when its user-veto rate in `review stats --plan-outcomes` (user-vetoed over user-accepted plus user-vetoed) exceeds a third. Plan seeded recall (below) holds the plan family to the same bands as code seeded recall.
- **Cost**: a run whose `agents` exceeds the proposal's estimate by more than half, or exceeds 25 without the composed map stating why, is a composition finding for compose, not a lens finding.

A **better** result on a measure is evidence to keep a lens's current text and activation; a **worse** one names the lens, facet or activation to rework, which ships as a new `experimental` id opted into beside the old one rather than as an edit in place.

## Seeded recall check

A small, repeatable recall probe run after merge and again after any large catalogue change.

1. Pick five v1 `fixed` items, from at least three categories, whose fix landed in a single commit you can name.
2. For each, create a scratch worktree at the fix commit's parent, outside this checkout.
3. In that worktree, run `/review-v2 --depth quick <the item's file>`.
4. Score a **hit** when the run holds a defect or cleanup finding at the item's file whose `symbol` matches the v1 item's symbol, or, when the v1 item has none, whose `line` is within 10 of the v1 item's line. An advisory at the same place is a partial hit and is scored a miss.
5. Remove the scratch worktree.

v1 found all five by construction, so the bar is set on v2 alone: 4 or 5 hits passes, 3 is inconclusive (repeat with five more items), 2 or fewer fails and points at the escapes and slicing of the items' concerns.

Record each check below, newest first, one row per item:

| Date | v1 id | Ledger | Category | File | Symbol | Fix commit | v2 run | Hit | Notes |
|---|---|---|---|---|---|---|---|---|---|

No check has been run yet.

## Plan seeded recall

No v2 plan has run under an execution engine yet, so no deviation, revert or post-approval change exists to join plan findings against. Until one does, plan review is measured by the run measures from `review stats --plan-outcomes` and `review stats --by model`, and by this probe, run after merge and again after any change to the plan lenses, the plan checks or the tier rules.

1. Pick a v2 flow that was later approved, and create a scratch worktree, outside this checkout, at the commit just before its approval, so its `plan.toml` holds no `[[approval]]` and it reviews as a draft. Delete the flow's `review-v2.toml` there if one exists, so the probe's run is the store's only one.
2. Seed five defects into that flow through the store's own write verbs, never by hand-editing the TOML, one per row below, each on a different task or record. Note each seeded target.
3. Re-record the probes (`plan probes --emit`, run the script, `plan probes --record`), so the vacuous entry carries a stored verdict; a missing verdict is no proof, and that seed would then score its tier as a miss for the wrong reason.
4. In that worktree, run `/review-plan-v2` on the flow at its default depth, and stop before any edit is applied.
5. Score a **hit** when the run holds a defect or cleanup finding whose `target` names the seeded record and field. An advisory at the same target is a partial hit and is scored a miss. Separately, record whether the hit's computed `tier` matches the expected one; a tier mismatch is a finding against the tier rules, not a recall miss.
6. Remove the scratch worktree.

| Seed | Target | Expected tier | Why |
|---|---|---|---|
| A vacuous `accept` entry: a check that passes whatever the task does | `task:<ref>.spec.accept[<n>]` | mechanical | its recorded probe verdict is not `healthy` (`proof {kind:"probe"}`) |
| A dead `design` anchor | `task:<ref>.spec.design` | mechanical | `plan check` reports `plan/design-anchor-dead` on the task (`proof {kind:"check-class"}`) |
| A wrong `files` path, not a `(new)` file | `task:<ref>.files` | mechanical | the path is absent from the tree (`proof {kind:"path-missing"}`) |
| An overstated `outcome`, claiming more than the task's `accept` entries bind | `task:<ref>.spec.outcome` | user | every `spec.outcome` target is user tier |
| A decision whose evidence the repo contradicts | `decision:D<n>.<key>` | user | every `decision:*` target is user tier |

The bands are the code check's: 4 or 5 hits passes, 3 is inconclusive (repeat with five more seeds on another flow), 2 or fewer fails. A miss on a mechanical seed points at the check or lens that should have raised it; a hit at the wrong tier points at `tier.rs`.

Record each check below, newest first, one row per seed:

| Date | Flow | Base commit | Seed | Target | Expected tier | v2 run | Hit | Computed tier | Notes |
|---|---|---|---|---|---|---|---|---|---|

No check has been run yet.

## Contract note for the execution engine

When the v2 execution engine plan joins plan review to execution, these rules bind it:

- **A deviation is an escape only with a cause.** Count a deviation against plan review only when its record carries a `plan-defect` cause, which the engine's deviation writer has to stamp. A deviation with no cause, or any other cause, is not an escape, and any escape column built before causes exist is labelled "candidate".
- **Reviewed and skipped plans side by side.** Report escapes, deviations and post-approval changes for plans whose `[[approval]]` records review as run next to those recorded as skipped, as two populations. Never report the difference as an effect size: which plans get skipped is the planner's choice, not a random assignment.
- **Reverts join on `(ref, field)`.** A post-approval `[[changes]]` row whose `new` equals a finding's `applied.old` on the same ref and field reverts that finding's edit; the part of a `target` before `[` is that field name.
