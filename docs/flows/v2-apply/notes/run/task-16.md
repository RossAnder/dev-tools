Status: done — the reproduce check is in the engine between the drain and the gate, with its round cap and re-entry handling

## What changed

- `claude/skills/implement-v2-engine/references/findings.md`: new section "Reproduce check", placed before "Reconcile". It covers when the check runs and when it is skipped, building the units from `[[finding_links]]` `fixes`, which findings a later pass checks again, the round (one more than the highest stored `round` for the flow and milestone, read with `items list <store> --array reproductions`), the args file and the `review-v2-pipeline` reproduce-mode launch with `plan run start --step reproduce:<id>`, the index, relaunching a unit that is not `ok` once and alone, reading the store with `review reproduce-status --expect gone` and then `--expect reproduces`, `plan run finish --step reproduce:<id>` once the store is read, the verdict table with the orchestrator's own `by = "orchestrator"` row, the fix task's shape and its `reproduce <id> round <n>: ` reason prefix, and the two-round cap counted from those reasons.
- `claude/skills/implement-v2-engine/SKILL.md`: rule 3 now names the reproduce launch as the second Workflow launch, under the same no-fallback and no-resume rules. The step table has a new "Reproduce check" row (`reproduce:<id>`). The loop paragraph and the references list mention the check. The out-of-scope vetting line names the check as the one vet pass. The open-run rule treats an open `reproduce:<id>` run like `implement:<id>`, abandons it with `--step reproduce:<id>` and relaunches its round. Rule 4 and the Agent-fallback out-of-scope line are unchanged.
- `claude/skills/implement-v2-engine/references/drain.md`: in step 6, "Relaunch or stop", the hand-over to step 7 now goes through the reproduce check whenever a row fixes a finding.

## Deviations and gap-filling choices

The design leaves several points open. I filled each one the way the surrounding code does it:

- **A launch result carrying `error`.** The design says the run is "abandoned when the launch returns `error`". Rule 3 and run-milestone step 4 record a run only after a launch with no `error`, so in that case there is nothing to abandon. I followed the worker launch: such a launch records nothing and stops the engine. A run whose notification has no index (stopped, or failed before returning) is closed with `plan run abandon --step reproduce:<id>` and also stops the engine. The alternative was recording `plan run start` before the launch, which would break rule 3's ordering.
- **A finding fixed by more than one row.** A reproduce fix task also carries `fixes` for its finding, so the finding goes only in the unit of the highest-id row. Otherwise the args would check it twice in one launch. The alternative was to let it appear in both units, which gives duplicate rows under different `task` refs.
- **More than one linked store.** The args hold a single `store`, so the check runs once per store, one launch after another. When a second store is checked in the same round, its `run_dir` gains a `-<s>`/`-<scope>` suffix. In practice a milestone usually has one store.
- **Executor of a reproduce fix task.** The design does not say. I mirrored the gate's fix rounds: `lite` when the citation names the fix, `deep` otherwise.
- **Relaunching an abandoned reproduce run at re-entry.** The other session's args file is gone, so the round is recomputed from the store. The cap counts fix-task reasons, so the relaunch spends no round.
- **Refusal index.** An index with empty `units` and errors ending `no agent launched` is closed as `errors` and is never relaunched with the same args. This mirrors the worker launch's refusal handling.

## Checks

- Accept: `grep -q 'review reproduce-status' ... && grep -q 'reproduce <id> round' .../findings.md` passed.
- Accept: `grep -q 'reproduce:<id>' .../SKILL.md` passed.
- Compile check: none applies (markdown only). I ran the new read commands against the live store and the binary accepted them, both exiting 0: `items list <store> --array reproductions --where flow= --where milestone= --pluck round --raw --lines` and `items list tasks.toml --array changes --where-prefix "reason=reproduce B round "`. I checked `review reproduce`, `review reproduce-status`, `plan run start`/`finish`/`abandon` against `--help`. I checked the `tasks add-many` keys `review_store` and `fixes` against `claude/skills/tomlctl/references/tasks-write.md`.
- I did not run the pre-commit hook's `cli::dispatch::tests` subset, which covers `claude/**/*.md`, because it builds. That is the gate's job.

TANGENTIAL: question | claude/workflows/review-v2.js | Reproduce mode's unit relaunch reuses the run_dir, so a retried unit overwrites its own payload file; harmless today but worth confirming the Workflow writes nothing else per run under run_dir | a second store sharing a round would collide without the suffix this task documents
