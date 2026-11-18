Verdict: Not enforced today: `append` (tomlctl/src/review/cells.rs:1445) never calls `ceiling` (cells.rs:144). Enforce it inside the minting closure at cells.rs:1490 as all-or-nothing: count the run's non-executor cells from every wave plus this call's accepted non-executor rows (context-free rows included) against `ceiling(run)`, and skip the check on a replay. Record a user-directed override as a computed run table written by the same call, rendered beside Waivers (digest.rs:907) and counted in `by_run` (stats.rs:672).

```toml sources
read_at = "00ff2113dbfab0b7db0b98df71442194340b81c6"
dirty = []

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1445
claim = "append validates rows, splits them into ready and refused, and mints the ready rows as the next wave with no ceiling check"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 144
claim = "ceiling(run) is min(depth ceiling, max(3, ceil(changed_lines/100))) for code and perf, and the depth ceiling for plan"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1064
claim = "propose counts planned cells as the run's cells with agent != EXECUTOR, every wave, check cells included"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1484
claim = "replayed_wave short-circuits before minting and again inside the closure on the re-read store"

[[source]]
path = "tomlctl/src/review/cells.rs"
line = 1348
claim = "context_free_variant: review-finder-cf is accepted only on a cell whose sole concern is correctness"

[[source]]
path = "tomlctl/src/review/store.rs"
line = 1532
claim = "mutate_minting runs mint on the raw doc and then edit on the parsed store under the same write, so a run-field write can go in edit"

[[source]]
path = "tomlctl/src/review/runs.rs"
line = 808
claim = "run finish's --incomplete reason is the precedent: read_prose flag pair, stored in run.rest, a later clean finish clears it"

[[source]]
path = "tomlctl/src/review/fields.rs"
line = 585
claim = "incomplete is registered as a computed Run field, writable only by its verb"

[[source]]
path = "tomlctl/src/review/runs.rs"
line = 186
claim = "run update's patchable fields are signals, waivers, audit, agents, baselines; an override reason is not among them"

[[source]]
path = "tomlctl/src/review/digest.rs"
line = 907
claim = "the report's coverage section renders cells and then a Waivers section; run_line (digest.rs:703) carries run metadata"

[[source]]
path = "tomlctl/src/review/stats.rs"
line = 672
claim = "by_run builds one row per run with waves, agents and a waivers count; the natural slot for override counts"

[[source]]
path = "claude/commands/review-v2.md"
line = 49
claim = "the context-free correctness cell is written into the wave-1 cell file with the composed rows and counts against the ceiling"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 83
claim = "the ceiling counts every wave, repeat cells included; plan-executor cells sit outside it; added cells must stay within it"

[[source]]
path = "claude/skills/review-v2-engine/references/compose.md"
line = 192
claim = "at quick, the escalation probe cell counts against the ceiling, yet the engine says it still gets one Opus cell"

[[source]]
path = "claude/skills/review-v2-engine/SKILL.md"
line = 220
claim = "the depth table's ceiling row says --propose scales it, and leaves open whether the scaled value or the depth cap binds later waves"

[[source]]
path = "claude/agents/plan-review-lead.md"
line = 109
claim = "plan-review-lead writes follow-up cells through review plan --ndjson and cannot ask the user"

[[source]]
path = "tomlctl/tests/review_cells.rs"
line = 817
claim = "an existing test asserts that executor cells sit outside the ceiling count"
```

## Answer

1. **Count.** Use the predicate `propose` already uses (cells.rs:1064): the run's existing cells whose `agent != EXECUTOR`, across every wave, sealed or not, check and probe cells included, plus this call's `ready` rows whose agent is not `plan-executor`. The carrier's `review-finder-cf` rows reach `append` as ordinary rows in the wave-1 file, so they count with no special case. Follow-up waves count because the existing-cell term covers every earlier wave. Refused rows are not counted.
2. **Where.** Put the check in the `mint` closure (cells.rs:1490), after its own `replayed_wave` check and against `current`. Checking there means two concurrent appends cannot both pass, and a replay, whose cells are already stored, is never refused for counting itself twice.
3. **Refuse the whole batch.** The current partial write, where good rows mint and bad rows land in `refused`, is wrong for the ceiling: which cell to cut is the orchestrator's judgement (floor cells first, then the context-free cell, then candidate cells and repeats, per compose.md:83 and review-v2.md:56). The error should name the ceiling, how many cells are already planned, how many this call adds, and the overflow, and should write nothing.
4. **Which number.** Reuse `ceiling(run)` so that `propose` and `append` agree, since the proposal's `budget = ceiling - planned` already treats the scaled value as run-wide. On a small diff this leaves no room for a follow-up wave or the context-free cell. The SKILL.md:220 table does not settle which number binds later waves, so that call is the planner's.
5. **Override.** Add a flag pair on the `--ndjson` mode, like `--over-ceiling <reason>` / `--over-ceiling-file`, built on `read_prose` (the `--incomplete` precedent, runs.rs:808). It should conflict with `--propose`. It writes, in the `edit` closure of the same `mutate_minting`, one entry per overriding wave to a computed run table list such as `ceiling_overrides = [{wave, ceiling, planned, added, reason}]`. Register that list in fields.rs as `Ty::TableList` and `.computed()`, as `incomplete` is (fields.rs:585), and do not add it to `run update`'s `UPDATE_FIELDS`, so the verb that broke the ceiling is the only writer. Leave it in `run.rest` (no `Run` struct change needed); `ordered()` places it by catalogue order. Store it only when the batch actually exceeds the ceiling, so a flag passed on a batch that fits records nothing.
6. **Surfacing.** In the report, add a `## Ceiling overrides` section after Waivers in `coverage()` (digest.rs:907), or a `ceiling over by N` part in `run_line` (digest.rs:703). In stats, add `row["ceiling_overrides"]` (count and total overflow) to `by_run` (stats.rs:672). An aggregate across runs (`override_rate`) sits beside `waiver_rate` if the measurement lens wants it.

## Findings

- **Library/API or file**: tomlctl/src/review/cells.rs:1445 `append`
- **Source**: file read; `rg -n "ceiling" tomlctl/src/review` finds `ceiling(` only in `propose` (cells.rs:1063) and its helpers
- **Evidence-grade**: high
- **Finding**: `review plan --ndjson` mints any number of cells past the run's ceiling. Add an all-or-nothing count check inside the mint closure.
- **Details**: `append` prepares rows, checks for a replay, then calls `store::mint(doc, Minted::Cells, &ready)`. `ceiling`/`depth_ceiling` are reached only from `propose`, and add.rs:11 states that per-cell caps "are not enforced here". `Minted::Cells` is minted only at cells.rs:1501, so this one site covers every cell write (carrier context-free rows, follow-ups, plan-review-lead waves, the measurement.md:179 precision probe).
- **Counter**: Refuted if some other verb (`run update`, `run finish`, `digest`) refuses a run whose non-executor cell count is over `ceiling(run)`. The `rg` sweep found none.
- **Impact on plan / report**: Touches cells.rs (`append`, a shared count helper drawn from `propose`'s `planned`) and tests/review_cells.rs (over-ceiling refusal, a replay still accepted, executor rows exempt, context-free row counted, follow-up wave counted).

- **Library/API or file**: tomlctl/src/review/cells.rs:1064 `propose` `planned` predicate, and tests/review_cells.rs:817
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The exemption rule already exists as `c.agent != EXECUTOR`. Lift it into one helper so `propose`'s budget and `append`'s check cannot drift.
- **Details**: `plan_executor_cells_batch_tasks_outside_ceiling` asserts that `summary["cells"]` excludes plan-executor rows. `append` should exclude incoming rows whose `agent` is `plan-executor` by the same test (the `EXECUTOR` const in fields.rs).
- **Counter**: Refuted if plan-executor cells were meant to have a cap of their own on append. The only cap they have is the task cap `executor_task_cap` (cells.rs:134), which counts tasks, not cells.
- **Impact on plan / report**: One shared helper in cells.rs. No doc change, because compose.md:83 and SKILL.md:231 already say executor cells sit outside the ceiling.

- **Library/API or file**: claude/commands/review-v2.md:49-56, with cells.rs:1069 `budget`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: The proposal fills the budget and the carrier then adds context-free rows to the same file, so on most standard and deep runs, enforcement turns a silent overflow into a refusal. The carrier and engine prose must say to cut a candidate or repeat cell first, or `--propose` must reserve the slot.
- **Details**: `propose` emits up to `ceiling - planned` cells and knows nothing about the carrier's context-free cell. review-v2.md:56 already ranks that cell above candidate cells and repeats, so the refusal enforces existing policy, but the wave-1 write will fail unless stage 3 cuts first. Reserving a slot in `--propose` belongs to the sibling proposal lens and is mentioned here only as an alternative.
- **Counter**: Refuted if the stored runs show wave-1 files that already leave room. B-f00449a1's run U5 filled all 16 standard cells in wave 1, so they do not.
- **Impact on plan / report**: Pair the enforcement task with a prose edit to review-v2.md (Ceiling bullet) and compose.md section 3 (The ceiling) naming the refusal. Otherwise the first run after the change stalls at stage 3.

- **Library/API or file**: claude/skills/review-v2-engine/references/compose.md:192 and SKILL.md:231
- **Source**: file read
- **Evidence-grade**: medium (prose reading; what the orchestrator does at run time is not observed)
- **Finding**: The `quick` escalation probe "counts against the ceiling" yet "still gets one Opus cell". A hard check on append contradicts the second clause once quick's 6-cell or scaled 3-cell ceiling is full. The plan must either route that probe through the override or exempt it explicitly.
- **Details**: At quick the scaled ceiling can be as low as 3 (`max(3, ...)`), and a full wave 1 is the usual case. Without a decision, the guarantee in SKILL.md:231 becomes a refusal.
- **Counter**: Refuted if the engine already cuts a wave-1 cell before the probe at quick. compose.md section 7 says nothing of the kind.
- **Impact on plan / report**: A decision for the planner (override vs exemption). An exemption needs a marker that `append` can check, such as a reserved reason tag, which the override avoids.

- **Library/API or file**: claude/agents/plan-review-lead.md:109 and :145
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: plan-review-lead writes waves through `review plan --ndjson` but cannot ask the user, so it must never pass the override flag. On refusal it cuts cells itself or returns. The override is the engine orchestrator's alone, after the user has been asked.
- **Details**: The lead's allowed writes are listed at :145. A "user-directed" override needs a user, and only the attended orchestrator has one. The lead's prose and the engine's follow-up step should say the override follows an explicit user answer, quoted into the reason.
- **Counter**: Refuted if the plan-v2-review Workflow lets the lead put questions to the user. Its lead is dispatched one-shot, and it can only return.
- **Impact on plan / report**: Prose edits to plan-review-lead.md (step 6, Follow-up) and to compose.md's follow-up section.

- **Library/API or file**: tomlctl/src/review/runs.rs:808 `finish` (`incomplete`), fields.rs:585, digest.rs:907 `coverage`, stats.rs:672 `by_run`
- **Source**: file read
- **Evidence-grade**: high
- **Finding**: Store the override as a computed run table list written only by `review plan`, render it beside Waivers, and count it per run in `review stats`.
- **Details**: `incomplete` is a computed `Run` field held in `run.rest` and written only by `run finish` from a `read_prose` flag pair. A `ceiling_overrides` TableList should follow that pattern, written in `mutate_minting`'s `edit` closure (store.rs:1536), which `append` currently passes as `|_, minted| Ok(minted)`. The report already lists waivers after the coverage table, and `by_run` already reports `waivers` counts, so the override sits in both places. compose.md:128 asks for budget-driven waivers to say `ceiling: …`. That is a separate, softer squeeze signal that stats would still have to parse from text.
- **Counter**: Refuted if `run update` should own the reason. That would let the override be recorded apart from the write it justifies, and `UPDATE_FIELDS` (runs.rs:186) has no such slot today.
- **Impact on plan / report**: Touches fields.rs (field spec), cells.rs (flag and write), digest.rs (section), stats.rs (`by_run` key), tomlctl's review.md flag table and `review stats` docs, and tests in review_cells.rs, review_digest.rs and review_stats.rs. A new flag on an existing leaf verb is not a new verb, so tests/output_options.rs is likely untouched; confirm when writing the task.

Coverage: read cells.rs in part (header, `plan`, ceiling helpers, `propose` budget, `prepare_cell`, `append`, `replayed_wave`), store.rs (`Run`, `RUN_CORE`, `rest`/`ordered`, `mutate_minting`), runs.rs (`UPDATE_FIELDS`, `update`, `finish`), fields.rs Run specs, digest.rs (`render`, `run_line`, `coverage`), stats.rs `by_run`, review-v2.md stage 3, compose.md (ceiling, follow-up), SKILL.md depth table, plan-review-lead.md, the review.md `review plan` docs, and the review_cells.rs ceiling tests. Ran the `rg` sweeps for `ceiling` and `Minted::Cells`. Ruled out: a second cell-minting path (none exists); `run update` as the override writer (it splits the reason from the write); refusing per row (cannot choose which cell to cut). Not checked: whether `review report` should also render `incomplete`, which it does not today and is outside this lens.
