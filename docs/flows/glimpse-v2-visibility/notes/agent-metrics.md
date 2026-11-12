Verdict: Partly. Time by milestone and by executor can be computed from the stores as they are: join `agents.toml` segments (tomlctl/src/agents/schema.rs:72-79) to task rows' `checkpoint` and the specs' `executor`. Probe health can be computed from `tasks.toml` alone (tomlctl/src/tasks/schema.rs:290,350,374). Token totals cannot: the store keeps one `context_tokens` per agent, the context size at its last turn (tomlctl/src/agents/correlate.rs:447-474), not tokens spent.

## Answer

**What each agent row records** (schema.rs:47-67):
- identity: `id`, `harness`, `session_id`, `agent_id`, `agent_type`, `kind`, `name`, `team`
- `status`
- times: `started_at`, `updated_at`, `ended_at`, taken from tomlctl's own clock when each hook event is recorded
- `transcript_path`
- `summary`, capped at 600 characters
- `context_tokens`
- `segments[]`: one per assignment, each with `task_ids`, `item_ids`, `started_at` and `ended_at`

There is no executor field and no milestone field. Neither is needed for time totals:
- A v2 worker's `agent_type` is `plan-worker-lite` or `plan-worker-deep`.
- Joining a segment's `task_ids` to `tasks.toml` gives `items[].checkpoint`, which is the milestone, and `specs[ref].executor`.
- Executor attribution does not even need `agents.toml`: a worker writes `agent` on its own row (sampled on v2-apply: `agent = "plan-worker-deep"`).

What limits the totals:
1. **Tokens are not spend.** `context_tokens` is the input, cache-read, cache-creation and output count of the last assistant turn only, written when the agent stops. Summing it across agents gives a sum of final context sizes, not tokens used. Real spend means reading every `usage` line in each `transcript_path`. Those transcripts live under the Claude config dir, outside the repo.
2. **Coverage.** Two kinds of agent never get task ids:
   - The milestone gate's `verification` agent.
   - Investigators. Only cold readers, through their `tasks show` prompt, are attributed.

   A dispatch gets task ids only from three prompt patterns: `tasks show <ids> --slug`, `plan view --slug <s> --for worker --task <id>`, and a `ledger:` line (correlate.rs:255-270). So gate time cannot be put under a milestone by join. A time-window match against the milestone's run would be a heuristic.
3. **History.** Of the five v2 flows, only `human-only-handovers` has an `agents.toml`:
   - It holds two cold-reader rows. Both are stop-only rows with `started_at == ended_at`, recorded before the 2026-11-11 `began_at` fix (56026d8d).
   - `v2-apply`, `milestone-cadence` and `v2-harness-tuning` have none.

   Worker attribution landed on 2026-11-10 (aa86c18d) and Workflow-agent attribution on 2026-11-11, so no completed v2 run has usable time data. The brief rules out backfill, so a view built now shows "no data" for every existing flow.
4. **Durability.** `.gitignore:8` ignores `.claude/flows/*/agents.toml*`. Metrics exist only on the machine that ran the flow, and a fresh clone has none.

**Probe health** can be computed from `tasks.toml` alone:
- accept checks: `specs[].accept[].probe` has `at` (a head sha), `neg_rc`, `pos_rc` and `verdict`, which is one of healthy, vacuous, unsatisfiable, permanently-green, broken or guard-alone.
- premises: `specs[].premises[].probe` has a verdict of holds, fails or broken.
- gate probes: `milestones[].gate_probes[]` has `verdict` (`""` until recorded) and `at`.

`plan check` already reports unhealthy and absent accept probes (`plan/probe-unhealthy`, `plan/probe-absent`, check.rs:549-567). It says nothing about premise or gate-probe verdicts. What counts as unprobed must exclude rows that are never probed: accepts that write the build or are not the worker's (`probes::skip_reason`, probes.rs:174), and `after:` premises whose target task is not done (probes.rs:193). Both helpers are `pub(super)`, so glimpse could not call them through the library as it stands.

## Findings

- **Library/API or file**: tomlctl/src/agents/correlate.rs:447-474 `tokens_in`; tomlctl/src/agents/record.rs:218-220
- **Source**: file:line, read 2026-11-12; the test `context_tokens_sums_the_last_assistant_usage` (correlate.rs:883-897)
- **Evidence-grade**: high
- **Finding**: `context_tokens` is the context size at the last turn, not cumulative spend, so "tokens totalled by milestone and executor" cannot be computed from `agents.toml` as it is.
- **Details**: `tokens_in` walks the transcript tail backwards and returns the sum of the first assistant `usage` it finds. `apply` writes that value only on Stop, and only when it is greater than 0. The Codex path uses `last_token_usage.total_tokens` (correlate.rs:476-507). The cumulative figure would need either a new stop-time read summing every usage line, or a reader over `transcript_path`, which is local to the machine.
- **Counter**: If the brief's success criterion means peak context per agent rather than spend, the existing field is enough and only labelling is needed.
- **Impact on plan / report**: The plan needs a decision: add a cumulative token count to the record (a new field, written by the stop path in record.rs and correlate.rs, plus glimpse's `model::Agent`), or label the view as final context size. This is `needs-plan` if a field is added, because the schema, recorder, snapshot and glimpse model must change together.

- **Library/API or file**: tomlctl/src/agents/schema.rs:47-79 `AgentRecord`, `Segment`; tomlctl/src/tasks/schema.rs:272,413 (`TaskSpec.executor`, `TaskRow.checkpoint`)
- **Source**: file:line; `tomlctl tasks list --slug v2-apply --select id,checkpoint,status,agent,commit`, which returned `{"id":1,"checkpoint":"A",...,"agent":"plan-worker-deep",...}`
- **Evidence-grade**: high
- **Finding**: Time by milestone and by executor can be computed by joining segment `task_ids` to task rows. The store needs no executor or milestone field.
- **Details**: Duration is each segment's `ended_at - started_at`, as RFC 3339 instants. Some cases need care:
  - A segment can hold several task ids. Cold readers show `[8,9,10,11,12]`, and `tasks show 1,2` dispatches do the same, so the time has to be split or counted per task.
  - Cold-reader segments name tasks they read, not tasks they worked, so filter by `agent_type` before attributing.
  - A retried task has several rows.

  Executor comes from `agent_type`, from `specs[].executor`, or from the row's `agent`.
- **Counter**: A worker dispatch prompt that does not carry `plan view --slug <s> --for worker --task <id>` would leave segments empty. Check that the current milestone Workflow prompt still emits that string.
- **Impact on plan / report**: The aggregation can be a pure library function over the snapshot's agents plus the task store. No new recorder work is needed for time.

- **Library/API or file**: tomlctl/src/agents/correlate.rs:251-329 `dispatch_in`
- **Source**: file:line
- **Evidence-grade**: high
- **Finding**: Gate (`verification`) agents and investigators get no task ids, so gate and planning time cannot be attributed to a milestone by join.
- **Details**: Only `tasks show`, `plan view --for worker --task` and `ledger:` prompts produce a dispatch. Any other agent is placed in the flow by session affinity alone (record.rs:390-392), with empty segments.
- **Counter**: If the gate dispatch prompt contains a `tasks show <ids> --slug` line, gate rows do carry ids. I did not read the implement-v2 gate prompt.
- **Impact on plan / report**: Either add a milestone marker to the gate and drain prompts and a matching pattern in `dispatch_in`, or count only worker time in milestone totals and say so.

- **Library/API or file**: .gitignore:8 `.claude/flows/*/agents.toml*`
- **Source**: `git check-ignore -v .claude/flows/human-only-handovers/agents.toml`, which printed `.gitignore:8:.claude/flows/*/agents.toml*`; `ls` of the five v2 flows
- **Evidence-grade**: high
- **Finding**: Agent metrics are local and ignored by git, and no completed v2 flow has usable data.
- **Details**:
  - Only `human-only-handovers` holds rows: two `plan-cold-reader` rows with zero-length intervals.
  - `v2-apply` (56 tasks), `milestone-cadence` and `v2-harness-tuning` have no `agents.toml`.
  - Attribution for v2 workers and Workflow agents landed on 2026-11-10 and 2026-11-11 (aa86c18d, 56026d8d).
- **Counter**: Another machine or an un-ignored copy could hold these stores. This lens cannot observe that.
- **Impact on plan / report**: A totals view must handle a missing or empty store as a "not recorded" state, not as zero. If the totals should survive, they have to be written somewhere tracked, for example a per-milestone summary stamped at commit time. That is a design choice for the plan.

- **Library/API or file**: tomlctl/src/plan/check.rs:546-567; tomlctl/src/plan/probes.rs:174 `skip_reason`, 193 `premise_skip_reason`; tomlctl/src/tasks/schema.rs:336-386
- **Source**: file:line; `tomlctl tasks show 1 --slug v2-apply --with spec`, which showed healthy accept probes at a sha and a premise with no `probe`
- **Evidence-grade**: high
- **Finding**: A probe-health summary can be computed from `tasks.toml`. Current checks cover only accept probes, and the rules for what is probeable are private to `plan`.
- **Details**:
  - `plan check` reports `plan/probe-unhealthy` (an error) and `plan/probe-absent` (a warning) for accepts only.
  - Premise verdicts (`holds`/`fails`/`broken`) and `gate_probes[].verdict` (`""` until recorded) go unreported.
  - `Probe.at` is a head sha, so a probe recorded at an older commit can be shown as stale.
  - `skip_reason` and `premise_skip_reason` are `pub(super)`. A library summary should expose one function rather than glimpse copying the rules.
- **Counter**: If another verb, such as `plan stage` or `plan view`, already prints premise and gate-probe health, the summary already exists. I checked only `check.rs` and `stage.rs`'s class map.
- **Impact on plan / report**: Plan one library function covering all three probe kinds, each as verdict, unprobed, not probeable or stale, for glimpse to call through the snapshot. The snapshot-path lens owns the question of how specs reach glimpse.

Coverage: read in full `tomlctl/src/agents/schema.rs`, `record.rs` and `mod.rs`. Read in part `correlate.rs` (lines 150-329 and 430-510), `audit.rs`, `dispatch.rs`, `plan/probes.rs` (1-200), `plan/check.rs` (500-589 and the premise and gate-probe greps), `tasks/schema.rs` (40-480), and `glimpse/src/model.rs` (255-300). Checks run: `tomlctl agents list` (human-only-handovers), `tomlctl tasks list` and `tasks show --with spec` (v2-apply), `git check-ignore`, `git log` on the agents recorder, `ls` of the v2 flow directories. Ruled out:
- an executor field in `agents.toml`, which is not needed given `agent_type`, `spec.executor` and the row's `agent`
- `agents audit` as a metrics source, since it only greps Bash commands
- `PROGRESS-LOG` and `execution-record` as token sources, which were not read and are outside this question
