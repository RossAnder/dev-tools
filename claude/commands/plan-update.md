---
description: Update plan documents — track progress, deviations, deferrals, and reconcile against codebase
argument-hint: [plan path] [operation: status|complete (gated)|deviation|defer|reconcile|reformat|catchup|snapshot|migrate]
---

# Plan Update

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

## Overview

Update plan documents as living records — track progress, document deviations with rationale, register deferrals with re-evaluation triggers, and reconcile plan expectations against actual code state. The nine sub-operations (`status` / `complete` / `deviation` / `defer` / `reconcile` / `reformat` / `catchup` / `snapshot` / `migrate`) are defined under `## Step 2: Determine Operation` below.

## Flow Context

Flow resolution + doctor checks are delegated to the `flow-bootstrap` sub-agent: Step 0 builds a JSON input envelope, dispatches the agent, gates on `envelope.ok`, and binds `envelope.resolved.{slug, context_path, artifacts.*, status, plan_path, scope, stale}` plus `envelope.doctor.ok` for downstream phases. The contract also covers project-local `.claude/` path resolution, the status vocabulary (`draft` / `in-progress` / `review` / `complete`) and the no-auto-complete rule, slug derivation, canonical artifact paths, completed-flow handling, the legacy `.claude/active-flow` ignore, and the mandatory bootstrap-summary console line.

Invoke the `flow-contract-flow-context` skill to load the full flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, and the mandatory bootstrap-summary console line).

## Step 0: Pre-flight (flow resolution + doctor)

Dispatch the `flow-bootstrap` sub-agent with a single JSON-encoded input envelope. The
agent emits one JSON object on stdout; parse it as `envelope`. All downstream phases consume
fields from `envelope.resolved` and `envelope.doctor`.

Input envelope (build at dispatch time):

```json
{
  "command": "plan-update",
  "flow_override": <--flow value or null>,
  "path_args": <$ARGUMENTS-derived path list — array of strings, [] if no path args>,
  "branch": <git branch --show-current or null>,
  "worktree": <git rev-parse --show-toplevel or null>,
  "cwd": <pwd or null>,
  "require_artifacts": ["execution_record"],
  "staleness_threshold": "7d"
}
```

Dispatch via the `Task` tool with `subagent_type: "flow-bootstrap"`. After parse:

1. **Gate on `envelope.ok`**. If `false`, surface `envelope.errors` to the user verbatim
   and halt. Do not proceed to scope analysis or any downstream phase.
2. **Bind for downstream**: `slug = envelope.resolved.slug`, `context_path =
   envelope.resolved.context_path`, `artifacts = envelope.resolved.artifacts` (object with
   `review_ledger` / `optimise_findings` / `execution_record` / `plan_review_findings`),
   `doctor_ok = envelope.doctor.ok` when `envelope.doctor` is non-null.
3. **No-flow fallback**: when `envelope.resolved.resolved == false`, the carrier follows
   its flow-less convention (`/review` → `.claude/reviews/<scope>.toml`; `/optimise` →
   `.claude/optimise-findings/<scope>.toml`; plan/implement/tdd carriers prompt the user
   per `envelope.warnings`). `envelope.resolved.tie_candidates` (when non-empty) lists the
   slugs surfaced for the user prompt.
4. **Doctor-fail handling**: when `envelope.doctor.ok == false`, surface
   `envelope.doctor.checks` (filtering for `ok == false`) and ask the user before the
   carrier mutates any artifact. Auto-repair (`tomlctl flow doctor --fix`) is the
   orchestrator's call — bootstrap is read-only.
5. **Staleness**: read `envelope.resolved.stale.stale` (boolean) plus
   `envelope.resolved.stale.reason`. When `true` AND the carrier is `/review` or
   `/optimise`, invoke the `plan-update` skill with literal arg `reconcile` before
   continuing.

## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Gate this step ONLY when `envelope.plans_directory == null` (the bootstrap agent normalises both the unset case AND the literal `"__DONT_ASK__"` sentinel to `null`); when non-null, skip entirely and use the already-bound value. The prompt builds a single-select option list (`docs/plans/` recommended → `.claude/plans/` when the directory exists → `other → free-text` → `Don't ask again`), handles the headless empty-answer case by binding `docs/plans/` in-memory without persisting, arbitrates the `Don't ask again` sentinel, runs the free-text follow-up, persists the chosen string to `.claude/settings.json`, and binds `plans_directory` for downstream phases. The wording is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan`.

Invoke the `flow-contract-plansdirectory-prompt` skill to load the first-use prompt contract (gate on `envelope.plans_directory == null`, option-list construction, single-select AUQ ordering, headless empty-answer in-memory binding, `Don't ask again` sentinel arbitration, free-text follow-up, persist-via-`tomlctl json set`, and downstream binding).

## Execution Record Schema

The per-flow append-only log at `.claude/flows/<slug>/execution-record.toml` records every task-completion, verification, deviation, deferral, reconcile, status-transition, and checkpoint emitted by `/plan-new`, `/implement`, and `/plan-update`. `PROGRESS-LOG.md` is a deterministic render of this log, regenerated by `tomlctl flow render-progress-log --slug <slug>` (Completed Items / Deviations / Deferrals / Session Log tables), and `[tasks].completed` is derived from it. The contract is the single source of truth for the file's shape: the canonical schema and per-type required fields, the `id` minting / monotonic-`E{n}` rule, the two-call heredoc write idiom (fully-qualified path required — never the bare filename), append-only + supersession semantics, the `[tasks].completed` derivation pipeline, the `--verify-integrity` read-path contract, field-length caps, and read rules.

Invoke the `flow-contract-execution-record-schema` skill to load the canonical execution-record schema (field set, type vocabulary, the two-call heredoc write contract, append-only + supersession, `[tasks].completed` derivation, read-path integrity contract, field-length caps, and read rules). Every per-operation body below that references "the canonical two-call heredoc pattern" or "Task 6's recipe" is pointing at this contract. `PROGRESS-LOG.md` is NOT hand-rendered: regenerate it deterministically with `tomlctl flow render-progress-log --slug <slug>` (a pure function of `execution-record.toml` + the flow title), as shown below:

```bash
tomlctl flow render-progress-log --slug <slug>
```

# Plan Maintenance

Maintain implementation plan documents as living records. Track progress against the codebase, document deviations with rationale, register deferrals with re-evaluation triggers, and reconcile plan expectations against actual code state.

Works in two modes:
- **Targeted operation** — `/plan-update docs/plans/todo/prod_preparation/ status` to run a specific operation
- **Auto-detect** — `/plan-update` after implementation work to update the relevant plan based on what changed

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and reconciliation depth.

## Step 1: Locate the Plan

**Reason thoroughly through plan location and operation analysis.** Understand the plan structure, document hierarchy, and what the requested operation needs before dispatching agents.

**Bind from Step 0**: the resolved flow's `slug`, `context_path`, and `plan_path` (read via `tomlctl get <context_path> plan_path` — the bootstrap envelope does NOT pass `plan_path` through; it surfaces only resolved metadata) are bound from Step 0's `envelope.resolved`. Do not re-resolve. Read `.claude/flows/<slug>/context.toml` (i.e. `context_path`) for the `plan_path` field — single-file plans point at the plan; multi-file plans point at the outline file. Honour the TOML read contract from the `## Flow Context` section: if required fields are missing or the file is malformed, prompt the user rather than synthesising defaults.

1. If $ARGUMENTS specifies a plan path (not just `--flow`), use that. If it's a directory, classify all markdown files by role:
   - **Outline/master** — defines structure, phases, references other files
   - **Detail documents** — numbered implementation docs with actionable tasks
   - **Progress log** — `PROGRESS-LOG.md` or equivalent tracking document
   - **Deferrals** — if a dedicated deferrals section/file exists
2. If no path specified, locate the active plan:
   a. Check conversation context for plan references or recently completed implementation work.
   b. Use `plan_path` from the resolved flow's `context.toml` (obtained via the flow resolution above). If the referenced plan file/directory is present, use it.
   c. Check `docs/plans/` (or the project's established plans directory) for recently modified plan files. If a single plan was modified recently, use it. If multiple candidates exist, list them and ask the user.
   d. If ambiguous or nothing found, ask the user which plan to update.
3. **Update flow context**: Once the plan is located, update the resolved flow's `.claude/flows/<slug>/context.toml`:
   - Set `updated` to today's date (ISO 8601 date value). Honour the date-validation check defined in Step 3 Task 6 (reject `<today>` outside `[existing_value, existing_value + 30 days]`; prompt via `AskUserQuestion` rather than writing silently on violation).
   - Set `status` according to what this operation determined (see the per-operation rules below). Accepted values: `draft`, `in-progress`, `review`, `complete`.
   - Update `[tasks].total` (plan-document-driven) and **derive `[tasks].completed`** from `<record>` per Task 6's recipe in Step 3 (distinct-slug count over `task-completion` entries with `status=done`). Leave `[tasks].in_progress` untouched (honours § flow-context field responsibilities).
   - **Preserve `created` verbatim.** Never regenerate it. Preserve key order. Do not introduce inline comments.
   - If `[artifacts]` is absent, compute it from `slug` and write it back on this same update. The execution-record itself needs **no manual bootstrap**: `flow init` / `/plan-new` pre-seed it, and any first mutating write (`tomlctl items add` / `set`) AUTO-CREATES a missing recognised flow file with the byte-identical `schema_version = 1` + `last_updated = <today>` skeleton (and its `.sha256` sidecar) in the same transaction. Just append — do NOT pre-`Write` a skeleton. (Pass `--no-create` only if you deliberately want a missing file to error instead.) If a record exists but its sidecar is missing (a `--verify-integrity` read fails with no `.sha256`), recover with `tomlctl integrity refresh <path>` — that is a sidecar repair, not a bootstrap.
   - If all plan items are now complete (or all remaining items are deferred), set `status = "review"` — NOT `"complete"`. The `review` status indicates "implementation finished, awaiting explicit user sign-off" and keeps the flow targetable by `/review`, `/optimise`, `/optimise-apply`, and `/review-apply` (their resolution filter is `status != "complete"`, which `review` satisfies). The user transitions `review → complete` by explicitly invoking the `/plan-update <plan> complete` op (see Operations below) — auto-transition to `complete` is forbidden because it strands a freshly-implemented plan beyond auto-resolution before the user has had a chance to /review or /optimise it. Append a `type=status-transition` entry whenever the value changes.
4. If no progress log exists for the plan, offer to create one.

## Step 2: Determine Operation

Parse the operation from $ARGUMENTS (after the path). If no operation specified, default to **reconcile** (the most comprehensive).

### Heading-preservation rule

Both `reformat` and `catchup` rewrite plan files and MUST honour this rule. `task_ref` is an opaque title slug derived from each task's heading text. If a restructure op rephrases a heading (e.g. "Add retry logic" → "Add retry with exponential backoff"), the derived slug changes and `/implement`'s idempotency skip-list misses the completed task, causing the task to re-execute. Therefore restructure ops MUST preserve each task's heading text **exactly as it appeared in the source plan**, byte-for-byte. Rephrasing is allowed ONLY as an explicit deviation recorded via the `deviation` op (which preserves `supersedes_entry` chains). Reordering, regrouping, or recategorizing tasks is allowed — only heading text is immutable.

**Heading-equality assertion (mandatory).** Before writing the restructured output, compare the set of pre-restructure task heading strings against the set of post-restructure task heading strings. On any mismatch (added, removed, or rephrased headings), error and require user intervention rather than writing the rewritten plan. Show the diff so the user can decide whether the change is intentional (record as a `deviation`) or accidental (regenerate with stricter heading preservation).

**Heading extraction rule** (for the equality assertion): from each `### N. Name [S|M|L]` line, extract the `Name` substring — split on `. ` once from the left (after the `### ` prefix), then strip any trailing ` [S]` / ` [M]` / ` [L]` effort tag. Normalise internal whitespace by collapsing runs of ` ` (U+0020) and `\t` (U+0009) to a single space. The assertion compares the **set** of extracted `Name` strings pre- vs post-restructure. Renumbering alone does NOT fail the assertion (numbers are stripped before comparison); rephrasing a heading DOES fail it (explicit deviation required via the `deviation` op). If the source uses a heading style that doesn't match the `### N. Name [S|M|L]` pattern (e.g. legacy plans without effort tags, or `##` instead of `###`), accept it by the same extraction logic: the leading `### ` / `## ` / `#### ` prefix is stripped, the `N. ` numbering prefix is stripped if present, and the trailing effort tag is stripped if present — everything remaining after whitespace normalisation is the `Name`.

### Operations

#### `status` — Update completion markers (reconciler-contract bound)

Scan plan items against the codebase and git history. For each plan task/item, check whether the referenced files exist, the described changes are present, and relevant tests pass. Then apply the **reconciler contract** below before any append, regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>`, and update `context.toml` (set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim, write `status` ∈ {`in-progress`, `review`} per the rules in `## Flow Context` — this op MUST NOT write `status = "complete"`; only the `complete` op below may do so). Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

##### RECONCILER CONTRACT

`/implement` Phase 4.5 auto-invokes `Skill("plan-update", "status")`, so the `status` op is called immediately after `/implement` writes its own `task-completion` entries. Without the reconciler contract, the `status` op would double-write every completion `/implement` just recorded. The acceptance criterion is: **N `task-completion` entries before `status` runs == N entries after** (not 2N). The contract:

1. **Build a skip-set first.** Before any append, query
   ```
   tomlctl items list <record> --where type=task-completion --pluck task_ref --lines --verify-integrity
   ```
   and treat each line of stdout as one `task_ref` in the skip-set. `--lines` emits one JSON value per line (tomlctl 0.2.0+), so no `jq -r '.[]'` unwrap is needed — downstream membership checks can read the output directly.
2. **Skip duplicates.** For any `task_ref` already present in the skip-set, do not append a new `task-completion` entry. The op never duplicates entries that `/implement` (or any prior writer) has already recorded.
3. **Status-transition writes are change-gated.** Append a `type=status-transition` entry (with `from_status` and `to_status`) ONLY when the flow's `status` field actually changes value (e.g. `in-progress` → `review`). Never on every invocation. If `status` is unchanged, skip the transition append entirely.
4. **Never silently back-fill.** The `status` op NEVER appends `type=task-completion` entries — those are exclusively written by `/implement` Phase 2. If reconciliation surfaces an unrecorded completion (e.g. files modified, tests pass, but no matching log entry exists), the op MUST **flag the gap in its reconciliation report** rather than silently appending. Only the `migrate` op (below) is authorised to back-fill `task-completion` entries from a legacy `PROGRESS-LOG.md`.
5. **Render after any appends.** After the (possibly zero) appends complete, regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>`. The render is always run, even when no entries were appended, so the rendered file stays a pure function of the log.
6. **Reconcile entries dedupe by (date, agent).** Before appending a `type=reconcile` entry, query the log for any existing `type=reconcile` entry on the same `date` with the same `agent`. If found, **supersede** it: set `supersedes_entry = "<old id>"` on the new entry. Do NOT leave both live. Rationale: reconcile is idempotent — the same reconcile fired twice on the same day from the same agent should not double-count. The supersession chain preserves the audit trail; the render surfaces only the latest per chain.
7. **Deviation entries dedupe by (task_ref, original_intent, rationale).** Before appending a `type=deviation` entry from any writer (`deviation` op, `reconcile`, `reformat`, `catchup`), query the log for existing `type=deviation` entries matching the same `(task_ref, original_intent, rationale)` triple. If found, **supersede** rather than duplicate: set `supersedes_entry = "<old id>"` on the new entry. Rationale: re-recording an already-captured deviation pollutes the rendered Deviations table and breaks the "latest-per-chain" render guarantee.
8. **Deferral entries dedupe by (task_ref, reason).** Before appending a `type=deferral` entry from any writer (`defer` op, `reconcile`, `reformat`, `catchup`), query the log for existing `type=deferral` entries matching the same `(task_ref, reason)` pair. If found, **supersede** rather than duplicate: set `supersedes_entry = "<old id>"` on the new entry. Rationale: deferring the same task for the same reason twice is a no-op; recording it twice just inflates the rendered Deferrals table.

The contract applies to **every writer** that can emit these types — not just `/plan-update status`. `/plan-update reconcile`, `/plan-update deviation`, `/plan-update defer`, `/plan-update reformat`, `/plan-update catchup`, and `/implement` (when it routes deviations/deferrals through plan-update patterns) all honour rules 6–8. Enforcement lives in each writer's body; this section is the contract writers must follow.

#### `complete` — Explicitly mark the flow as complete

User-invoked op. The ONLY path that may set `status = "complete"` (auto-transitions are forbidden — see the `status` op above and `defer` op below). Run when the user has finished `/review`-ing and `/optimise`-ing the implemented plan and is ready to drop it from auto-resolution.

The op:

1. Read `<old_status>` from `context.toml` via `tomlctl get .claude/flows/<slug>/context.toml status --verify-integrity`.
2. **Refuse to transition from `draft`** — a plan that was never `in-progress` cannot be `complete`. Emit: `flow <slug>: refusing transition draft → complete. A plan that was never in-progress cannot be marked complete. Run /implement first, or transition via /plan-update <slug> status.` and exit. The user must run `/implement` (or manually transition via the `status` op above) first.
3. **No-op if already `complete`.** Emit: `flow <slug>: already complete — no change.` and exit (no log entry, no render).
4. **Warn-if-incomplete gate.** After the no-op check and before any write, count outstanding items from the resolved review/optimise ledgers. Distinguish file-absent (acceptable, count = 0) from tomlctl-failed (must surface — do NOT swallow with bare `2>/dev/null`):

   ```
   r_count=0; r_list=""
   if [ -f .claude/flows/<slug>/review-ledger.toml ]; then
     r_count=$(tomlctl items list .claude/flows/<slug>/review-ledger.toml --status open --count --raw)
     r_list=$(tomlctl items list .claude/flows/<slug>/review-ledger.toml --status open --pluck id --raw)
   fi
   o_count=0; o_list=""
   if [ -f .claude/flows/<slug>/optimise-findings.toml ]; then
     o_count=$(tomlctl items list .claude/flows/<slug>/optimise-findings.toml --status open --count --raw)
     o_list=$(tomlctl items list .claude/flows/<slug>/optimise-findings.toml --status open --pluck id --raw)
   fi
   ```

   If `tomlctl` itself errors (binary missing, lock contention, corrupt TOML), the non-zero exit propagates and the op halts — file-absent yields count = 0 (acceptable), but tomlctl-failed must surface.

   If `r_count + o_count > 0`, invoke `AskUserQuestion` with question text: `<N> open finding(s) on flow <slug>: <r_count> review (<r_list>), <o_count> optimise (<o_list>). Mark complete anyway?` where `<r_list>` and `<o_list>` are short comma-separated ID lists, capped at 5 IDs each + ellipsis (`...`) when truncated. Two options: `Mark complete anyway` (proceeds — set `<override_flag> = true` for step 6) or `Cancel` (exits without writing).

   **AskUserQuestion-unavailable fallback.** If `AskUserQuestion` is not available (non-interactive harness, no open user-question slot), refuse the transition: emit `flow <slug>: complete blocked — N open items, AskUserQuestion unavailable for override. Re-run interactively or transition the open items first.` and exit. The user can re-run interactively or disposition the open items first.

5. Otherwise (status is `in-progress` or `review`): set `status = "complete"` in `context.toml`. Set `updated` to today. Preserve `created` verbatim and preserve key order. **If `<old_status> == "in-progress"`**, surface a one-line console note: `flow <slug>: skipping the review intermediate state (status was in-progress, transitioning directly to complete). Most flows should pass through review (set via /plan-update <slug> status) before completing.` This is informational — the user explicitly invoked `complete`, so honour it.
6. Append a `type=status-transition` entry to `<record>` with `from_status = <old_status>`, `to_status = "complete"`, using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block above. The `summary` field MUST record whether the warn-gate fired and was overridden:
   - When the gate did not fire (no open items): `summary = "User explicitly marked flow complete via /plan-update <slug> complete"`.
   - When the gate fired and the user chose `Mark complete anyway`: `summary = "User explicitly marked flow complete via /plan-update <slug> complete (warn-if-incomplete gate overridden with N open items)"` — substitute the observed open-item count.

   Mint the id with `tomlctl items next-id <record> --prefix E`. Always conclude with `tomlctl set <record> last_updated <today>`.
7. Regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>`.
8. Print a one-line confirmation: `flow <slug>: status <old_status> → complete. Auto-resolution will skip this flow on subsequent /review, /optimise, /implement runs (use --flow <slug> to target explicitly).`

**Gate semantics.** The warn-if-incomplete gate runs at step 4, between the already-complete no-op check (step 3) and the write (step 5) — the queries and AskUserQuestion prompt MUST NOT execute before the no-op check (otherwise an already-complete flow would be re-prompted) and MUST execute before the write (otherwise the transition would land before the user has a chance to cancel).

#### `deviation` — Record a deviation

Capture a deviation from the plan. The agent MUST:

- Gather evidence from the conversation/git history: which task was affected, what the original intent was, what was actually done, and why. Confirm with the user before writing.
- Append a `type=deviation` entry to `<record>` (the resolved value of `[artifacts].execution_record` from the flow's `context.toml` — never the bare filename `execution-record.toml`) using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block above. Required fields beyond the always-required five (`id`, `type`, `date`, `agent`, `summary`): `task_ref` (opaque title slug of the affected task), `original_intent`, `rationale`, `commits[]` (from `git log -1 --format=%H` or the relevant SHAs). Optional: `supersedes_entry = "E<n>"` when this deviation supersedes an earlier one — supersession is by `supersedes_entry` pointing at the prior entry's `id`, NEVER by re-using its number.
- Mint the new `id` via `tomlctl items next-id <record> --prefix E` so the E-counter stays monotonic.
- This op MUST NOT mint legacy IDs of any kind (honours § flow-context field responsibilities — leaves `[tasks].in_progress` untouched).
- After the append, regenerate `PROGRESS-LOG.md` deterministically from `<record>` by running `tomlctl flow render-progress-log --slug <slug>`. Then update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe (Step 3 below), preserve `created` verbatim.

**Example two-call append (fully-qualified path required):**

```
cat <<'EOF' | tomlctl items add .claude/flows/<slug>/execution-record.toml --json -
{"id":"E17","type":"deviation","date":"2026-04-18","agent":"plan-update","task_ref":"add-redis-cache","summary":"Used existing LruCache util rather than introducing Redis","original_intent":"Add Redis dependency for caching","rationale":"src/util/cache.rs already covers the use case","commits":["def5678"],"supersedes_entry":"E9"}
EOF
tomlctl set .claude/flows/<slug>/execution-record.toml last_updated 2026-04-18
```

#### `defer` — Register a deferral

Move a plan item to the deferrals section. The agent MUST:

- Gather evidence from the conversation: which task is being deferred, why, and the **re-evaluation trigger** (a concrete observable condition like "when frontend types are next refactored" or "when migrating to .NET 11" — not vague triggers like "later"). Confirm with the user before writing.
- Append a `type=deferral` entry to `<record>` using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block above. Required fields beyond the always-required five: `task_ref` (opaque title slug of the deferred task), `reason`, `reevaluate_when`. Optional: `legacy_id = "DF<n>"` — only set by the `migrate` op when back-filling from a legacy hand-authored `PROGRESS-LOG.md`; the active `defer` op MUST NOT populate it.
- Mint the new `id` via `tomlctl items next-id <record> --prefix E` so the E-counter stays monotonic.
- This op MUST NOT mint legacy IDs of any kind (honours § flow-context field responsibilities — leaves `[tasks].in_progress` untouched).
- After the append, regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>`. Then update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe (Step 3 below), preserve `created` verbatim. If every remaining non-complete item is now deferred (after consulting the log), set `status = "review"` — NOT `"complete"` (per the explicit-sign-off rule documented in the `status` op above; only `/plan-update <plan> complete` may set `status = "complete"`).

The two-call heredoc shape matches the `deviation` op example above; substitute `type=deferral` and the deferral-specific required fields (`task_ref`, `reason`, `reevaluate_when`) per the Execution Record Schema type vocabulary.

#### `reconcile` — Full plan-code reconciliation
The most comprehensive operation. Launch **two** `general-purpose` agents in parallel (subagent_type: "general-purpose"):

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents. Each provides a distinct reconciliation perspective (forward vs reverse) that cannot be combined.

**Agent 1: Forward reconciliation (plan → code)**
- Read all plan items and their expected outcomes.
- For each item marked "Done", verify the expected artifact exists in the codebase (files exist, code patterns present, tests pass).
- For each item marked "Not Done" or "In Progress", check if it was actually implemented but the plan wasn't updated.
- Check `git log` since the progress log's "Last updated" date for commits touching plan-scoped files.
- Flag: items done but unmarked, items marked done but with subsequent breaking changes, new work not tracked by any plan item.

**Agent 2: Reverse reconciliation (code → plan)**
- Run `git diff --name-only {baseline}..HEAD` where baseline is either the progress log's "Last updated" commit or `git merge-base HEAD master`.
- For each changed file, check whether the change is covered by a plan item.
- Identify untracked changes — code that changed in the plan's scope but has no corresponding plan entry.
- Check for stale items — plan items marked "In Progress" with no recent commits touching the relevant files.
- Look for implicit deviations — implementation that differs from what the plan described.

**Reason thoroughly through reconciliation synthesis.** Cross-reference both agents' findings, resolve conflicting evidence, and determine the accurate status of every plan item before writing updates.

**Each parallel agent appends a `type=reconcile` entry to `<record>`** using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block. Required fields beyond the always-required five: `direction` ∈ {`forward`, `reverse`} (Agent 1 = `forward`, Agent 2 = `reverse`), `findings_count` (integer count of items the agent flagged), `commits_checked[]` (the SHAs the agent inspected). Mint each `id` via `tomlctl items next-id <record> --prefix E`.

**Follow-up deviations and deferrals discovered during reconciliation are recorded as separate entries** via the same patterns the `deviation` and `defer` ops use (above) — append `type=deviation` / `type=deferral` entries with the appropriate fields. Do NOT inline them into the `reconcile` entries.

The same **reconciler contract** that governs `status` (above) applies here: build a skip-set of existing `task-completion` `task_ref` values from `<record>` before any append; never silently back-fill `task-completion` entries (flag gaps in the report instead — `migrate` is the only authorised back-filler); only emit `type=status-transition` when the flow's `status` field actually changes value.

After both agents return, produce the reconciliation report **and apply all updates in the same response** — do not pause for confirmation. Agent results are in context now and may be lost to compaction if you wait. The user can review and revert via git. After all appends, regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>`.

**Update the resolved flow's `context.toml`** as part of the same write batch:
- Write `[tasks].total` (the count of plan items) and **derive `[tasks].completed`** per Task 6's recipe (Step 3 below — distinct-slug count over `task-completion` entries with `status=done`). Leave `[tasks].in_progress` untouched (honours § flow-context field responsibilities).
- Set `updated` to today's date (honours the date-validation check defined in Step 3 Task 6).
- Preserve `created` verbatim.
- **Refine `scope`** if reconciliation reveals the plan's actual edits touched paths outside the original `scope` — add the new globs/paths (prefer `<dir>/**` glob form for directories). Never shrink `scope` below its initial derivation unless the user explicitly asks.
- Set `status` to `review` if every item reconciled as done (or deferred); otherwise `in-progress`. **This op MUST NOT set `status = "complete"`** — only the explicit `complete` op may do that (per the explicit-sign-off rule documented in the `status` op above). If `status` changes value, append a `type=status-transition` entry per the reconciler contract.

```
## Reconciliation Report — [plan name]

**Plan scope**: [files/features covered]
**Period**: [last updated] → [now]
**Commits in scope**: [N]

### Status Updates
- [item] Changed from [old status] → [new status] — evidence: [commit/file]

### Unrecorded Deviations
- [description] — code at [file:line] differs from plan [section]. Suggested `type=deviation` E-entry: task_ref=..., original_intent=..., rationale=...

### Untracked Changes
- [file] changed in [commit] but has no plan coverage

### Stale Items
- [item] marked "In Progress" but no activity since [date]

### Unrecorded Completions (gap flags — DO NOT auto-append)
- [task_ref] — files at [file:line] suggest completion, but no `type=task-completion` entry in `<record>`. Per the reconciler contract, the `status` and `reconcile` ops MUST NOT silently back-fill these. Run `/plan-update <plan> migrate` to back-fill from a legacy `PROGRESS-LOG.md`, or have `/implement` re-record the completion explicitly.

### Suggested Deferrals
- [item] appears blocked or deprioritized — consider deferring with trigger: [suggestion]
```

#### `reformat` — Rewrite plan into standardized structure

Read the entire existing plan (single file or multi-file directory) and rewrite it into a clean, standardized structure. This is a **full rewrite** — the one exception to the "append, don't rewrite" rule. Every piece of content from the original must appear in the output; nothing is discarded.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-reformat state for reference. Create the archive directory if it doesn't exist.

**IMPORTANT: This operation ONLY restructures documents. It does NOT perform reconciliation, status updates, or codebase validation. Those are handled by `reconcile` and `status` as a separate step after reformatting.**

Launch **two** `general-purpose` agents in parallel (subagent_type: "general-purpose"):

**IMPORTANT: You MUST make both Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch both agents.

**Agent 1: Content extraction and classification**
Read every plan document in scope. Extract and classify every piece of content into:
- **Tasks/items** — actionable work items with their current status, effort estimates, risk levels, dependencies
- **Completed items** — items marked done, with any commit references or dates
- **Research notes/corrections** — technical findings, library version notes, API behavior, etc. (e.g. the "Key corrections from research" sections)
- **Deviations** — anything that records a departure from the original plan, whether previously numbered with legacy `D<n>` IDs (preserved as `legacy_id` on migrated entries) or embedded in prose
- **Deferrals** — items explicitly deferred or marked as future work, with any stated triggers
- **User Decisions** — answers captured from `/plan-new` Phase 4 (Directed Questions), recording the question, the chosen answer, and the finding that prompted the question. If the source plan contains a `## User Decisions` section, every entry must survive into the reformatted output as a preserved `## User Decisions` section in the outline (adjacent to `## Approach`). Do not merge into Research Notes or Context — the provenance and question-answer structure must stay intact.
- **Verification criteria** — checklists, test commands, acceptance conditions
- **Dependencies** — stated relationships between items, phases, or waves
- **Context/rationale** — background information, objectives, constraints, scope statements

Return the full classified inventory. **Nothing from the original documents should be missing.**

**Agent 2: Codebase state snapshot**
For the plan's scope, gather current state to inform the reformat:
- Which files referenced by the plan exist? Which have changed recently?
- What's the latest commit touching plan-scoped files? (for "Last updated" dating)
- Are there any obvious completed items that the plan doesn't reflect?

Return a concise state snapshot — this is informational for the reformat, not a full reconciliation.

**Reason thoroughly through reformat synthesis.** Cross-reference both agents' results to ensure every piece of content from the original plan is accounted for and correctly classified before writing the reformatted output.

After both agents return, produce the reformatted plan:

**Output structure for multi-file plans:**

```
{plan-directory}/
├── 00-outline.md              — Master sequencing: objective, constraints, phases/waves, item table with status
├── 01-{topic}.md              — Detail documents (one per major topic/wave)
├── ...                        — (preserve existing detail doc numbering and topics)
├── PROGRESS-LOG.md            — Separated progress tracking (see format below)
└── RESEARCH-NOTES.md          — Extracted research findings, corrections, and technical notes
```

**Output structure for single-file plans:**
Split into at minimum: the plan itself (clean, actionable) + a PROGRESS-LOG.md if there's any status tracking content to extract.

**PROGRESS-LOG.md format**: `reformat` MUST regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>` rather than hand-authoring it. The rendered shape (marker line + the four tables: Completed Items / Deviations / Deferrals / Session Log, their columns, and every value derivation) is the single source of truth in the `flow-contract-execution-record-schema` skill's reference spec (invoked under `## Execution Record Schema` above) — do not duplicate the table layout here. Row identifiers come from the log's `id` field (`E<n>`); migrated entries also carry `legacy_id = "D<n>"` / `"DF<n>"` for back-compat, but it does not appear in the `#` column.

**RESEARCH-NOTES.md format:**

```markdown
# {Plan Name} — Research Notes

> Technical findings, corrections, and version-specific notes extracted from plan documents.
> Reference these from plan items rather than embedding inline.
> Last updated: {date}

## {Topic 1} (referenced by Item #N)
- Finding...
- Source/version note...

## {Topic 2} (referenced by Item #N)
- Finding...
```

**Key rules for reformatting:**
- **Faithful content preservation** — every fact, note, correction, finding, and status marker from the original must appear in the output. Verify by checking the original line count and ensuring no content was silently dropped.
- **User Decisions survive verbatim** — if the source plan has a `## User Decisions` section, copy it intact into the reformatted outline. Do NOT redistribute entries into Research Notes, Context, or Approach; the question/answer/finding triple is meaningful as a unit and downstream agents (including `/implement` and later `/plan-new` runs on adjacent plans) reference it by section.
- **Clean the outline** — the outline should contain the sequencing table, dependencies, constraints, and verification checklists. Research notes, verbose corrections, and progress tracking move to their own files. The outline should reference these files where needed (e.g. "See RESEARCH-NOTES.md §{Topic}").
- Entries carry `legacy_id` for back-compat; no renumbering is required because E-numbers are monotonic.
- **Preserve task headings verbatim** — honours § Heading-preservation rule (above, under `## Step 2: Determine Operation`).
- **Infer deferrals** — items described as "deferred", "future", "nice-to-have", "not needed yet" in the original should be formalized as `type=deferral` E-entries (via the `defer` op pattern) with concrete re-evaluation triggers. If the source row carried a legacy `DF<n>` ID, copy it into `legacy_id`.
- **Infer deviations** — prose that describes "we did X instead of Y" or "the plan said X but actually Y" should be formalized as `type=deviation` E-entries (via the `deviation` op pattern). If the source row carried a legacy `D<n>` ID, copy it into `legacy_id`; supersession is by `supersedes_entry = "E<n>"`, not by re-using legacy numbers.
- **PROGRESS-LOG.md is regenerated, not hand-authored.** The reformat MUST regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>` — NOT by hand-authoring D/DF-numbered markdown. After the inferred deviation/deferral entries are appended to `<record>` and any new completed-items entries are migrated, append exactly **one `type=checkpoint` entry** tagging the restructure (`summary` should describe what changed: "Restructured plan into outline + detail docs + RESEARCH-NOTES.md", etc.). Then run `tomlctl flow render-progress-log --slug <slug>`.
- **Present summary then write immediately** — show the user a brief summary of what files will be created/rewritten and key content movements, then **write all files in the same response without waiting for confirmation**. Do NOT pause and ask "Shall I proceed?" — the agent analysis results are in context NOW and may be lost to compaction if you wait. The user invoked `reformat` intentionally; they can review and revert via git if needed.

After all writes, update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim. Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

#### `catchup` — Revive a stale plan with fresh research and codebase re-exploration

For old or unimplemented plans that have fallen behind the codebase. Performs deep re-exploration of the codebase and fresh research to reorient the plan to current reality, then automatically reformats into the standardized structure. This is the most expensive operation — it combines research, reconciliation, and reformat into one pass.

**Archive before rewriting**: Before overwriting any files, copy the current plan files to `docs/plans/archive/{plan-name}-{YYYY-MM-DD}/`. This preserves the pre-catchup state for reference. Create the archive directory if it doesn't exist.

**This operation runs in three phases sequentially. Do not skip phases or wait for user input between them.**

**Phase 1: Deep exploration and fresh research** — Launch **three** agents in parallel (Agents 1 + 3: subagent_type: "general-purpose"; Agent 2: subagent_type: "research-lite" — see each agent heading for the explicit declaration):

<!-- Migration note (specialised-flow-agents.md Wave 2 §16):
     Agent 2 → subagent_type: "research-lite" — research workload (fetch-and-summarise technology state).
     Agents 1 + 3 stay on general-purpose — reconcile / synthesis workloads.
     reconcile op (lines ~394) + reformat op (lines ~463) trios stay on general-purpose — out of scope per
     docs/plans/specialised-flow-agents.md audit (workloads too varied per Phase A research). -->

**IMPORTANT: You MUST make all three Agent tool calls in a single response message.** **Do NOT reduce the agent count** — launch all three agents. Each has a non-overlapping scope (codebase, technology, content).

**Agent 1: Codebase re-exploration**
Thoroughly explore the current state of the codebase in the plan's scope:
- Read every file the plan references — do they exist? Have they moved, been renamed, or been deleted?
- Search for code that implements plan items, even if in different files or using different approaches than the plan expected
- Identify structural changes since the plan was written (new directories, refactored modules, renamed classes, split files)
- Map the current architecture in the plan's domain — what does the codebase actually look like now?
- Check `git log` for the full history of changes in the plan's scope area
- Return a comprehensive current-state inventory

**Agent 2: Technology and API research** (`subagent_type: "research-lite"` for the default mechanical case; escalate to `subagent_type: "research-deep"` if the plan introduces architectural pattern questions or compares libraries — state the rationale at the top of the prompt as `DISPATCH: research-deep — <reason>`)
Research the current state of every technology, library, and framework version referenced in the plan:
- Check whether the plan's technical approach is still valid or has been superseded by newer patterns
- Flag anything in the plan that references deprecated APIs, removed features, or outdated guidance
- Return a technology assessment with specific corrections needed

**Agent 3: Content extraction and classification**
Same as the `reformat` Agent 1 — read every plan document and extract the full classified inventory (tasks, completed items, research notes, deviations, deferrals, verification criteria, dependencies, context).

**Phase 1.5: Vet agent output (orchestrator)** — Before Phase 2 synthesis, the orchestrator (Opus) MUST vet Agent 2's output (the `research-lite` tech-research run). The catchup operation is the most expensive op in /plan-update — propagating fabricated tech findings into a rewritten plan corrupts the plan and the user's trust in the catchup.

**Scope:** This vet pass applies to Agent 2 (`research-lite` tech research) only. Agents 1 + 3 (general-purpose reconcile / synthesis) are vetting-exempt because the orchestrator already cross-references their outputs in Phase 2.

**Sample size:** Spot-check at least 3 findings from Agent 2 (or all if fewer).

**Lens-specific verification rules:** Verify every "deprecated" / "removed" / "superseded" claim before sampling: re-query Context7 for the API in question; check the library's official changelog (WebFetch on the changelog URL); confirm the version pin in the project manifest matches Agent 2's claimed version. These are the highest-impact assertions because they drive plan rewrites.

The vet pass follows the universal procedure: triage by source agent + evidence-grade, honour any `ESCALATE-TO-DEEP` flags, drop unverified low-confidence findings, spot-check sampled findings against their cited `file:line` / URLs / version pins (sample size per the **Sample size** rule above), drop-or-downgrade-with-rationale, append a durable `[[vet_events]]` ledger entry per vetted agent, emit the mandatory per-agent console line, and re-dispatch (escalating `research-lite` lenses to `research-deep`) on >30% systemic failure.

Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule).

Carry only post-vet Agent 2 findings into Phase 2 synthesis.

**Phase 2: Synthesize and rewrite** — After all three agents return AND vetting completes:

**Reason thoroughly through catchup synthesis.** Cross-reference all three agents' results — codebase state, vetted technology research, and content inventory — to determine accurate status for every plan item, identify which research notes are stale, and resolve conflicts between the plan's expectations and codebase reality.

Using all three agents' results together, produce the reformatted plan following the same structure and rules as the `reformat` operation (outline, detail docs, PROGRESS-LOG.md, RESEARCH-NOTES.md). Additionally:

- **Update task status** based on Agent 1's codebase findings — items that are done get marked done with commit evidence, items that are partially done get noted, items that are no longer relevant get flagged for deferral
- **Replace stale research** in RESEARCH-NOTES.md with Agent 2's fresh findings — keep original notes that are still valid, mark outdated ones as superseded with the updated information
- **Update file paths** throughout the plan to match the current codebase structure
- **Flag invalidated tasks** — if the codebase has changed so fundamentally that a plan item no longer makes sense, note it as needing user decision rather than silently dropping it
- **Add deviations** for any implementation that happened differently from what the plan described — appended as `type=deviation` E-entries to `<record>` (via the `deviation` op pattern, with `legacy_id` populated when migrating a numbered legacy row)
- **Add deferrals** for items that are no longer actionable in their current form — appended as `type=deferral` E-entries to `<record>` (via the `defer` op pattern, with `legacy_id` populated when migrating a numbered legacy row)
- **Preserve task headings verbatim** — honours § Heading-preservation rule (above, under `## Step 2: Determine Operation`). Codebase realignment from Agent 1 may suggest *file-path* updates (which are fine) but never *heading text* changes.
- **PROGRESS-LOG.md is regenerated, not hand-authored.** Catchup MUST regenerate `PROGRESS-LOG.md` by running `tomlctl flow render-progress-log --slug <slug>` — NOT by hand-authoring D/DF-numbered markdown. After back-filled entries (deviations, deferrals, completions) are appended to `<record>`, append exactly **one `type=checkpoint` entry** tagging the restructure (`summary` should describe the catchup scope: research updates, structural changes, etc.). Then run `tomlctl flow render-progress-log --slug <slug>`.

After all writes, update `context.toml`: set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim. Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

**Write all files immediately in the same response** — do not pause for confirmation. Agent results are in context now and will be lost to compaction if you wait.

**Phase 3: Catchup summary** — After writing all files, output:

```
## Catchup Summary — [plan name]

**Plan age**: [last revised date] → [today]
**Codebase drift**: [summary of major structural changes]

### Status Changes
- [N] items newly marked as complete
- [N] items invalidated or need user decision
- [N] items unchanged and still actionable

### Research Updates
- [N] technology notes refreshed
- [N] items had stale/outdated guidance replaced
- Key changes: [brief list of the most impactful research updates]

### New Deviations Recorded
- E{n} (`type=deviation`, optional `legacy_id = D{n}` if migrated from a legacy hand-authored row): ...

### Items Needing User Decision
- [item] — [why it needs a decision: conflicting approaches, obsolete requirement, etc.]

### Recommended Next Steps
1. Review the items needing decision
2. Run `/review-plan` to validate the refreshed plan
3. Begin implementation
```

#### `snapshot` — Progress summary

Generate a compact progress summary suitable for standup notes, PR descriptions, or status updates:
- What was completed since last update (read from `<record>` `type=task-completion` entries since the prior `type=checkpoint` or `last_updated`)
- What deviated and why (read `type=deviation` entries)
- What's next (prioritized remaining plan items)
- Any blockers or deferred items (read `type=deferral` entries)

`snapshot` is **read-only**: it emits nothing to disk. The most recent render of `PROGRESS-LOG.md` already reflects the log state because every mutating op (`status`, `complete`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`) re-renders on every append, and `snapshot` is only invoked between mutations. Running `tomlctl flow render-progress-log --slug <slug>` here would be redundant at best and would break the "does not append entries" / "no filesystem writes" invariant at worst (use `--stdout` if you ever want a fresh render printed without touching disk). `snapshot` returns a summary of the current log state to the caller for inspection only.

#### `migrate` — Back-fill execution-record.toml from a legacy hand-authored `PROGRESS-LOG.md`

One-shot, opt-in operation. User invokes `/plan-update <plan> migrate`. Reads the existing `PROGRESS-LOG.md` in the flow directory and translates each row into an append-only E-entry in `<record>`. After back-fill, runs `tomlctl flow render-progress-log --slug <slug>` so the on-disk `PROGRESS-LOG.md` is regenerated from the now-populated log (the legacy hand-authored content is replaced by the deterministic render). Leaves `[tasks].in_progress` untouched (honours § flow-context field responsibilities).

##### Per-section translation rules

For each row in the legacy `PROGRESS-LOG.md` tables:

- **Deviations table** — every row whose ID column starts with `D<n>` becomes a `type=deviation` entry with `legacy_id = "D<n>"`. Best-effort fill: `task_ref` (slug from the row's "Item" / affected-task column), `original_intent` (from the row's description or rationale columns), `rationale` (from the row's "Rationale" column), `commits` (from the row's "Commit" column, single-element array). `summary` is the row's deviation description.
- **Deferrals table** — every row whose ID column starts with `DF<n>` becomes a `type=deferral` entry with `legacy_id = "DF<n>"`. Best-effort fill: `task_ref` (slug from the "Item" / "Deferred From" column), `reason` (from "Reason"), `reevaluate_when` (from "Re-evaluate When"). `summary` is the row's item description.
- **Completed Items table** — every row becomes a `type=task-completion` entry with `status = "done"`. Best-effort fill: `task_ref` (slug derived from the "Item" column heading text), `files` (from the "Files" column if present, else `[]`), `commits` (from the "Commit" column, single-element array if present). Source rows have NO D/DF prefix, so no `legacy_id` is set on these.
- **Session Log table** — no-op. Session-Log rows are rederived from the log at render time; back-filling them would duplicate state.

Mint each `id` via `tomlctl items next-id <record> --prefix E` so E-numbers stay monotonic across the back-fill.

##### Idempotency

Re-running `migrate` MUST NOT duplicate entries. Before appending each row:

1. **For deviations and deferrals (rows with D/DF prefix):** scan the existing log via `tomlctl items list <record> --where legacy_id=<D|DF><n> --verify-integrity` (or `--pluck legacy_id --verify-integrity` and check membership). If a matching `legacy_id` is already present, skip the row.
2. **For completed-items rows (no `legacy_id`):** dedupe by `task_ref` slug — query `tomlctl items list <record> --where type=task-completion --pluck task_ref --verify-integrity` and skip the row if its derived slug is already present.

Apply each authorised append using the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block. After all back-fills complete, run `tomlctl flow render-progress-log --slug <slug>` to regenerate `PROGRESS-LOG.md` and update `context.toml` (set `updated` to today, derive `[tasks].completed` per Task 6's recipe in Step 3, preserve `created` verbatim).

### Regenerating PROGRESS-LOG.md

`PROGRESS-LOG.md` is a DERIVED artifact — never hand-authored, never carries a `.sha256` sidecar. Regenerate it deterministically by running:

```bash
tomlctl flow render-progress-log --slug <slug>
```

This rebuilds `.claude/flows/<slug>/PROGRESS-LOG.md` as a pure function of `execution-record.toml` + the flow title (the `# Plan:` header reached via `context.toml`→`plan_path`): the marker line, the four tables (Completed Items / Deviations / Deferrals / Session Log — each with `(none)` empty-state rows), and a trailing newline. Pass `--stdout` to print instead of writing, or `--verify-integrity` to check the record's `.sha256` before rendering. The canonical execution-record schema this render reads from lives in the `flow-contract-execution-record-schema` skill (invoked under `## Execution Record Schema` above); the per-operation bodies' mentions of "the canonical two-call heredoc pattern from the `## Execution Record Schema` shared block" point at that contract.

## Step 3: Apply Updates

After determining what needs to change:

1. **Append entries to `<record>`** — for any op that mutates plan state (`status`, `deviation`, `defer`, `reconcile`, `reformat`, `catchup`, `migrate`), use the canonical heredoc-stdin two-call pattern from the `## Execution Record Schema` shared block. Never tempfile-stage payloads. Never edit `PROGRESS-LOG.md` by hand — it is regenerated.
2. **Regenerate `PROGRESS-LOG.md`** by running `tomlctl flow render-progress-log --slug <slug>` (see above) as the last step of every mutating op. The file's first line is the literal `<!-- Generated from execution-record.toml. Do not edit by hand. -->` marker.
3. **Update the outline** if completion markers or wave status changed.
4. **Do NOT update detail documents** unless a deviation fundamentally changes the implementation approach described there.
5. **Always update the "Last updated" date** on the outline (and any other actively edited plan file). `PROGRESS-LOG.md` does not carry a separate "Last updated" line — its content is a pure function of `<record>`'s `last_updated` field.
6. **Update the resolved flow's `context.toml`** at `.claude/flows/<slug>/context.toml`. This file is always touched whenever `plan-update` runs an operation that changes plan state (`status`, `complete`, `reconcile`, `defer`, `deviation`, `reformat`, `catchup`, `migrate`). Rules:
   - **Preserve `created` verbatim.** Never regenerate it.
   - Set `updated` to today's ISO 8601 date on every write. **Date validation**: before writing `updated` (here) or `last_updated` (on `<record>`), verify `<today> >= existing_value` and `<today> <= existing_value + 30 days` (upper bound allows sane timezone drift but rejects wild clock skew). On violation, prompt the user via `AskUserQuestion` with the observed delta and ask whether to proceed with the machine's clock value, use the existing stored value, or abort. Do not write silently on any of the three error cases.
   - Write `[tasks].total` from the plan-document item count (unchanged behaviour: plan-document-driven).
   - **Derive `[tasks].completed` from `<record>` on every write.** See § Execution Record Schema → `[tasks].completed` derivation (above) for the canonical pipeline. **Precondition**: verify `<record>` exists (`Test-Path <record>` / `[ -e <record> ]`) before running the derivation pipeline. A missing record is normally already handled — this op's append step auto-creates it (recognised-flow-file skeleton: `schema_version = 1` + `last_updated = <today>`) on the first `tomlctl items add` / `set`, so by derivation time the log exists. Only halt and surface the error if `[artifacts].execution_record` is genuinely unresolvable (both `[artifacts]` and the file absent) — do NOT let the pipe silently emit 0 and overwrite a valid prior `[tasks].completed`.
   - **`[tasks].in_progress` rule**: this field is written **only by `/implement` during live execution** (when it picks up a task and when it finishes one). Every `/plan-update` op MUST leave `[tasks].in_progress` untouched — read it once if you need to display it, but do not write it back. (The literal phrase appears throughout the per-op bodies above as a regression guard.)
   - Write `status` as one of `draft`, `in-progress`, `review`, `complete` — use `review` when every item is done or all remainders are deferred (the new default for finished implementation), or when a plan enters a review phase between rounds. Only the explicit `complete` op (see Operations → `complete`) may write `status = complete`; `status`, `defer`, and `reconcile` MUST NOT. If `status` changes value, append a `type=status-transition` entry per the reconciler contract.
   - `reconcile` may refine `scope`; other operations leave `scope` alone.
   - If `[artifacts]` is absent in the existing file, compute from `slug` and write it back. The execution-record needs no manual bootstrap per the `## Flow Context` `[artifacts]` rule: if `[artifacts].execution_record` points at a path that does not yet exist, the first `tomlctl items add` / `set` auto-creates it with the recognised-flow-file skeleton (`schema_version = 1` + `last_updated = <today>`) and its `.sha256` sidecar in the same transaction — just append.
   - Preserve key order. Do not introduce inline comments.
7. Present a summary of changes made to `<record>`, the rendered `PROGRESS-LOG.md`, and the flow's `context.toml`.

## Important Constraints

- **Propose, don't assume** — When marking items as complete or recording deviations, show the evidence and let the user confirm before committing plan changes. The exception is `status` updates with clear-cut evidence (file exists, test passes).
- **Deviations capture design-level differences, not typos** — Don't create `type=deviation` entries for minor implementation details like variable naming. Deviations should reflect meaningful departures from the planned approach.
- **Plans should remain human-readable** — The agent is a maintainer, not the owner. Don't restructure the plan format or add machine-only metadata. Note that `PROGRESS-LOG.md` is the one exception: it is regenerated from `<record>` and SHOULD NOT be hand-edited (its first line warns the reader).
- **Append-only log; rendered view is regenerated** — `<record>` is append-only (entries are never mutated; corrections append a new entry with `supersedes_entry`). `PROGRESS-LOG.md` is a deterministic render of `<record>` and is regenerated in full on every mutating op by running `tomlctl flow render-progress-log --slug <slug>`. The plan documents themselves (outline, detail docs, RESEARCH-NOTES.md) continue to be edited in place — never truncated and rewritten — outside of the explicit `reformat` / `catchup` ops.
- **Separate commits** — Plan updates should be committed separately from code changes unless the deviation is inherent to the implementation (e.g., a plan said "add column X" but you added "column Y" instead — that code + plan update belongs together).
- **Supersession via `supersedes_entry`** — When recording a deviation that supersedes an earlier one, set `supersedes_entry = "E<n>"` on the new entry (pointing at the prior entry's `id`). The render routine surfaces the latest entry per supersession chain; older entries remain in the log for audit. There is no separate "Superseded by" backlink — it is implied by the forward pointer.
- **Concrete re-evaluation triggers** — Deferral `reevaluate_when` values must be specific and observable ("when X happens"), not vague ("when we have time").
