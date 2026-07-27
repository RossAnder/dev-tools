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

Invoke the `flow-contract-execution-record-schema` skill to load the canonical contract for the per-flow append-only log at `.claude/flows/<slug>/execution-record.toml` (field set; the `task-completion` / `verification` / `deviation` / `deferral` / `reconcile` / `status-transition` / `checkpoint` type vocabulary with each type's required fields; monotonic `E{n}` id minting; the two-call heredoc write contract; append-only + supersession; the `[tasks].completed` derivation; read-path `--verify-integrity` integrity contract; field-length caps; and read rules). Every reference below to "the canonical two-call heredoc pattern" points at that contract. Resolve `<record>` as `[artifacts].execution_record` and use the fully-qualified path in every `tomlctl` call — never the bare filename `execution-record.toml`. No manual bootstrap is needed: `flow init` / `/plan-new` pre-seed the record, and any first mutating write auto-creates a missing recognised flow file with the `schema_version = 1` + `last_updated = <today>` skeleton and its `.sha256` sidecar in the same transaction. (If a record exists but its sidecar is missing, repair with `tomlctl integrity refresh <path>` — that is sidecar repair, not bootstrap.)

Invoke the `flow-contract-reconciler` skill to load the reconciler contract (build a `task_ref` skip-set before appending, skip duplicates, gate `status-transition` appends on an actual status change, never silently back-fill completions, always re-render after appends, and supersede-rather-than-duplicate on the reconcile / deviation / deferral dedupe keys). It binds every op below that appends to `<record>`, not just `status`.

`PROGRESS-LOG.md` is a DERIVED artifact — never hand-authored, never carrying a `.sha256` sidecar. Regenerate it deterministically as the last step of every mutating op:

```bash
tomlctl flow render-progress-log --slug <slug>
```

This rebuilds the file as a pure function of `<record>` plus the flow title (the `# Plan:` header reached via `context.toml` → `plan_path`): the `<!-- Generated from execution-record.toml. Do not edit by hand. -->` marker line and the four tables (Completed Items / Deviations / Deferrals / Session Log). Pass `--stdout` to print without writing, `--verify-integrity` to check the record's sidecar first.

## Step 1: Locate the Plan

**Reason thoroughly through plan location and operation analysis** before dispatching agents. `slug` and `context_path` are bound from Step 0 — do not re-resolve. The bootstrap envelope does NOT pass `plan_path` through, so read it from the context file (`tomlctl get <context_path> plan_path`): single-file plans point at the plan, multi-file plans at the outline. If required fields are missing or the file is malformed, prompt the user rather than synthesising defaults.

Resolve the plan in this order: an explicit `$ARGUMENTS` path (if a directory, classify its markdown by role — outline/master, numbered detail documents, `PROGRESS-LOG.md`, deferrals); else the resolved flow's `plan_path` when the referenced file or directory exists; else recently-modified files under `docs/plans/` (or the project's established plans directory) — one recent candidate is used, several are listed for the user; else ask. Offer to create a progress log if the plan has none.

Once located, update the resolved flow's `context.toml`: set `updated` to today (honouring the Step-3 date validation), set `status` per the operation's rules, write `[tasks].total` from the plan-document item count, derive `[tasks].completed` from `<record>`, preserve `created` verbatim and preserve key order, and compute `[artifacts]` from `slug` if absent. Leave `[tasks].in_progress` untouched — that field is written only by `/implement` during live execution. When every plan item is complete (or all remainders deferred), set `status = "review"`, **never** `"complete"`: `review` means "implementation finished, awaiting explicit user sign-off" and keeps the flow targetable by `/review`, `/optimise`, `/optimise-apply`, and `/review-apply` (their filter is `status != "complete"`). Auto-transition to `complete` is forbidden because it strands a freshly-implemented plan beyond auto-resolution before the user has had a chance to review or optimise it; only the explicit `complete` op may write it.

## Step 2: Determine Operation

Parse the operation from `$ARGUMENTS` after the path. `reformat` and `catchup` rewrite plan files and MUST honour the plan-restructure contract — **invoke the `flow-contract-plan-restructure` skill** to load it (byte-for-byte heading preservation and the mandatory pre-write heading-equality assertion, heading-extraction normalisation, archive-before-rewriting, the multi-file and single-file output structures, the RESEARCH-NOTES.md format, `## User Decisions` and `## Execution Policy` survival with checkpoint-marker renumbering, inferred deviations/deferrals, PROGRESS-LOG regeneration, and the present-summary-then-write-immediately rule).

#### `status` — Update completion markers

Scan plan items against the codebase and git history: for each item, check whether the referenced files exist, the described changes are present, and the relevant tests pass. Apply the reconciler contract before any append — this op is auto-invoked by `/implement` Phase 4.5 immediately after `/implement` wrote its own completions, so the skip-set is what stops a double-write. Then re-render `PROGRESS-LOG.md` and update `context.toml` per Step 1. Writes `status ∈ {in-progress, review}` only — MUST NOT write `complete`.

#### `complete` — Explicitly mark the flow as complete

User-invoked; the ONLY path that may set `status = "complete"`. Run once the user has finished `/review`-ing and `/optimise`-ing the implemented plan and is ready to drop it from auto-resolution. In order:

1. Read `<old_status>` via `tomlctl get <context_path> status --verify-integrity`.
2. **Refuse to transition from `draft`** — emit `flow <slug>: refusing transition draft → complete. A plan that was never in-progress cannot be marked complete. Run /implement first, or transition via /plan-update <slug> status.` and exit.
3. **No-op if already `complete`** — emit `flow <slug>: already complete — no change.` and exit; no log entry, no render.
4. **Warn-if-incomplete gate.** Count open items in the resolved review and optimise ledgers with `tomlctl items list <ledger> --status open --count --raw` (plus `--pluck id --raw` for the ID lists), guarded by a file-existence test. Distinguish file-absent (acceptable — count 0) from tomlctl-failed (must surface): let a non-zero exit propagate and halt, and never swallow it with a bare `2>/dev/null`. If the combined count is > 0, ask via `AskUserQuestion`: `<N> open finding(s) on flow <slug>: <r_count> review (<r_list>), <o_count> optimise (<o_list>). Mark complete anyway?` — ID lists capped at 5 each plus `...`; options `Mark complete anyway` (proceed, recording the override) or `Cancel` (exit without writing). **If `AskUserQuestion` is unavailable** (non-interactive harness, no open question slot), refuse: emit `flow <slug>: complete blocked — N open items, AskUserQuestion unavailable for override. Re-run interactively or transition the open items first.` and exit.
5. Set `status = "complete"` and `updated` to today; preserve `created` and key order. When `<old_status> == "in-progress"`, surface the informational note `flow <slug>: skipping the review intermediate state (status was in-progress, transitioning directly to complete). Most flows should pass through review (set via /plan-update <slug> status) before completing.` — the user invoked `complete` explicitly, so honour it.
6. Append a `type=status-transition` entry with `from_status` / `to_status`, minting the id via `tomlctl items next-id <record> --prefix E`. Its `summary` MUST record whether the warn-gate fired and was overridden (`"User explicitly marked flow complete via /plan-update <slug> complete"`, or the same with `(warn-if-incomplete gate overridden with N open items)` appended). Conclude with `tomlctl set <record> last_updated <today>`.
7. Re-render `PROGRESS-LOG.md`, then print `flow <slug>: status <old_status> → complete. Auto-resolution will skip this flow on subsequent /review, /optimise, /implement runs (use --flow <slug> to target explicitly).`

**Gate ordering is load-bearing**: the step-4 queries and prompt MUST run after the step-3 no-op check (otherwise an already-complete flow is re-prompted) and before the step-5 write (otherwise the transition lands before the user can cancel).

#### `deviation` — Record a deviation

Gather evidence from the conversation and git history — which task was affected, the original intent, what was actually done, and why — and confirm with the user before writing. Append a `type=deviation` entry to `<record>` via the canonical two-call heredoc pattern, with `task_ref` (opaque title slug), `original_intent`, `rationale`, and `commits[]` beyond the always-required five fields; set `supersedes_entry = "E<n>"` when superseding an earlier deviation (supersession is the forward pointer, never number re-use). Mint the id with `tomlctl items next-id <record> --prefix E`. This op MUST NOT mint legacy IDs of any kind. Then re-render `PROGRESS-LOG.md` and update `context.toml` per Step 1.

#### `defer` — Register a deferral

Gather evidence — which task is being deferred, why, and the **re-evaluation trigger**, which must be a concrete observable condition ("when frontend types are next refactored", "when migrating to .NET 11") and never a vague one ("later") — and confirm with the user before writing. Append a `type=deferral` entry with `task_ref`, `reason`, and `reevaluate_when`; `legacy_id = "DF<n>"` is set only by `migrate`, never here. Mint the id via `tomlctl items next-id <record> --prefix E`; mint no legacy IDs. Then re-render and update `context.toml` per Step 1. If every remaining non-complete item is now deferred, set `status = "review"` — never `"complete"`.

#### `reconcile` — Full plan-code reconciliation

The most comprehensive operation. Launch **two** `subagent_type: "general-purpose"` agents in a single response message — do not reduce the count; forward and reverse are distinct perspectives that cannot be combined.

- **Agent 1 (forward, plan → code)**: read every plan item and its expected outcome; for items marked Done, verify the expected artifact exists (files present, code patterns present, tests pass); for items marked Not Done / In Progress, check whether they were implemented without the plan being updated; check `git log` since the progress log's last-updated date for commits touching plan-scoped files. Flags items done but unmarked, items marked done then broken by later changes, and new work tracked by no plan item.
- **Agent 2 (reverse, code → plan)**: run `git diff --name-only {baseline}..HEAD` (baseline = the progress log's last-updated commit or `git merge-base HEAD master`); for each changed file check whether a plan item covers it. Flags untracked changes, stale items (marked In Progress with no recent commits touching the relevant files), and implicit deviations.

**Reason thoroughly through reconciliation synthesis** — cross-reference both agents, resolve conflicting evidence, and determine the accurate status of every plan item before writing. Each agent appends its own `type=reconcile` entry with `direction ∈ {forward, reverse}`, `findings_count`, and `commits_checked[]`. Follow-up deviations and deferrals discovered during reconciliation are recorded as separate `type=deviation` / `type=deferral` entries via the ops above — never inlined into the reconcile entries. The reconciler contract applies in full here.

Produce the reconciliation report **and apply all updates in the same response** — do not pause for confirmation: agent results are in context now and are lost to compaction if you wait, and the user can review and revert via git. The report covers Status Updates (old → new with commit/file evidence), Unrecorded Deviations (with a suggested `type=deviation` entry), Untracked Changes, Stale Items, **Unrecorded Completions as gap flags that MUST NOT be auto-appended** (per reconciler rule 4 — point the user at `migrate` or at having `/implement` re-record the completion), and Suggested Deferrals with trigger suggestions. Then re-render `PROGRESS-LOG.md` and update `context.toml` in the same write batch per Step 1 — additionally, **refine `scope`** if reconciliation reveals edits outside the original scope (add the new globs, preferring `<dir>/**`; never shrink `scope` unless the user asks), and set `status` to `review` when every item reconciled as done or deferred, else `in-progress`. This op MUST NOT set `complete`.

#### `reformat` — Rewrite plan into standardized structure

Read the entire existing plan and rewrite it into the standardized structure per the plan-restructure contract. **This operation ONLY restructures documents** — it performs no reconciliation, status updates, or codebase validation; those belong to `reconcile` and `status` as a separate step afterwards.

Launch **two** `subagent_type: "general-purpose"` agents in a single response message; do not reduce the count. **Agent 1 (content extraction and classification)** reads every plan document in scope and returns the full classified inventory — tasks/items with status, effort, risk, dependencies; completed items with commit references and dates; research notes and corrections; deviations (whether legacy `D<n>`-numbered or embedded in prose); deferrals with any stated triggers; `## User Decisions` entries (question, chosen answer, prompting finding); the `## Execution Policy` section; verification criteria; dependencies; and context/rationale. **Nothing from the original documents may be missing.** **Agent 2 (codebase state snapshot)** returns a concise informational snapshot: which plan-referenced files exist, which changed recently, the latest commit touching plan-scoped files, and any obviously-completed items the plan does not reflect.

**Reason thoroughly through reformat synthesis** — cross-reference both agents to confirm every piece of original content is accounted for and correctly classified before writing. Then produce the reformatted plan per the restructure contract, and update `context.toml` per Step 1.

#### `catchup` — Revive a stale plan with fresh research and re-exploration

For plans that have fallen behind the codebase. Combines research, reconciliation, and reformat into one pass — the most expensive operation. Runs three phases sequentially; do not skip a phase or wait for user input between them. Archives before rewriting and honours the plan-restructure contract throughout.

**Phase 1** — launch **three** agents in a single response message, non-overlapping scopes, do not reduce the count. **Agent 1 (codebase re-exploration, `general-purpose`)**: read every file the plan references (do they exist? moved, renamed, deleted?), search for code implementing plan items even in different files or via different approaches, identify structural changes since the plan was written, map the current architecture in the plan's domain, check `git log` for the full history in scope, and return a comprehensive current-state inventory. **Agent 2 (technology and API research, `research-lite`** for the default mechanical case; escalate to `research-deep` when the plan introduces architectural pattern questions or library comparisons, stating `DISPATCH: research-deep — <reason>` at the top of the prompt**)**: research the current state of every technology, library, and framework version the plan references, flag deprecated APIs / removed features / outdated guidance, and return a technology assessment with specific corrections. **Agent 3 (content extraction and classification, `general-purpose`)**: same contract as `reformat`'s Agent 1.

**Phase 1.5 — vet Agent 2's output (orchestrator).** Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule). Scope: **Agent 2 only** — Agents 1 and 3 are exempt because Phase 2 already cross-references their outputs. Sample at least 3 findings (or all if fewer). Before sampling, verify every "deprecated" / "removed" / "superseded" claim: re-query Context7 for the API, WebFetch the library's official changelog, and confirm the version pin in the project manifest matches the claimed version — these are the highest-impact assertions because they drive plan rewrites. Carry only post-vet findings into Phase 2; propagating a fabricated tech finding into a rewritten plan corrupts the plan and the user's trust in the catchup.

**Phase 2 — synthesise and rewrite.** **Reason thoroughly**: cross-reference codebase state, vetted technology research, and the content inventory to determine accurate status for every plan item, identify stale research notes, and resolve conflicts between plan expectations and codebase reality. Produce the reformatted plan per the restructure contract, additionally: update task status from Agent 1's findings (done items get commit evidence, partial items get noted, no-longer-relevant items get flagged for deferral); replace stale RESEARCH-NOTES.md content with Agent 2's vetted findings, keeping still-valid notes and marking outdated ones superseded; update file paths to match the current structure; **flag invalidated tasks for user decision rather than silently dropping them**; and append `type=deviation` / `type=deferral` entries for implementations that happened differently and items no longer actionable. Codebase realignment may suggest *file-path* updates (fine) but never *heading text* changes. Update `context.toml` per Step 1 and write all files immediately in the same response.

**Phase 3 — catchup summary.** Report plan age and codebase drift, then Status Changes (counts newly complete / invalidated / unchanged), Research Updates (counts refreshed and replaced, plus the most impactful changes), New Deviations Recorded (`E{n}`, with `legacy_id` when migrated), Items Needing User Decision with the reason each needs one, and recommended next steps (review the decisions, `/review-plan`, then implement).

#### `snapshot` — Progress summary

Compact progress summary for standup notes, PR descriptions, or status updates: what completed since the last update (`type=task-completion` entries since the prior `type=checkpoint` or `last_updated`), what deviated and why (`type=deviation`), what is next (prioritized remaining plan items), and any blockers or deferred items (`type=deferral`). **`snapshot` is read-only** — it appends no entries and writes nothing to disk, not even a render. The most recent `PROGRESS-LOG.md` already reflects the log because every mutating op re-renders on append and `snapshot` only runs between mutations; re-rendering here would be redundant at best and would break the no-filesystem-writes invariant at worst (use `--stdout` if you want a fresh render printed).

#### `migrate` — Back-fill `<record>` from a legacy hand-authored `PROGRESS-LOG.md`

One-shot, opt-in, user-invoked. Reads the existing `PROGRESS-LOG.md` and translates each row into an append-only E-entry, then re-renders so the on-disk file is regenerated from the now-populated log (replacing the legacy hand-authored content). The ONLY op authorised to back-fill `type=task-completion` entries.

Per-section translation, best-effort field fill: **Deviations** rows with a `D<n>` ID become `type=deviation` entries carrying `legacy_id = "D<n>"` plus `task_ref` (slug from the affected-task column), `original_intent`, `rationale`, and `commits` (single-element array). **Deferrals** rows with a `DF<n>` ID become `type=deferral` entries carrying `legacy_id = "DF<n>"` plus `task_ref`, `reason`, and `reevaluate_when`. **Completed Items** rows become `type=task-completion` entries with `status = "done"` plus `task_ref` (slug from the Item heading text), `files`, and `commits`; source rows carry no D/DF prefix, so no `legacy_id` is set. **Session Log** rows are a no-op — they are re-derived at render time, and back-filling them would duplicate state.

**Idempotency (mandatory):** re-running `migrate` MUST NOT duplicate entries. For D/DF-prefixed rows, scan for the `legacy_id` first (`tomlctl items list <record> --where legacy_id=<D|DF><n> --verify-integrity`) and skip the row on a hit. For completed-items rows (no `legacy_id`), dedupe by derived `task_ref` slug against the existing `type=task-completion` entries. Mint each id via `tomlctl items next-id <record> --prefix E` so E-numbers stay monotonic across the back-fill, apply each authorised append with the canonical two-call heredoc pattern, then re-render and update `context.toml` per Step 1.

## Step 3: Apply Updates

1. **Append entries to `<record>`** for any op that mutates plan state, using the canonical heredoc-stdin two-call pattern. Never stage payloads via a tempfile. Never hand-edit `PROGRESS-LOG.md` — it is regenerated.
2. **Re-render `PROGRESS-LOG.md`** with `tomlctl flow render-progress-log --slug <slug>` as the last step of every mutating op.
3. **Update the outline** when completion markers or wave status changed; **do NOT touch detail documents** unless a deviation fundamentally changes the implementation approach they describe. Always refresh the "Last updated" date on the outline and any other actively-edited plan file (`PROGRESS-LOG.md` has no separate line — its content is a pure function of `<record>`'s `last_updated`).
4. **Update the resolved flow's `context.toml`.** It is touched by every state-changing op. Preserve `created` verbatim and preserve key order; introduce no inline comments. Set `updated` to today's ISO date, subject to **date validation**: before writing `updated` (or `<record>`'s `last_updated`), verify `<today> >= existing_value` and `<today> <= existing_value + 30 days` — the upper bound allows timezone drift but rejects wild clock skew. On violation, prompt via `AskUserQuestion` with the observed delta, offering the machine's clock value, the existing stored value, or abort; do not write silently on any of the three error cases. Write `[tasks].total` from the plan-document item count, and **derive `[tasks].completed` from `<record>`** on every write per the execution-record skill's pipeline — **precondition**: verify `<record>` exists before running the derivation, and halt with a surfaced error only if `[artifacts].execution_record` is genuinely unresolvable; never let the pipeline silently emit 0 and overwrite a valid prior count. **Leave `[tasks].in_progress` untouched** — it is written only by `/implement` during live execution; read it if you need to display it, but never write it back. Write `status` from `{draft, in-progress, review, complete}`, using `review` when every item is done or all remainders are deferred; only the explicit `complete` op may write `complete`. Append a `type=status-transition` entry when `status` changes value, per the reconciler contract. Only `reconcile` may refine `scope`. Compute `[artifacts]` from `slug` and write it back if absent.
5. Present a summary of changes made to `<record>`, the rendered `PROGRESS-LOG.md`, and the flow's `context.toml`.

## Important Constraints

- **Propose, don't assume** — show the evidence and let the user confirm before committing plan changes when marking items complete or recording deviations. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Deviations capture design-level differences, not typos** — no `type=deviation` entry for variable naming; deviations reflect meaningful departures from the planned approach.
- **Plans stay human-readable** — the agent is a maintainer, not the owner. Do not restructure the plan format outside the explicit `reformat` / `catchup` ops, and do not add machine-only metadata. `PROGRESS-LOG.md` is the one exception: it is regenerated and must not be hand-edited (its first line warns the reader).
- **Append-only log; rendered view is regenerated** — `<record>` entries are never mutated; corrections append a new entry with `supersedes_entry` (the render surfaces the latest per chain, older entries remain for audit, and there is no separate backlink — it is implied by the forward pointer). Plan documents themselves are edited in place, never truncated and rewritten, outside `reformat` / `catchup`.
- **Separate commits** — commit plan updates separately from code changes unless the deviation is inherent to the implementation (a plan said "add column X" but you added "column Y" — that code plus plan update belongs together).
- **Concrete re-evaluation triggers** — `reevaluate_when` values must be specific and observable ("when X happens"), never vague ("when we have time").
