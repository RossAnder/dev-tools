---
name: flow-contract-reconciler
description: The reconciler contract every execution-record writer honours — build a task_ref skip-set before appending so `/plan-update status` never double-writes the `task-completion` entries `/implement` just recorded, gate `status-transition` appends on an actual status change, never silently back-fill completions (only the `migrate` op may), always re-render PROGRESS-LOG.md after appends, and supersede-rather-than-duplicate on the three dedupe keys — reconcile `(date, agent)`, deviation `(task_ref, original_intent, rationale)`, deferral `(task_ref, reason)`. Consult before any operation that appends `task-completion`, `status-transition`, `reconcile`, `deviation`, or `deferral` entries to a flow's `execution-record.toml`.
---

## Reconciler contract

`/implement` Phase 4.5 auto-invokes `Skill("plan-update", "status")`, so the `status` op runs immediately after `/implement` wrote its own `task-completion` entries. Without this contract, `status` would double-write every completion just recorded. The acceptance criterion is: **N `task-completion` entries before the op runs == N entries after** (not 2N).

The contract binds **every writer** that can emit these entry types — `/plan-update`'s `status`, `reconcile`, `deviation`, `defer`, `reformat`, `catchup`, and `migrate` ops, plus `/implement` when it routes deviations or deferrals through plan-update patterns. Enforcement lives in each writer's body; this is the contract they follow.

1. **Build a skip-set first.** Before any append, query:

   ```bash
   tomlctl items list <record> --where type=task-completion --pluck task_ref --lines --verify-integrity
   ```

   Treat each line of stdout as one `task_ref` in the skip-set. `--lines` emits one JSON value per line (tomlctl 0.2.0+), so no `jq -r '.[]'` unwrap is needed — membership checks read the output directly.
2. **Skip duplicates.** Never append a `task-completion` for a `task_ref` already in the skip-set. The op never duplicates entries a prior writer already recorded.
3. **Status-transition writes are change-gated.** Append a `type=status-transition` entry (with `from_status` and `to_status`) ONLY when the flow's `status` field actually changes value (e.g. `in-progress` → `review`). If `status` is unchanged, skip the append entirely — never emit one per invocation.
4. **Never silently back-fill.** No op except `migrate` may append `type=task-completion` entries — those are written exclusively by `/implement` Phase 2. When reconciliation surfaces an unrecorded completion (files modified, tests pass, but no matching entry exists), **flag the gap in the reconciliation report** rather than appending it.
5. **Render after any appends.** After the (possibly zero) appends complete, regenerate `PROGRESS-LOG.md` with `tomlctl flow render-progress-log --slug <slug>`. Always run it, even when nothing was appended, so the rendered file stays a pure function of the log.
6. **Reconcile entries dedupe by `(date, agent)`.** Before appending a `type=reconcile` entry, query for an existing entry of that type with the same `date` and `agent`. If found, **supersede** it — set `supersedes_entry = "<old id>"` on the new entry; do not leave both live. Reconcile is idempotent: the same reconcile fired twice on one day from one agent must not double-count.
7. **Deviation entries dedupe by `(task_ref, original_intent, rationale)`.** Same supersede-don't-duplicate treatment. Re-recording an already-captured deviation pollutes the rendered Deviations table and breaks the latest-per-chain render guarantee.
8. **Deferral entries dedupe by `(task_ref, reason)`.** Same supersede-don't-duplicate treatment. Deferring the same task for the same reason twice is a no-op; recording it twice just inflates the rendered Deferrals table.

Supersession is always the forward pointer `supersedes_entry = "<prior entry's id>"` — never re-use or renumber an id. The render surfaces only the latest entry per supersession chain; superseded entries stay in the log for audit.
