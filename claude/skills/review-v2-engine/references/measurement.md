# Measurement and tuning

v2 is trialled in use rather than compared against v1 run for run, so its quality is judged by signals that come from outside the reviewers: what happened to exported findings, what later runs found in places earlier runs called covered, what the audit overturned, and what the orchestrator spent. This reference names those signals, the v1 baselines they are read against, and the numbers that count as better or worse.

No lens, facet or activation tier is retuned on fewer than **five runs** of evidence for it. Below that, record what you see and wait.

## Signals

| Signal | Read with | What it measures |
|---|---|---|
| Apply outcomes | `review stats --outcomes` | Exported findings joined to their v1 status through `v2_ref` — fixed, wontfix with reasons, deferred, verified-clean — per lens and kind |
| Escapes | `review stats --escapes` | Findings minted in a file and concern that an earlier run's cell had sealed `covered`: the recall proxy for composed coverage |
| Refute rate | `review stats --by lens` | Share of a lens's verified rows its verifiers refuted |
| Audit overturns | `review stats --by lens` | Overturn rate on CONFIRMED and REFUTED defects and on checked cleanup, per lens and per agent model |
| Dismissals | `review stats --by lens` | Share of a lens's rows the user dismissed at triage, with reasons |
| Coverage | `review stats --by run` | Uncovered and thin cells, waiver rates, per run |
| Probe recurrence | `review stats --by probe` | Probes recurring under one concern tag across runs — a promotion candidate at three or more |
| Calibration | `review stats --calibration` | Exemplars drawn only from user dismissals, v1 wontfix reasons and audit overturns |
| Cost | the run record (`agents`, `waves`) and the context measurement below | Agents and waves per run; orchestrator context per run |

`review sync-v1` runs at every run's stage 0, so the store is current before any of these is read, and a re-found fixed issue resolves as a **regression**, not a repeat. `stats` writes nothing.

The review across all stores, after five or more runs:

```bash
tomlctl review stats --all --outcomes --escapes --calibration
tomlctl review stats --all --by lens
tomlctl review stats --all --by probe
tomlctl review stats --all --family perf --by lens
```

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

Measured 2026-10-12 over the 60 most recent flow transcripts. The scripts that produced these were scratch and never committed, so the method is recorded here and is the method of record: for each session, take its top-level transcript `~/.claude/projects/<project dir>/<session>.jsonl` (subagent transcripts live under `<session>/subagents/` and are excluded), sum the characters of every main-thread tool input and tool result, and divide by 4 for tokens. The peak is the largest such total among the sessions that ran the command. Measure a v2 run's orchestrator context the same way, over the session that ran it, and record it with its date.

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
- **Plan family**: v1 merged 98% of its plan findings and discarded none, so the merge measured nothing. A v2 plan lens is worse when its audit overturn on CONFIRMED defects exceeds 20%, or when the user dismisses more than a third of its rows at the merge offer.
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
