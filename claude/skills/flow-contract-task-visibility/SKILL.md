---
name: flow-contract-task-visibility
description: Canonical task-visibility contract for every multi-step flow carrier (/implement, /review, /optimise, /review-apply, /optimise-apply, /plan-new, /review-plan, /plan-update, /tdd, /test-bootstrap, /backlog, plus the lumina run-sprint and plan-story orchestrators) — the run-scoped `TaskCreate` / `TaskUpdate` surface that renders an orchestrator's live progress to external readers, the `<slug> /<command> · <ref> — <title>` subject-prefix convention that keeps one session's shared task directory legible across successive commands, the create-up-front / transition-on-dispatch / transition-on-return lifecycle, `blockedBy` DAG mirroring and the apply-flow exception, the granularity rules that stop task entries shadowing a ledger, the self-clearing behaviour of the on-disk list, and the mandatory silent-degradation rule for when the tools are absent. Task entries are a VIEW; the ledger or execution-record remains the source of truth. Consult before minting or transitioning task entries in any flow command.
---

## Task-visibility contract

Flow carriers already record durable state in a ledger (`review-ledger.toml`, `optimise-findings.toml`) or an execution record (`execution-record.toml`). Those files are the source of truth and this contract does not touch them. What they cannot do is show a human — or an external reader — what the orchestrator is doing *right now*, mid-run, while agents are in flight. That is what the task surface is for.

Task entries are written with `TaskCreate` and moved with `TaskUpdate`. Nothing in this contract requires a particular reader to exist.

### 0. What the surface is, on disk

Claude Code persists tasks one JSON file per task at `~/.claude/tasks/<list-id>/<task-id>.json`, carrying `id`, `subject`, `description`, `activeForm`, `owner`, `status`, `blocks`, `blockedBy`, and an optional `metadata` map. Task ids are small decimal strings, not UUIDs. The directory also holds `.lock` and `.highwatermark` dotfiles, so a reader must take only `*.json` and should skip entries carrying `metadata._internal`. Both list-id and task-id have every character outside `[a-zA-Z0-9_-]` rewritten to `-`.

The list id resolves in order: `CLAUDE_CODE_TASK_LIST_ID` when set, then the team name when the run has a team, then a leader's team name propagated into a teammate process, then the session id. So an orchestrator and every teammate it spawns share one list, and an external reader can pin a known list with the environment variable.

**The list self-clears when every task completes.** Roughly five seconds after the last task in a non-empty list reaches `completed`, Claude Code advances `.highwatermark` to the highest task id and unlinks every `*.json`; `TaskList` then returns `No tasks found`. This is a UI sweep, not a session-end or agent-exit hook — nothing removes the directory when a session ends. Two consequences bind anything reading the directory: *empty directory plus a non-zero `.highwatermark`* means "a run finished and was swept", not "no run happened"; and surviving task ids always start at `.highwatermark + 1`.

**Only the orchestrator mints.** No agent under `claude/agents/` is granted `TaskCreate`/`TaskUpdate`, so delegates cannot write to the surface by construction. Were one granted the tools while running outside a team, its entries would land in its own session-id list, orphaned from the orchestrator's.

### 1. Task entries are a view, never a store

Never mint a task per ledger finding, per review item, or per research note — that shadows the ledger, which owns per-item state across runs, and leaves two records disagreeing the moment one is updated and the other is not. Mint tasks for the units the *orchestrator* schedules: one per plan task, one per parallel lens, one per apply cluster, one per phase that takes long enough for a reader to wonder whether it is stuck.

Task entries are ephemeral to the run. They are never read back to decide control flow, never consulted for idempotency (the execution record's skip-list owns that), never handed forward to a downstream command, and **never mirrored into a persisted field**. A value written to `context.toml`, a ledger, or an execution record must be derivable without the task surface, because that surface may be absent entirely (§5).

### 2. Subject prefix is mandatory

A session's task directory accumulates across successive commands — a `/review` run's entries are still on disk when `/implement` starts, and both share one list. The prefix is what keeps that legible:

```
<slug> /<command> · <ref> — <title>
```

`<slug>` is the resolved flow slug, `<command>` is the carrier that minted the entry, `<ref>` is the carrier's own identifier for the unit — a plan task number (`t12`), a lens name (`security`), a cluster id (`c3`), a phase name (`verify`). **`<ref>` is lowercase and hyphenated**, always, so the same lens does not split into two groups depending on which carrier ran. `<title>` is prose, not a path or a glob.

```
auth-refresh /implement · t12 — Extract the token reader
auth-refresh /review · security — Security lens over src/auth/**
```

Set `activeForm` to the present-participle form of the same work (`Extracting the token reader`) — readers render it while a task is `in_progress`, and a missing one degrades the row to the bare subject.

**When no flow slug exists.** Three carriers mint before or without a slug. `/test-bootstrap` and `/backlog` are not flow-aware at all, so their slug slot is the literal `no-flow`. `/plan-new` creates its flow in Phase 9, long after it mints — and this contract has no subject-rewrite mechanism, so a retroactive rename is not available. It therefore substitutes a stable per-run discriminator for `<slug>`: the kebab-cased exploration target from Phase 1. Never leave every entry of a run under a bare `no-flow` when two runs of that carrier could occur in one session — that is the collision the prefix exists to prevent.

### 3. Create up front, transition at the boundaries

Mint the full set for a phase **before dispatching any of it**, so a reader sees the whole shape of the run rather than watching it appear one task at a time. Then:

- `pending → in_progress` at the moment of dispatch, in the same response that dispatches.
- `in_progress → completed` when the unit's return has been processed — after any vet pass, not on bare agent return.
- A failed unit is `completed` with the failure named in its `description`, not left `in_progress`. A reader cannot distinguish an abandoned task from a running one, and a permanently-spinning row trains people to ignore the surface.
- A unit that a conditional phase skips is `completed` with the reason named, never left `pending`.

Where the carrier already has a dependency DAG — `/implement`'s plan edges — mirror it with `blockedBy` at create time. Where it does not, leave `blockedBy` empty rather than inventing edges.

**Apply-flow exception.** `/review-apply` and `/optimise-apply` mint per sequential batch, completing batch *k*'s tasks before minting batch *k+1*'s, so that progress reads cleanly without inter-batch leakage. Cross-batch `blockedBy` edges are therefore not expressible and must not be attempted; intra-batch clusters are file-disjoint by construction and genuinely have no edges. This exception is deliberate — it trades the inter-batch edges for a surface that never shows a wall of `pending` rows.

**Resume rule.** A carrier that can re-enter a unit it already minted — `/tdd resume` re-entering a halted cycle is the live case — mints nothing new on re-entry. Because §1 forbids reading entries back, the carrier cannot detect its own prior set, so the rule is unconditional: on a resume path, complete any stranded rows with the halt reason named and do not re-mint the set.

### 4. Granularity floor

Do not mint a task for work that completes within a single response. A task created and completed in the same turn is noise in every reader; the console line the carrier already emits covers it. The floor is a unit that **dispatches an agent, runs a build, or waits on the user** — an op that does any of those is above the floor regardless of how few turns it occupies, and one that does none of them is below it.

### 5. Degrade silently, always

The task tools are gated on the running model. Claude Code carries a per-family version threshold — currently opus ≥ 4.8, sonnet/fable/mythos ≥ 5 — and a model at or above it gets the tools only when `CLAUDE_CODE_ENABLE_TODO_TOOLS` is set or a remote flag is on. Below the threshold they are unconditional. That gate has caught this project's model before: entries stopped on 2026-08-15 and did not resume for four days, with no error and no announcement, because the tools are simply filtered out of the tool surface rather than failing when called.

**Never gate flow progress on a task call.** If `TaskCreate` or `TaskUpdate` is unavailable or errors, continue the run unchanged; do not retry, do not warn per call, and do not fall back to writing progress into the ledger. Emit one console line at the point of first failure —

```
task-visibility: unavailable this session; progress surface will be empty
```

— and proceed. Every durable output of the command is unaffected by this contract: a run with no task entries and a run with a full set must produce byte-identical ledgers, records, commits, and reports.

A related nudge exists and is not this contract's concern: Claude Code injects a `task_reminder` attachment after 10 assistant turns without a task write, at most once per 10 turns. It re-injects the current list; it does not oblige a carrier to mint anything.

### 6. Do not name the reader

Carriers state what they mint and when. They never reference a particular panel, pane, or plugin — the task directory is a public surface and more than one reader may consume it. A carrier that hard-codes a consumer's expectations makes the contract that consumer's, not the flow's. This rule binds the carriers and this document alike.
