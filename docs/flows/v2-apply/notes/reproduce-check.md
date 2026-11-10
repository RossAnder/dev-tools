Verdict: It can run as a new drain step between the replan and the gate (drain.md:214 to :216), launched as a top-level `review-v2-pipeline` run. But the store cannot hold the check's verdict as an ordinary `[[verdicts]]` row. Upsert is keyed (finding, by, seat) at store.rs:230, the row copies the finding's own run at verdict.rs:252, and a `refuted` sets the finding's status at verdict.rs:264-281. So the check needs its own run row (a new `command`), a cell that names the task and its findings, and a verdict record keyed on its own run, using its own vocabulary, that no settle reader counts.

# reproduce-check

## Answer

**Where it runs.** Drain step 6 already ends with "every row of the milestone is `done`, go to step 7" (`claude/skills/implement-v2-engine/references/drain.md:214`). The check fits there as a new step:

1. Start one review run per drain round.
2. Plan one cell per apply task.
3. Launch `review-v2-pipeline` from the orchestrator.
4. Read the verdicts back.
5. A finding that still reproduces mints a fix task through the existing "New tasks" shape (drain.md:160-188) before the gate.

The drain runs in the orchestrator conversation, so the pipeline launch is top-level and nothing nests. The workers' edits are still uncommitted at this point, and they share one tree. Each cell therefore judges the whole milestone's working tree, not its own task's edits alone.

**What the store needs.** Reusing `[[verdicts]]` unchanged fails in four ways:

- **It overwrites.** The upsert key has no run or purpose (store.rs:230-240). A `--by verifier` re-check replaces the finding's original verifier verdict.
- **It mis-attributes.** `review verdict` stamps `run: finding.run.clone()` (verdict.rs:252), so the row cannot name the vetting run.
- **It changes the finding's status.** At drain the finding is still `open`, because resolution waits for the gate and the commit (docs/ideas/v2-pipeline.md:169). `open` is a status a verdict decides (fields.rs:192-194). So a `refuted` meaning "gone now" flips the finding to `refuted` through `decide` and `set_status` (verdict.rs:264-273), and `review stats` then counts it as a false positive.
- **It is refused or misread.** A verifier verdict on a panel-routed finding is refused (verdict.rs:208-218). Even rows that land are read by every settle and latest-verdict reader, which groups verdicts by finding alone (sweep below).

The records therefore need:

- **Run.** A `command` value for the apply carrier, since `COMMANDS` holds only the three review commands (fields.rs:267). Plus a pointer naming what is vetted (milestone id, drain round, and the plan slug when the store is flow-less), on the pattern of `rechecks` (fields.rs:497, runs.rs:27). One run per family, since `family` is one value per run (fields.rs:475).
- **Cell.** A `task` field holding the task id, and the finding ids under check. Today a cell's agent must be one of the four finder agents (fields.rs:262, 509-511), and `review plan` derives the expected agent from the catalogue mode of the cell's first concern (cells.rs:1286-1291, 1341-1347). So a reproduce cell needs either a catalogue entry plus `review-verifier` admitted as a cell agent, or a cell kind that skips catalogue binding.
- **Verdict.** It needs all of the following:
  - The vetting run's own id, not the finding's run.
  - A key that includes that run, so the original verdict and every earlier re-check survive.
  - Its own vocabulary, e.g. `reproduces | gone | unsettled`, never `confirmed`/`refuted`.
  - No effect on the finding's status: the blocking decision lives in the engine, and resolution stays with the gate and commit.
  - Invisible to `settle` and to every latest-verdict reader.

  There are two ways to do this:
  - **(a) A separate array, e.g. `[[rechecks]]`, with its own verb or a `review verdict` mode.** None of the existing readers change. But `from_toml` drops arrays it does not know (store.rs:393-401), and its own comment at :372 says a write by an older binary silently drops what it cannot type. So this needs a `SCHEMA_VERSION` bump (store.rs:43).
  - **(b) A discriminator field on `[[verdicts]]`.** An older binary keeps the field, because unknown keys ride in `rest`. But every reader in the sweep has to filter on it.

  I lean (a): one new reader against about ten edited ones.

**What `review-v2-pipeline` needs.** It reaches verifiers only through findings a cell agent returns. Cells must be one of the four finder agents (review-v2.js:20, 154-157), and routing reads `ret.findings` (review-v2.js:168-191). The verifier prompt also hard-codes refute-first and `--by verifier` (review-v2.js:75-78). So reuse needs a reproduce mode: args carry `{cell, task, findings}` per task, the Cells phase is skipped, one `review-verifier` runs per cell under a reproduce prompt and the batch schema, and `SCRIPT_VERSION` is bumped. `pack_path` is required (review-v2.js:40) and the run directory is derived from it (:50), so the mode also has to drop that requirement or supply a pack. Plan rechecks copy the first run's pack for the same reason (plan-flow.md:302).

## Findings

- **Library/API or file**: tomlctl/src/review/store.rs:230 `Store::upsert_verdict`; tomlctl/src/review/verdict.rs:250-260 `apply_one`
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: A re-check written through `review verdict` as it stands replaces the original verdict and carries the finding's run, not the vetting run.
- **Details**: The key is `(finding, by, seat)`. A changed row is removed and pushed to the end. `Verdict.run` is set from `finding.run`. A re-check by `verifier` on a verifier-routed finding therefore overwrites the original, and nothing in the row says which run made it.
- **Counter**: Invalid if the re-check used a `by` value no original verdict uses and kept its run elsewhere. Even then, `run` would still be wrong and the next three findings would still hold.
- **Impact on plan / report**: The verdict record needs its own run and a key that includes it: option (a) or (b) above.

- **Library/API or file**: tomlctl/src/review/verdict.rs:264-281 `apply_one` / :363 `decide`; tomlctl/src/review/fields.rs:192 `decided_by_verdict`
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: A re-check verdict on an `open` finding re-decides its status, so "gone" written as `refuted` marks a real, fixed finding as a false positive.
- **Details**: `decided_by_verdict` covers `unverified`, `open` and `refuted`. Findings stay `open` until the gate and commit resolve them (docs/ideas/v2-pipeline.md:169), and the check runs before the gate, so every re-checked finding is in a status a verdict decides.
- **Counter**: Invalid if the apply path moved findings to a status outside `decided_by_verdict` before drain, such as `resolved`. The brief's success criteria forbid that until the commit (brief.md:29).
- **Impact on plan / report**: Re-check verdicts must never call `decide`/`set_status`. They need their own vocabulary, and the engine, not the store, turns "reproduces" into a fix task.

- **Library/API or file**: tomlctl/src/review/verdict.rs:208-218 `apply_one` (route rule)
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: A re-check by a single verifier on a panel-routed finding (critical defects) is refused today.
- **Details**: Only `panel` seats and `audit` may give a verdict on a panel-routed finding. A re-check through the same verb would need three seats or would fail. A separate record or a discriminator exempts it.
- **Counter**: Invalid if no apply task carries a panel-routed finding. That is unlikely, since criticals are exactly what apply fixes first.
- **Impact on plan / report**: The route rule must not apply to re-check verdicts.

- **Library/API or file**: tomlctl/src/review/fields.rs:1067 and :1121 `project`; tomlctl/src/review/verdict.rs:398 `settle`; tomlctl/src/review/resolve.rs:387-399 `decide`; tomlctl/src/review/stats.rs:245 `StoreView`; tomlctl/src/review/digest.rs:301-307 `judgement`; tomlctl/src/review/export.rs:505-512 `latest_verdict`; tomlctl/src/review/store.rs:244 `Store::severity`; tomlctl/src/review/triage.rs:356; tomlctl/src/review/query.rs:238
- **Source**: Grep `\.verdicts` over tomlctl/src (non-test), read 2026-11-10
- **Evidence-grade**: high
- **Finding**: Every reader groups verdicts by finding alone, so option (b) means filtering at all of these sites (needs-plan).
- **Details**: `settle` takes the latest non-panel verdict that is not `unsettled`. `project` shows the latest verdict and flags disagreement. `judgement`, `latest_verdict` and `severity` take the newest row. stats feeds overturn and outcome counts. sync.rs:364 matches only `by == "sync"` and is unaffected.
- **Instances**: the ten sites in the anchor. Sweep string `\.verdicts` over tomlctl/src/review/*.rs, excluding tests: complete for the review group. Sites outside it were not swept.
- **Counter**: Invalid if option (a) is taken, which touches none of these sites.
- **Impact on plan / report**: Weighs toward a separate array plus a `SCHEMA_VERSION` bump.

- **Library/API or file**: tomlctl/src/review/store.rs:43, :372-379, :393-401 `from_toml`
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: A new top-level array is dropped by an older binary's write unless `SCHEMA_VERSION` is raised.
- **Details**: `from_toml` types only `runs`, `cells`, `findings` and `verdicts`, and refuses a newer `schema_version`. Option (a) must bump it to 2 and accept version-1 stores.
- **Counter**: Invalid if the re-check rides inside an existing array, as in option (b).
- **Impact on plan / report**: Option (a) carries a schema-bump task. The carrier's pre-flight should refuse a `tomlctl` that is too old, as `/implement-v2` already does for its own features.

- **Library/API or file**: tomlctl/src/review/fields.rs:262, :267, :509-511; tomlctl/src/review/cells.rs:1286-1291, :1341-1347
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: The run and cell records cannot express a reproduce check today: `command` is limited to the three review commands, and a cell's agent must be one of the four finder agents, derived from a catalogue concern.
- **Details**: `review run start` validates `command` against `COMMANDS`. `review plan --ndjson` binds the concerns through the catalogue and requires `agent == expected_agent`, where `probe:`/`check:` concerns map to `review-finder`. Neither has a field for a task id or a finding list.
- **Counter**: Invalid if the check were modelled as a finder cell, with a `probe:` question, that re-raises a still-present finding as a new row with `same_as`. That would put a new finding, not a verdict, on the original, and `same_as` leaves the original `unverified` until `review resolve`.
- **Impact on plan / report**: New `command` value. New run fields (vetted milestone, round). New cell fields (`task`, `findings`). A cell kind or catalogue entry admitting `review-verifier`.

- **Library/API or file**: claude/workflows/review-v2.js:20, :40, :75-78, :154-157, :168-191
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: high
- **Finding**: `review-v2-pipeline` cannot run a verifier-only wave. Verifiers launch only from findings a finder cell returns, and their prompt fixes refute-first with `--by verifier`.
- **Details**: The pipeline needs a reproduce mode: per-cell `{task, findings}` in args, no Cells phase, one `review-verifier` per cell with a prompt naming the task's files and the reproduce verb, and the existing `batchSchema`. `pack_path` stays required unless the mode exempts it.
- **Counter**: Invalid if the plan chose a separate saved Workflow, which the question's reuse constraint rules out, or an Agent-wave dispatch from the drain.
- **Impact on plan / report**: One edit to review-v2.js plus its `SCRIPT_VERSION`. A reproduce section in `review-verifier.md` saying "reproduces" is decided against the current working tree and the verifier stays read-only. A matching Agent-wave fallback line in `agent-waves.md`.

- **Library/API or file**: claude/skills/implement-v2-engine/references/drain.md:214-216, :160-188, :269-286
- **Source**: file:line, read 2026-11-10
- **Evidence-grade**: medium (design reading; the step does not exist yet)
- **Finding**: The step belongs after "every row `done`" and before step 7. A finding that still reproduces mints a fix task in the same milestone. The plan must decide whether that spends a gate round and whether the check reruns after a gate fix round.
- **Details**: The fix-round counter reads the `reason=gate <id> round ` prefix, so a reproduce-minted task needs its own prefix (e.g. `reproduce <id> round <n>: `) to stay countable. A new run per round, recorded on the run, keeps each round's verdicts distinct.
- **Counter**: Invalid if the user wants a still-reproducing finding to park the milestone instead of minting a fix. The brief says only that it "blocks the milestone" (brief.md:36).
- **Impact on plan / report**: A decision for the planner: does a reproduce failure count against the two-round limit?

## Outside this lens (one line each)

- The cell's task-to-findings list comes from the finding-to-task link, which is the `finding-to-task` lens.
- The verifier projection hides `fix` and shows a cleanup's `edit` only when the row is verifier-routed (fields.rs:448, :451). A reproduce check of a checker-routed cleanup may need its own role projection.

Coverage: Read in full: tomlctl/src/review/verdict.rs, claude/workflows/review-v2.js, claude/agents/review-verifier.md, docs/flows/v2-apply/brief.md. Read in part: store.rs 1-320 and 364-402, fields.rs 1-270, 370-694 and 820-845, cells.rs 1286-1470, runs.rs 1-80, drain.md 100-234, v2-pipeline.md 70-219, and the readers listed above. Checks: grep for `.verdicts` readers, for `rechecks` and for `workflow(`/`agentType` in implement-v2-milestone.js. Ruled out: the plan `rechecks` run as precedent for re-verdicts, because it mints new findings and never re-verdicts old ones (plan-flow.md:292-315); `by = "orchestrator"`/`"sync"` as a ready-made discriminator, because those rows still feed `settle` and the latest-verdict readers.
