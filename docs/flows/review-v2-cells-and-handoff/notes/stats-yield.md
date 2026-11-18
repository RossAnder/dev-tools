Verdict: `review stats` lumps every audited row into one `audited`/`overturned` pair (tomlctl/src/review/stats.rs:376 `overturn`, summed at :439 and :571) and never reads a cell's `pass` or `agent` for yield; all the inputs a split and a yield section need are already stored (Cell.pass/agent store.rs:149,153; Finding.same_as/outcome store.rs:198,202; merged_into via resolve.rs:519; apply links fields.rs:239), so both are additive, read-only changes in stats.rs plus docs, a feature flag and tests, with no schema change.

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 376
claim = "overturn(): latest non-unsettled audit verdict vs the verifying call; no kind/route/class split"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 431
claim = "Tally::add_finding sums audited/overturned over every finding kind into one pair (by lens/probe/run)"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 541
claim = "by_model keys on the deciding verifier's run `models` role and sums audited/overturned into one pair"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 365
claim = "deciding_verifier returns (refuted, deciding verdict) — the bool and `by` a class split needs"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 819
claim = "apply_state: scheduled / resolved-with-apply_flow / reopened, read from the finding row"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 1023
claim = "the only place stats reads cell.agent (blocked rates); `pass` is never read in stats.rs"

[[source]]
path = "tomlctl/src/review/digest.rs"
line = 417
claim = "audit_sample defines the three classes: standing defects, REFUTED non-critical defects, checker-decided cleanup"

[[source]]
path = "tomlctl/src/review/store.rs"
line = 145
claim = "Cell carries run, wave, pass (Option<u32>, absent = 1) and agent"

[[source]]
path = "tomlctl/src/review/store.rs"
line = 185
claim = "Finding carries run, cell, seen_in, same_as, outcome, status; merged_into/apply_flow live in rest"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 885
claim = "repeats(): standard repeats floor and hotspot cells, deep hotspot or signal cells, with pass = 2 and reversed slice"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1346
claim = "review-finder-cf is accepted only on a cell whose concerns are exactly [\"correctness\"]"

[[source]]
path = "tomlctl/src/review/resolve.rs"
line = 476
claim = "resolve apply(): a same_as row becomes merged with merged_into = target and outcome repeat/contested/reopened/suppressed; regression stays open"

[[source]]
path = "tomlctl/src/review/add.rs"
line = 891
claim = "review add sets `related` (not same_as) when anchor candidates exist and the finder named none"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 118
claim = "FindingStatus values incl. merged, exported, scheduled, resolved"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 156
claim = "orchestrator links cross-cell duplicates with --same-as, the later-verified or weaker-evidenced row as the duplicate"

[[source]]
path = "claude/skills/review-v2-engine/references/measurement.md"
line = 142
claim = "audit bands need 30 audited rows per class; stats' `audited` sums the three classes, so per-class counts are done by hand today"

[[source]]
path = "claude/skills/tomlctl/references/review.md"
line = 419
claim = "documented stats row and --by model shapes a split must extend"

[[source]]
path = "tomlctl/tests/review_stats.rs"
line = 442
claim = "integration tests pin audited/overturned/audit_overturn_rate per lens and per model"

[[source]]
path = "tomlctl/src/cli/types/mod.rs"
line = 194
claim = "FEATURES list where a new stats capability is advertised (review_stats_apply precedent)"
```

## Answer

**How audit rows are computed today.** `overturn` (stats.rs:376) takes the latest `by = "audit"` verdict that is not `unsettled`, compares it with `verifier_call` (the `settle()` outcome over `verifier`/`panel`/`checker` verdicts only, stats.rs:359), and counts the row audited when both exist, overturned when they disagree on refuted-vs-standing. `Tally::add_finding` (stats.rs:431) adds that per finding into one `audited`/`overturned` pair for `--by lens|probe|run`; `by_model` (stats.rs:541) keys the same pair on `models.<role>` of the run the deciding verdict names. No kind, route or verifier-side outcome enters the key, so CONFIRMED defects, REFUTED defects and checked cleanup are summed, and measurement.md:142 tells the reader to recount each class by hand from `review list --for audit`.

**Where the yield inputs live.**
- Cell `pass`: `Cell.pass: Option<u32>` (store.rs:149), range 1–2, written only when 2 (cells.rs:834); `None` reads as 1. Repeat cells are minted by `repeats()` (cells.rs:885).
- Cell agent: `Cell.agent` (store.rs:153); `review-finder-cf` is allowed only on a cell whose concerns are exactly `["correctness"]` (cells.rs:1346). A finding reaches both through `Finding.cell` (store.rs:188).
- `same_as`: `Finding.same_as` (store.rs:198). A finder sets it at `review add` only against an anchor candidate (add.rs:863); the cf finder reads no prior rows (correctness.md:51), so its duplicates arrive only through the orchestrator's `review resolve --same-as DUP=TARGET` (compose.md:156). After `resolve`, the duplicate is `merged` with `merged_into` and an `outcome` (`repeat`, `contested`, `reopened`, `suppressed`), or stays `open` as a `regression` with `related` (resolve.rs:476–523). A candidate that was not linked only gets `related` (add.rs:891).
- Apply outcome: `apply_flow`/`apply_task` on the row (fields.rs:239), with status `scheduled` or `resolved`, read by `apply_state` (stats.rs:819). A v1 export is `exported_as` plus the v1 join in `v1_links` (stats.rs:716).

**What a split takes (small, one file plus tests and docs).** In `overturn`, or in a helper beside it, classify the audited row with the predicates `audit_sample` already uses (digest.rs:417): `defect` + standing → `confirmed_defects`, `defect` + refuted → `refuted_defects`, `cleanup` + deciding `by == "checker"` → `checked_cleanup`, anything else → `other`. `deciding_verifier` already returns the refuted bool and the deciding verdict's `by` (stats.rs:365). Emit a per-class `{audited, overturned, rate}` map beside the existing totals in `Tally::to_json` and `by_model`, keeping the existing keys so the tests at review_stats.rs:442 and :1380 still hold. Two choices for the planner: digest classifies by the *latest non-audit verdict* and stats by `settle()`, which differ only on panels; and `audit_sample` excludes refuted criticals from its class. Pick one rule and state it in review.md.

**What a yield report takes (moderate; needs a decision, one file).** Add a section flag, e.g. `--yield`, in stats.rs. For each run (and pooled over the view), give each finding the arm of its cell: `first` (pass 1, not cf), `repeat` (pass 2) or `cf` (agent `review-finder-cf`). Count **verified** as `verifier_call == Some(false)`. Count a finding **unique** when no row of the same run in another arm is linked to it by identity. Two facts make attribution symmetric, not "the row with `same_as`": the orchestrator chooses the *later-verified or weaker-evidenced* row as the duplicate, so a first-pass row can be the one merged into a repeat row (compose.md:156); and repeat cells share wave 1 with their first-pass twins (they come from one `--propose`), so a finder's PRIOR seldom holds the twin. So the unit is the identity cluster: the union of `same_as`/`merged_into` edges within the run (store.rs:383 already iterates those links). A repeat or cf cluster is unique when it holds no `first` member. A cross-run `same_as` (outcome `repeat` against an older run) is not new yield and should be reported apart. The **applied share** is the share of unique clusters whose root has `apply_state` `scheduled`/`resolved`, or `exported`/v1-fixed under `--outcomes`. Report per-run rows with `store`, `run`, `depth` and the three arms' `{cells, findings, verified, unique, applied}`, then pooled totals, so the five-run rule in measurement.md:5 can be applied. Data to read already exists: 7 flow stores hold `pass = 2` cells and 4 hold `review-finder-cf` cells (rg below).

**Ripple.** New FEATURES entry (cli/types/mod.rs:194, precedent `review_stats_apply`) with its case in `tests/capabilities.rs` and `tests/output_options.rs`, the flag table in review.md:415, README's feature table, and measurement.md's Signals table and the hand-count paragraph at :142.

## Findings

- **Library/API or file**: tomlctl/src/review/stats.rs:376 `overturn`, summed at :439 (`Tally::add_finding`) and :571 (`by_model`)
- **Source**: file:line read; digest.rs:417 `audit_sample`; measurement.md:142
- **Evidence-grade**: high
- **Finding**: Audit counts are not split by class; add a per-class map keyed by the `audit_sample` predicates beside the existing totals.
- **Details**: `overturn` returns `(audited, overturn verdict)` with no kind or route. `deciding_verifier` (stats.rs:365) already yields the refuted bool and the deciding `by`, so a classifier `(finding.kind, refuted, deciding.by)` → `confirmed_defects | refuted_defects | checked_cleanup | other` slots in without new store reads. The plain `audited` also counts verifier-routed cleanup and plan-family cleanup, which none of the three bands covers, hence the `other` bucket.
- **Counter**: If the planner wants the classes to match the digest's sample exactly, including excluding refuted criticals and judging by the latest non-audit verdict rather than `settle()`, the classifier must call digest's `judgement` rule instead, and panel rows may then land in different classes.
- **Impact on plan / report**: One task in stats.rs plus review_stats.rs tests and the review.md and measurement.md lines; existing keys stay, so no consumer breaks.

- **Library/API or file**: tomlctl/src/review/store.rs:149 `Cell.pass`, :153 `Cell.agent`; stats.rs (no read of `pass` anywhere; `agent` read only at :1023)
- **Source**: Grep `\bpass\b` over tomlctl/src/review/ shows no stats.rs hit; `rg -c '^pass = 2'` shows 7 flow stores, `rg -c 'agent = "review-finder-cf"'` shows 4
- **Evidence-grade**: high
- **Finding**: No stats section reports yield by pass or by cf agent; the stored fields suffice for a new read-only `--yield` (name open) section.
- **Details**: The arm comes from `Finding.cell` → `Cell.pass.unwrap_or(1)` and `Cell.agent == FINDER_CF` (fields.rs:283). The per-run grouping can reuse `by_run`'s run loop (stats.rs:675), and the pooled grouping a merge like `PlanTally::merge`.
- **Counter**: A run that composed cf or repeat cells by hand without the `pass` or `agent` fields would be misattributed to `first`; cells.rs:1346 and fields.rs:590 refuse an unknown agent or pass, which bounds this risk.
- **Impact on plan / report**: Defines the measurement milestone's main task; needs a feature flag for the engine pre-flight list (review-v2-engine SKILL.md:78) only if the engine will call it.

- **Library/API or file**: tomlctl/src/review/resolve.rs:476 `apply`; claude/skills/review-v2-engine/references/compose.md:156
- **Source**: file:line read
- **Evidence-grade**: high
- **Finding**: "Unique beyond first pass" must be computed over same-run identity clusters, not from which row carries `same_as`. needs-plan (attribution rule is a decision).
- **Details**: The orchestrator names the later-verified or weaker-evidenced row as the duplicate, so a first-pass row can end up `merged_into` a repeat or cf row. `resolve` then sets the duplicate `merged` (outcome `repeat`) or keeps a `regression` `open` with `related`. Only a cluster with no pass-1, non-cf member is new yield. Pairs the orchestrator judged distinct keep both rows and both count, and an unlinked `related` candidate (add.rs:891) is not identity.
- **Counter**: If the planner accepts "the row without `same_as` gets the credit" as good enough, the cluster walk is unnecessary. Store data would show how often a first-pass row was the linked duplicate: run `tomlctl review list` over the 7 stores and compare each `merged` row's cell pass with its target's.
- **Impact on plan / report**: Fixes the definition the stats task and measurement.md must state; the applied share then reads `apply_state` (stats.rs:819) on the cluster root.

Coverage: read in full tomlctl/src/review/stats.rs (1–1500 of non-test code) and claude/skills/review-v2-engine/references/measurement.md; read the relevant parts of store.rs (Run/Cell/Finding/Verdict), cells.rs (bundle/repeats/replayed_wave/cf rule), digest.rs (audit_sample), resolve.rs (header, apply), add.rs (finish_row), fields.rs (statuses, link fields), compose.md (duplicates), review-v2 SKILL.md depth table, and tomlctl references/review.md (stats). Checks run: Grep for pass/finder-cf/same_as/audit across tomlctl/src/review and claude/skills; rg counts of `pass = 2`, cf cells and `same_as` in the flow stores; git status on the cited paths (clean). Ruled out: a schema change (every field already stored); plan-family `rechecks` yield (already in `--plan-outcomes`, a different repeat mechanism); escapes (cross-run recall, not in-run yield).
