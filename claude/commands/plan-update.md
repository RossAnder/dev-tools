---
description: Update plan documents — track progress, deviations, deferrals, and reconcile against codebase
argument-hint: [plan path] [operation: status|complete (gated)|deviation|defer|reconcile|reformat|catchup|snapshot|migrate]
---

# /plan-update — plan documents as living records

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Maintains implementation plans as living records: tracks progress against the codebase, documents deviations with rationale, registers deferrals with concrete re-evaluation triggers, and reconciles plan expectations against actual code state. Runs in targeted mode (`/plan-update docs/plans/prod_preparation/ status`) or auto-detect mode (`/plan-update` after implementation work). The nine operations are defined under `## Step 2`; with no operation specified the default is **reconcile**, the most comprehensive.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and reconciliation depth.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, project-local `.claude/` path resolution, the `draft` / `in-progress` / `review` / `complete` status vocabulary and its no-auto-complete rule, slug derivation, canonical artifact paths, completed-flow handling, the legacy `.claude/active-flow` ignore, and the mandatory bootstrap-summary console line).

Build the input envelope:

```bash
tomlctl flow envelope build \
  --command plan-update \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --require-artifact execution_record \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`. The `--require-artifact execution_record` flag pins `require_artifacts = ["execution_record"]` (`/plan-update` reads the record before writing it); `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD, omit `--branch` so the envelope records `branch:null`. Add `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch `flow-bootstrap` via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*` (esp. `execution_record`), `doctor.ok`, and `resolved.stale` for downstream phases. Emit the bootstrap-summary line before any other action.

## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Gate: fire ONLY when `envelope.plans_directory == null` (the bootstrap agent normalises both the unset case AND the literal `"__DONT_ASK__"` sentinel to `null`); when non-null, skip entirely and use the bound value. Invoke the `flow-contract-plansdirectory-prompt` skill to load the first-use prompt contract (option-list construction, recommended-first single-select AUQ ordering, headless empty-answer in-memory binding, `Don't ask again` sentinel arbitration, the free-text follow-up, the `tomlctl json set` persist idiom, and the downstream binding). The wording is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan` — the skill is the single source.

## Execution record

Invoke the `flow-contract-execution-record-schema` skill to load the canonical contract for the per-flow append-only log at `.claude/flows/<slug>/execution-record.toml` (field set; the `task-completion` / `verification` / `deviation` / `deferral` / `reconcile` / `status-transition` / `checkpoint` type vocabulary with each type's required fields; monotonic `E{n}` id minting; the `flow record` write contract; append-only + supersession; the `[tasks].completed` derivation; read-path `--verify-integrity` integrity contract; field-length caps; and read rules). Every append below goes through that contract: one `tomlctl flow record --slug <slug> --type <type>` call per entry, which resolves `<record>` from the slug, mints the `E{n}` id under the write lock, defaults `date` to today, derives `task_ref` from `--task <id>`, validates the type's required fields and enums, caps over-long text, and restamps `<record>`'s `last_updated` — so the payload carries no `id` and no `date`, and no second write follows. Free text goes in a file written with the Write tool and passed by `--set-file`. Reads of `<record>` resolve it as `[artifacts].execution_record` and use the fully-qualified path — never the bare filename `execution-record.toml`. No manual bootstrap is needed: `flow init` / `/plan-new` pre-seed the record, and `flow record` auto-creates a missing one under an existing flow with the `schema_version = 1` skeleton and its `.sha256` sidecar in the same write. (If a record exists but its sidecar is missing, repair with `tomlctl integrity refresh <path>` — that is sidecar repair, not bootstrap.)

Invoke the `flow-contract-reconciler` skill to load the reconciler contract (build a `task_ref` skip-set before appending, skip duplicates, gate `status-transition` appends on an actual status change, never silently back-fill completions, always re-render after appends, and supersede-rather-than-duplicate on the reconcile / deviation / deferral dedupe keys). It binds every op below that appends to `<record>`, not just `status`.

`PROGRESS-LOG.md` is a DERIVED artifact — never hand-authored, never carrying a `.sha256` sidecar. Regenerate it deterministically as the last step of every mutating op:

```bash
tomlctl flow render-progress-log --slug <slug>
```

This rebuilds the file as a pure function of `<record>` plus the flow title (the `# Plan:` header reached via `context.toml` → `plan_path`): the `<!-- Generated from execution-record.toml. Do not edit by hand. -->` marker line and the four tables (Completed Items / Deviations / Deferrals / Session Log). Pass `--stdout` to print without writing, `--verify-integrity` to check the record's sidecar first.

## Task store

Invoke the `flow-contract-task-store` skill to load the canonical contract for the per-flow task DAG at `.claude/flows/<slug>/tasks.toml` (the schema and every row field; the `ref` slug rule that makes a task heading the store's primary key and the import's upsert key; the status vocabulary; the stored `needs` / `coupling` edges and the graph products computed on read; the semantics of each `tasks` verb; the `check` finding classes and exit policy; the render contract and its three owned sections; the ref-set diff gate and the renamed-heading trap it exists to catch; the orchestrator-only status-write rule; and `tomlctl integrity refresh` as the recovery after a failed write).

From `/plan-new` Phase 9 onward the store is canonical and the plan's `## Execution Policy`, `## Tasks` and `## Dependency Graph` sections are rendered from it — the same derive-don't-author pattern `PROGRESS-LOG.md` already follows. Its path is `[artifacts].tasks`, and every `tasks` call below reaches it with `--slug <slug>` rather than a path. Import before anything reads a row status — on a first run because the store is absent or holds no rows, and on **every later run** because `<record>` can hold completions the store has not seen. Re-running is idempotent: the upsert is keyed on `ref` and `--reconcile-record` only ever promotes a row to `done`, never demoting one.

```bash
tomlctl tasks import-plan --slug <slug> --reconcile-record
```

`[tasks].total` is the store's row count on every `context.toml` write (Step 1). Beyond that, three ops touch it: `reformat` and `catchup` run the post-rewrite store sequence in Step 2, and `migrate` reads the plan's derived refs from a plan-mode dry run. `reconcile`, `complete`, `deviation`, `defer` and `snapshot` require no further store call beyond the row-count refresh. **No op here writes a row's `status`** — `/implement` owns execution state, and the completions this command reads come from `<record>`. The ensure-the-store reconcile above is the one exception, and it decides nothing: it promotes exactly the rows `<record>` already holds as `done`.

**The join between the two `[tasks]` counters is gated, never assumed.** `total` is the store's row count and `completed` is derived from `<record>`, joined on the `ref` / `task_ref` string alone — so the reconcile's `unmatched_refs` is the only thing that says whether the numerator belongs to that denominator. Read it from the import above on every write that touches `[tasks]`. Empty ⇒ the join is complete and `completed` is written as derived. Non-empty ⇒ `<record>` holds completions no plan task owns, so **write `completed` as the derived count less the entries `unmatched_refs` names**, which is what stops the pair reporting `completed > total`, and surface them in the Step-3 summary as `N record completion(s) match no plan task: <refs>` with the two recoveries: a rephrased heading is repaired through `reformat` / `catchup` and their ref-set gate, and work that belonged to no task is recorded via the `deviation` op. Skipping the gate is not silent — the execution-record contract's backstop has `flow doctor` warn on the shapes an ungated write leaves behind. A `/tdd` sub-flow has no store and no join to gate; its counters stay the plan-item `total` and the raw record-derived `completed`.

**`/tdd` sub-flows are exempt.** A slug matching `<parent>-tdd-<NNN>` never creates, imports or reads a store, and its `[tasks].total` stays the plan-document item count. The parent flow owns the plan and therefore the store.

## Step 1: Locate the Plan

**Reason thoroughly through plan location and operation analysis** before dispatching agents. `slug` and `context_path` are bound from Step 0 — do not re-resolve. The bootstrap envelope does NOT pass `plan_path` through, so read it from the context file (`tomlctl get <context_path> plan_path`): single-file plans point at the plan, multi-file plans at the outline. If required fields are missing or the file is malformed, prompt the user rather than synthesising defaults.

Resolve the plan in this order: an explicit `$ARGUMENTS` path (if a directory, classify its markdown by role — outline/master, numbered detail documents, `PROGRESS-LOG.md`, deferrals); else the resolved flow's `plan_path` when the referenced file or directory exists; else recently-modified files under `docs/plans/` (or the project's established plans directory) — one recent candidate is used, several are listed for the user; else ask. Offer to create a progress log if the plan has none.

Once located, update the resolved flow's `context.toml` per the write procedure in Step 3, item 4. Leave `[tasks].in_progress` untouched — that field is written only by `/implement` during live execution. When every plan item is complete (or all remainders deferred), set `status = "review"`, **never** `"complete"`: `review` means "implementation finished, awaiting explicit user sign-off" and keeps the flow targetable by `/review`, `/optimise`, `/optimise-apply`, and `/review-apply` (their filter is `status != "complete"`). Auto-transition to `complete` is forbidden because it strands a freshly-implemented plan beyond auto-resolution before the user has had a chance to review or optimise it; only the explicit `complete` op may write it.

## Step 2: Determine Operation

Parse the operation from `$ARGUMENTS` after the path. `reformat` and `catchup` rewrite plan files and MUST honour the plan-restructure contract — **invoke the `flow-contract-plan-restructure` skill** to load it (byte-for-byte heading preservation and the mandatory `tasks import-plan --dry-run` ref-set diff gate, the `tasks update --ref` rename recovery, archive-before-rewriting, the multi-file and single-file output structures, the RESEARCH-NOTES.md format, `## User Decisions` survival and `## Execution Policy` survival with checkpoint markers re-rendered rather than re-mapped by hand, the merge-exit consistency re-derivation, inferred deviations/deferrals, PROGRESS-LOG regeneration, and the present-summary-then-write-immediately rule).

**Post-rewrite store sequence (`reformat`, `catchup`).** The heading-equality assertion is a dry-run import read for its ref-set diff, not a string comparison this carrier re-implements. Once the rewritten files are on disk, run these in order:

```bash
tomlctl tasks import-plan --slug <slug> --dry-run
tomlctl tasks import-plan --slug <slug>
tomlctl tasks render --slug <slug>
```

**A non-empty `added_refs` or `removed_refs` stops the sequence at the first command** and goes to the user: a removed ref is a rephrased or deleted heading, and one whose row is not `pending` is a settled task about to be orphaned. Proceed only on explicit confirmation, and only after renaming the row and recording the rename through the `deviation` op:

```bash
tomlctl tasks update <id> --slug <slug> --ref <new-ref>
```

Without the rename the real import raises `dag/duplicate-number` and writes nothing, because the old row still holds the number the renamed heading re-claims — the dry run is what turns that refusal into a decision taken before the rewrite reaches the store. The render is what brings renumbered checkpoint markers back correct.

#### `status` — Update completion markers

Scan plan items against the codebase and git history: for each item, check whether the referenced files exist, the described changes are present, and the relevant tests pass. Apply the reconciler contract before any append — this op is auto-invoked by `/implement` Phase 4.5 immediately after `/implement` wrote its own completions, so the skip-set is what stops a double-write. Then re-render `PROGRESS-LOG.md` and update `context.toml` per Step 1. Writes `status ∈ {in-progress, review}` only — MUST NOT write `complete`.

Report the backlog items promoted to this flow from a read-only reconcile:

```bash
tomlctl backlog reconcile --flow <slug>
```

Print `backlog: N ready, M in-progress, K stalled, L unlinked, O orphaned` from the lengths of those five `buckets` arrays (`dangling` and `external` are always empty under `--flow`). This op resolves nothing: a `ready` item is resolved by `/implement`'s Phase 4 or by `complete`.

#### `complete` — Explicitly mark the flow as complete

User-invoked; the ONLY path that may set `status = "complete"`. Run once the user has finished `/review`-ing and `/optimise`-ing the implemented plan and is ready to drop it from auto-resolution. In order:

1. Read `<old_status>` via `tomlctl get <context_path> status --verify-integrity`.
2. **Refuse to transition from `draft`** — emit `flow <slug>: refusing transition draft → complete. A plan that was never in-progress cannot be marked complete. Run /implement first, or transition via /plan-update <slug> status.` and exit.
3. **No-op if already `complete`** — emit `flow <slug>: already complete — no change.` and exit; no log entry, no render.
4. **Warn-if-incomplete gate.** Invoke the `backlog-capture` skill to load the promotion lifecycle (the live `promoted` claim, the reconcile buckets, and `--reopen`'s rationale rule). Then:

   - **Resolve delivered backlog items.** Run `tomlctl backlog reconcile --flow <slug> --apply`. It resolves every `ready` item — every task that `closes` it is `done` — and reports those ids under `applied`; surface each `skipped` entry with its reason. The remaining claims are the ids across `buckets.{in-progress, stalled, unlinked, orphaned}`: `<b_count>` and `<b_list>`. A repo with no `.claude/backlog.toml` reads as empty, so it needs no existence guard.
   - **Count open findings.** Count open items in the resolved review and optimise ledgers with `tomlctl items list <ledger> --status open --count --raw` (plus `--pluck id --raw` for the ID lists), guarded by a file-existence test. Distinguish file-absent (acceptable — count 0) from tomlctl-failed (must surface): let a non-zero exit from this or the reconcile propagate and halt, and never swallow it with a bare `2>/dev/null`.
   - **Ask.** If the combined count `<N>` is > 0, ask via `AskUserQuestion`: `<N> open item(s) on flow <slug>: <r_count> review (<r_list>), <o_count> optimise (<o_list>), <b_count> backlog (<b_list>). Mark complete anyway?` — ID lists capped at 5 each plus `...`. With `<b_count>` = 0 the options are `Mark complete anyway` (proceed, recording the override) or `Cancel` (exit without writing). With `<b_count>` > 0 they are `Reopen them and complete`, `Complete, keep them promoted`, or `Cancel`; either completing option also overrides any open findings. `Reopen them and complete` returns the backlog claims to triage in one call, before step 5, and a failure halts the op:

     ```bash
     tomlctl backlog triage <ids> --reopen --rationale "flow <slug> completed without resolving it"
     ```

     `<ids>` is every remaining claim, not the capped `<b_list>`. `Complete, keep them promoted` leaves the claims on the flow, where `backlog reconcile` and `/backlog` report them as `orphaned`.
   - **If `AskUserQuestion` is unavailable** (non-interactive harness, no open question slot), refuse: emit `flow <slug>: complete blocked — N open items, AskUserQuestion unavailable for override. Re-run interactively or transition the open items first.` and exit. The `ready` items the reconcile resolved stay resolved.
5. Set `status = "complete"` with `tomlctl set <context_path> status complete`; the write refreshes `updated` to today itself, and `created` and key order are preserved. When `<old_status> == "in-progress"`, surface the informational note `flow <slug>: skipping the review intermediate state (status was in-progress, transitioning directly to complete). Most flows should pass through review (set via /plan-update <slug> status) before completing.` — the user invoked `complete` explicitly, so honour it.
6. Append a `type=status-transition` entry with `from_status` / `to_status` through `flow record`. Its `summary` MUST record whether the warn-gate fired and was overridden (`"User explicitly marked flow complete via /plan-update <slug> complete"`, or the same with `(warn-if-incomplete gate overridden with N open items)` appended), then append `; resolved K backlog item(s)` when the step-4 reconcile applied any, and `; reopened K backlog item(s)` or `; kept K backlog item(s) promoted` for the backlog answer:

   ```bash
   tomlctl flow record --slug <slug> --type status-transition --set agent=plan-update --set from_status=<old_status> --set to_status=complete --set summary='<summary>'
   ```
7. **Reap the flow's transient artefacts.** Completion is the only point at which a flow's scaffolding is provably dead, and it is the only step that owns their deletion — every other retention rule in the harness is next-run-triggered and therefore never fires on a flow's final run. Two sweeps, both after the step-5/6 writes have landed:

   - **Review siblings.** Delete `<plan>.premerge.md`, `<plan>.revised.md`, and `<plan>.revised.prev.md` for every plan document in scope, including inside `docs/plans/archive/**` for this slug. Each is a near-duplicate of a document git already holds. Report `reaped N review sibling(s)`.
   - **Terminal ledger rows.** For each resolved review and optimise ledger, list terminal rows with `tomlctl items list <ledger> --status fixed,wontfix,verified-clean,merged --pluck id --raw`, then remove them in one batched `tomlctl items apply <ledger> --ops -` call with an `{"op":"remove","id":…}` per id. Report `reaped N terminal item(s) from <ledger>` per file.

   **`deferred` is NEVER reaped** — it carries a user-committed re-evaluation condition and is live state, not residue. **Do not reap outside `complete`**: the apply carriers treat a terminal row as the signal that turns a re-applied id into a warn-and-skip, so reaping while a flow is still open destroys apply-flow idempotency and lets a landed fix be applied a second time. Skip both sweeps entirely under `--no-reap`, or when the run is non-interactive and the step-4 gate was overridden. The rows' content remains in git history and in `<record>`.

8. Re-render `PROGRESS-LOG.md`, then print `flow <slug>: status <old_status> → complete. Auto-resolution will skip this flow on subsequent /review, /optimise, /implement runs (use --flow <slug> to target explicitly).`

**Gate ordering is load-bearing**: the step-4 reconcile, queries and prompt MUST run after the step-3 no-op check (otherwise an already-complete flow is re-prompted) and before the step-5 write (otherwise the transition lands before the user can cancel). The reconcile's `--apply` and the batch reopen are the only writes allowed ahead of step 5: a `ready` item is delivered whether or not the flow completes, so a later `Cancel` or refusal leaves it correctly resolved; and a reopen that lands first leaves a failed run safe to repeat, since the flow is still open and the reopened items no longer count.

#### `deviation` — Record a deviation

Gather evidence from the conversation and git history — which task was affected, the original intent, what was actually done, and why — and confirm with the user before writing. Append a `type=deviation` entry to `<record>` through `flow record`, with `task_ref`, `original_intent`, `rationale`, and `commits[]` beyond the always-required fields; add `--set supersedes_entry=E<n>` when superseding an earlier deviation (supersession is the forward pointer, never number re-use). `--task <id>` copies the affected store row's `ref` into `task_ref`. Write the planned and actual approaches to two files with the Write tool first:

```bash
tomlctl flow record --slug <slug> --type deviation --task <id> --set agent=plan-update --set summary='<done>' --set-file original_intent=<intent-file> --set-file rationale=<rationale-file> --set-json commits='["<sha>"]' --get id
```

`--get id` prints the minted id bare, for the report or a later `supersedes_entry`. This op MUST NOT mint legacy IDs of any kind. Then re-render `PROGRESS-LOG.md` and update `context.toml` per Step 1.

#### `defer` — Register a deferral

Gather evidence — which task is being deferred, why, and the **re-evaluation trigger**, which must be a concrete observable condition ("when frontend types are next refactored", "when migrating to .NET 11") and never a vague one ("later") — and confirm with the user before writing. Append a `type=deferral` entry with `task_ref`, `reason`, and `reevaluate_when`; `legacy_id = "DF<n>"` is set only by `migrate`, never here. Append it through `flow record`, with the reason and trigger written to files first; mint no legacy IDs:

```bash
tomlctl flow record --slug <slug> --type deferral --task <id> --set agent=plan-update --set summary='<what is deferred>' --set-file reason=<reason-file> --set-file reevaluate_when=<trigger-file>
```

Then re-render and update `context.toml` per Step 1. If every remaining non-complete item is now deferred, set `status = "review"` — never `"complete"`.

#### `reconcile` — Full plan-code reconciliation

The most comprehensive operation. Launch **two** `subagent_type: "general-purpose"` agents in a single response message — do not reduce the count; forward and reverse are distinct perspectives that cannot be combined.

- **Agent 1 (forward, plan → code)**: read every plan item and its expected outcome; for items marked Done, verify the expected artifact exists (files present, code patterns present, tests pass); for items marked Not Done / In Progress, check whether they were implemented without the plan being updated; check `git log` since the progress log's last-updated date for commits touching plan-scoped files. Flags items done but unmarked, items marked done then broken by later changes, and new work tracked by no plan item.
- **Agent 2 (reverse, code → plan)**: run `git diff --name-only {baseline}..HEAD` (baseline = the progress log's last-updated commit or `git merge-base HEAD master`); for each changed file check whether a plan item covers it. Flags untracked changes, stale items (marked In Progress with no recent commits touching the relevant files), and implicit deviations.

**Reason thoroughly through reconciliation synthesis** — cross-reference both agents, resolve conflicting evidence, and determine the accurate status of every plan item before writing. Each agent appends its own `type=reconcile` entry through `flow record` with `direction ∈ {forward, reverse}`, `findings_count`, and `commits_checked[]` — the last two by `--set-json`, so the count stays an integer and the commits an array:

```bash
tomlctl flow record --slug <slug> --type reconcile --set agent=plan-update --set direction=forward --set-json findings_count=<n> --set-json commits_checked='["<sha>"]' --set summary='<findings>'
``` Follow-up deviations and deferrals discovered during reconciliation are recorded as separate `type=deviation` / `type=deferral` entries via the ops above — never inlined into the reconcile entries. The reconciler contract applies in full here.

Produce the reconciliation report **and apply all updates in the same response** — do not pause for confirmation: agent results are in context now and are lost to compaction if you wait, and the user can review and revert via git. The report covers Status Updates (old → new with commit/file evidence), Unrecorded Deviations (with a suggested `type=deviation` entry), Untracked Changes, Stale Items, **Unrecorded Completions as gap flags that MUST NOT be auto-appended** (per reconciler rule 4 — point the user at `migrate` or at having `/implement` re-record the completion), and Suggested Deferrals with trigger suggestions. Then re-render `PROGRESS-LOG.md` and update `context.toml` in the same write batch per Step 1 — additionally, **refine `scope`** if reconciliation reveals edits outside the original scope (add the new globs, preferring `<dir>/**`; never shrink `scope` unless the user asks), and set `status` to `review` when every item reconciled as done or deferred, else `in-progress`. This op MUST NOT set `complete`.

#### `reformat` — Rewrite plan into standardized structure

Read the entire existing plan and rewrite it into the standardized structure per the plan-restructure contract. **This operation ONLY restructures documents** — it performs no reconciliation, status updates, or codebase validation; those belong to `reconcile` and `status` as a separate step afterwards.

Launch **two** `subagent_type: "general-purpose"` agents in a single response message; do not reduce the count. **Agent 1 (content extraction and classification)** reads every plan document in scope and returns the full classified inventory — tasks/items with status, effort, risk, dependencies; completed items with commit references and dates; the `## Summary`, `## Success Criteria` (with its `success:` keys) and `## After Merge` sections; `## Exploration Notes` and research notes and corrections; deviations (whether legacy `D<n>`-numbered or embedded in prose); deferrals with any stated triggers; `## User Decisions` entries (question, chosen answer, prompting finding); the `## Execution Policy` section; verification criteria; dependencies; and context/rationale. **Nothing from the original documents may be missing.** **Agent 2 (codebase state snapshot)** returns a concise informational snapshot: which plan-referenced files exist, which changed recently, the latest commit touching plan-scoped files, and any obviously-completed items the plan does not reflect.

**Reason thoroughly through reformat synthesis** — cross-reference both agents to confirm every piece of original content is accounted for and correctly classified before writing. Then produce the reformatted plan per the restructure contract, run the post-rewrite store sequence at the head of this step, and update `context.toml` per Step 1.

#### `catchup` — Revive a stale plan with fresh research and re-exploration

For plans that have fallen behind the codebase. Combines research, reconciliation, and reformat into one pass — the most expensive operation. Runs three phases sequentially; do not skip a phase or wait for user input between them. Archives before rewriting and honours the plan-restructure contract throughout.

**Phase 1** — launch **three** agents in a single response message, non-overlapping scopes, do not reduce the count. **Agent 1 (codebase re-exploration, `general-purpose`)**: read every file the plan references (do they exist? moved, renamed, deleted?), search for code implementing plan items even in different files or via different approaches, identify structural changes since the plan was written, map the current architecture in the plan's domain, check `git log` for the full history in scope, and return a comprehensive current-state inventory. **Agent 2 (technology and API research, `research-lite`** for the default mechanical case; escalate to `research-deep` when the plan introduces architectural pattern questions or library comparisons, stating `DISPATCH: research-deep — <reason>` at the top of the prompt**)**: research the current state of every technology, library, and framework version the plan references, flag deprecated APIs / removed features / outdated guidance, and return a technology assessment with specific corrections. Any upgrade it recommends is a dependency-upgrade finding, which brings the method's supply-chain checks to bear. **Agent 3 (content extraction and classification, `general-purpose`)**: same contract as `reformat`'s Agent 1.

**Phase 1.5 — vet Agent 2's output (orchestrator).** Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule). Scope: **Agent 2 only** — Agents 1 and 3 are exempt because Phase 2 already cross-references their outputs. Before sampling, verify every "deprecated" / "removed" / "superseded" claim against the pinned installed source (Context7 when none is on disk) and the library's official changelog — these are the highest-impact assertions because they drive plan rewrites. Carry only post-vet findings into Phase 2; propagating a fabricated tech finding into a rewritten plan corrupts the plan and the user's trust in the catchup.

**Phase 2 — synthesise and rewrite.** **Reason thoroughly**: cross-reference codebase state, vetted technology research, and the content inventory to determine accurate status for every plan item, identify stale research notes by the fetch dates on each topic's `Searched:` line (a note with no such line has no date to trust, so treat it as stale), and resolve conflicts between plan expectations and codebase reality. Produce the reformatted plan per the restructure contract, additionally: update task status from Agent 1's findings (done items get commit evidence, partial items get noted, no-longer-relevant items get flagged for deferral); replace stale RESEARCH-NOTES.md content with Agent 2's vetted findings and their `Searched:` lines, keeping still-valid notes and marking outdated ones superseded; update file paths to match the current structure; **flag invalidated tasks for user decision rather than silently dropping them**; and append `type=deviation` / `type=deferral` entries for implementations that happened differently and items no longer actionable. Codebase realignment may suggest *file-path* updates (fine) but never *heading text* changes. Write all files immediately in the same response, then run the post-rewrite store sequence at the head of this step and update `context.toml` per Step 1.

**Phase 3 — catchup summary.** Report plan age and codebase drift, then Status Changes (counts newly complete / invalidated / unchanged), Research Updates (counts refreshed and replaced, plus the most impactful changes), New Deviations Recorded (`E{n}`, with `legacy_id` when migrated), Items Needing User Decision with the reason each needs one, and recommended next steps (review the decisions, `/review-plan`, then implement).

#### `snapshot` — Progress summary

Compact progress summary for standup notes, PR descriptions, or status updates: what completed since the last update (`type=task-completion` entries since the prior `type=checkpoint` or `last_updated`), what deviated and why (`type=deviation`), what is next (prioritized remaining plan items), and any blockers or deferred items (`type=deferral`). **`snapshot` is read-only** — it appends no entries and writes nothing to disk, not even a render. The most recent `PROGRESS-LOG.md` already reflects the log because every mutating op re-renders on append and `snapshot` only runs between mutations; re-rendering here would be redundant at best and would break the no-filesystem-writes invariant at worst (use `--stdout` if you want a fresh render printed).

#### `migrate` — Back-fill `<record>` from a legacy hand-authored `PROGRESS-LOG.md`

One-shot, opt-in, user-invoked. Reads the existing `PROGRESS-LOG.md` and translates each row into an append-only E-entry, then re-renders so the on-disk file is regenerated from the now-populated log (replacing the legacy hand-authored content). The ONLY op authorised to back-fill `type=task-completion` entries.

Per-section translation, best-effort field fill: **Deviations** rows with a `D<n>` ID become `type=deviation` entries carrying `legacy_id = "D<n>"` plus `task_ref` (slug from the affected-task column), `original_intent`, `rationale`, and `commits` (single-element array). **Deferrals** rows with a `DF<n>` ID become `type=deferral` entries carrying `legacy_id = "DF<n>"` plus `task_ref`, `reason`, and `reevaluate_when`. **Completed Items** rows become `type=task-completion` entries with `status = "done"` plus `task_ref` (from the store's ref set, below), `files`, and `commits`; source rows carry no D/DF prefix, so no `legacy_id` is set. **Session Log** rows are a no-op — they are re-derived at render time, and back-filling them would duplicate state.

**`task_ref` comes from the store's `ref` rule, never from slugging the Item heading here.** A back-filled completion joins to a store row on that one string, so a locally-derived slug differing by a character re-executes a completed task or skips an unexecuted one. Take the plan's derived ref set from a plan-mode dry run, where the absent store leaves `added_refs` holding every ref in document order:

```bash
tomlctl tasks import-plan --plan <plan_path> --dry-run
```

Match each Completed-Items row's Item-heading text against that set and adopt the matching ref verbatim. Surface any heading matching none rather than minting a `task_ref` no row carries.

**Idempotency (mandatory):** re-running `migrate` MUST NOT duplicate entries. For D/DF-prefixed rows, scan for the `legacy_id` first (`tomlctl items list <record> --where legacy_id=<D|DF><n> --verify-integrity`) and skip the row on a hit. For completed-items rows (no `legacy_id`), dedupe by the adopted `task_ref` against the existing `type=task-completion` entries. Stage the surviving rows with the Write tool, one JSON object per line, each carrying its own `type`, its legacy `date`, and no `id`, and append them in one all-or-nothing call, which mints a contiguous run in source order so E-numbers stay monotonic across the back-fill:

```bash
tomlctl flow record --slug <slug> --set agent=plan-update --ndjson <staged-rows-path>
```

`flow record` requires the four dispatch fields on every `task-completion`, which a legacy row cannot know: give each back-filled completion `"dispatch_tier":"deep"`, `"dispatch_agent":"implement-deep"`, `"vet":"skipped"` and `"retries":0` — deep is what a reader already assumes for an unknown tier, and it keeps these rows out of the lite-tier entries the vet rate is counted over. Then re-render and update `context.toml` per Step 1.

## Step 3: Apply Updates

1. **Append entries to `<record>`** for any op that mutates plan state, one `tomlctl flow record` call per entry (or one `--ndjson` batch), which mints the id, stamps the date and restamps `last_updated`. Never append with `items add`, `items add-many` or `items apply`, which skip the record checks. Never hand-edit `PROGRESS-LOG.md` — it is regenerated.
2. **Re-render `PROGRESS-LOG.md`** with `tomlctl flow render-progress-log --slug <slug>` as the last step of every mutating op.
3. **Update the outline** when completion markers or wave status changed; **do NOT touch detail documents** unless a deviation fundamentally changes the implementation approach they describe. Refresh the "Last updated" line in `RESEARCH-NOTES.md` whenever this op edits it — it is the one plan document whose format carries that line, and `PROGRESS-LOG.md` has none (its content is a pure function of `<record>`'s `last_updated`).
4. **Update the resolved flow's `context.toml`.** It is touched by every state-changing op. Preserve `created` verbatim and preserve key order; introduce no inline comments. Never type `updated`: every `set` on `context.toml` refreshes it to today (UTC) in the same write, as every `flow record` append refreshes `<record>`'s `last_updated`. **Date guard**: before the op's first write, read the stored value with `tomlctl get <context_path> updated --raw` and compare it with `date -u +%F`. A stored date later than today means the machine clock runs behind and both stamps would move backwards; prompt via `AskUserQuestion` with the observed delta, offering to proceed on the machine clock, to keep the stored dates (pass `--no-stamp` on every write of the op), or to abort — never write silently over a regressing date. Write `[tasks].total`, `[tasks].completed` and `status` in one `set`:

   ```bash
   tomlctl set <context_path> status <status> --set tasks.total=<total> --set tasks.completed=<completed>
   ```

   Take `[tasks].total` from the store's row count (`tomlctl tasks list --slug <slug> --count --raw`), and **derive `[tasks].completed` from `<record>`** on every write per the execution-record skill's pipeline, under the Task-store section's join gate — **precondition**: verify `<record>` exists before running the derivation, and halt with a surfaced error only if `[artifacts].execution_record` is genuinely unresolvable; never let the pipeline silently emit 0 and overwrite a valid prior count. **Leave `[tasks].in_progress` untouched** — it is written only by `/implement` during live execution; read it if you need to display it, but never write it back. Write `status` from `{draft, in-progress, review, complete}`, using `review` when every item is done or all remainders are deferred; only the explicit `complete` op may write `complete`. Append a `type=status-transition` entry when `status` changes value, per the reconciler contract. Only `reconcile` may refine `scope`. Compute `[artifacts]` from `slug` and write it back if absent.
5. Present a summary of changes made to `<record>`, the rendered `PROGRESS-LOG.md`, and the flow's `context.toml`.

## Important Constraints

- **Propose, don't assume** — show the evidence and let the user confirm before committing plan changes when marking items complete or recording deviations. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Dispatch every agent one-shot — never pass `name:`** — every agent this command launches (`general-purpose`, `research-lite` / `research-deep`) is a one-shot lens that returns a payload and is done. A named spawn becomes an `in_process_teammate` with no return channel: its final text reaches no one and its report arrives only if it calls `SendMessage` back. The built-in `general-purpose` carries no teammate-delivery instruction, so naming one loses its inventory outright — you get a "Teammate @x finished" notification and nothing else. Treat a payload-less finish as a failed dispatch and re-dispatch unnamed; a completed teammate has nothing left to send.
- **Deviations capture design-level differences, not typos** — no `type=deviation` entry for variable naming; deviations reflect meaningful departures from the planned approach.
- **Plans stay human-readable** — the agent is a maintainer, not the owner. Do not restructure the plan format outside the explicit `reformat` / `catchup` ops, and do not add machine-only metadata. `PROGRESS-LOG.md` is the one exception: it is regenerated and must not be hand-edited (its first line warns the reader).
- **Append-only log; rendered view is regenerated** — `<record>` entries are never mutated; corrections append a new entry with `supersedes_entry` (the render surfaces the latest per chain, older entries remain for audit, and there is no separate backlink — it is implied by the forward pointer). Plan documents themselves are edited in place, never truncated and rewritten, outside `reformat` / `catchup`.
- **Separate commits** — commit plan updates separately from code changes unless the deviation is inherent to the implementation (a plan said "add column X" but you added "column Y" — that code plus plan update belongs together).
- **Concrete re-evaluation triggers** — `reevaluate_when` values must be specific and observable ("when X happens"), never vague ("when we have time").
