---
description: Review code for issues, DRY violations, idiomatic patterns, project structure, security, and completeness
argument-hint: [file paths, directories, feature name, or empty for recent changes]
---

# /review — code review across five lenses

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Reviews code through five (or six) parallel lenses — Quality, Security, Architecture, Completeness, Testability (plus conditional Package Quality for `claude/commands/` and `claude/skills/` paths). Findings persist to a TOML ledger keyed by stable `R{n}` IDs; re-runs dedupe, merge, escalate chronic items, and surface regressions. Targeted mode takes paths/feature names as `$ARGUMENTS`; with no args, scope is derived from recent git changes.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, and the mandatory bootstrap-summary console line).

Build the input envelope:

```bash
tomlctl flow envelope build \
  --command review \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)" \
  --staleness-threshold 7d
```

The block above is complete and copy-pasteable as-is — do NOT look up `--help`; `/review` lazily creates its ledger, so no `--require-artifact` flag is needed, and `--staleness-threshold 7d` is the default, passed explicitly for clarity. On detached HEAD, `git branch --show-current` prints an empty string; omit the `--branch` flag in that case so the envelope records `branch:null` rather than `branch:""`.

Pass `--flow-override <slug>` to the envelope build when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch `flow-bootstrap` via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*`, `doctor.ok`, and `resolved.stale` for downstream phases. Emit the bootstrap-summary line before any other action. When staleness fires for `/review`, invoke the `plan-update` skill with literal arg `reconcile` before continuing.

## Step 1: Determine Scope and Load Prior Findings

Identify in-scope files from `$ARGUMENTS` (paths / globs / feature names) or — when empty — from git diff against the merge-base (feature branch) or `HEAD~1` (main). Before treating a token as a path arg, check it against `tomlctl flow list`: if a `$ARGUMENTS` path token exactly matches a registered flow slug, treat it as `--flow-override` (route it to the Step-0 envelope build) rather than passing it as a `--path-arg`. This prevents a bare flow-slug argument from being misread as a path while the resolver silently auto-picks the wrong flow via active-latest. Enforce the scope-input safety invariant: reject `..` traversal, refuse absolute paths outside the worktree, cap glob breadth ≤ 200 matches. Classify each file by area (backend / API / frontend / infra / config / test). When scope > 10 files, delegate per-file classification to a `subagent_type: "Explore"` agent (`thoroughness: "quick"`) and keep only the ≤ 600-word summary table. Dispatch it one-shot — **never pass `name:`**: a named spawn becomes a teammate with no return channel, and the built-in `Explore` carries no teammate-delivery instruction, so the table is lost and you get only a "Teammate @x finished" notification. Treat that as a failed dispatch and re-dispatch unnamed rather than pumping it with `SendMessage`.

Invoke the `flow-contract-ledger-schema` skill to load the canonical ledger schema (field set, disposition vocabulary, dedup rules, read/write contract via `tomlctl items` / `array-append`, key-order convention, rollback / vet event logs). Resolve the ledger path: `envelope.resolved.artifacts.review_ledger` when a flow resolved, else `.claude/reviews/<scope>.toml` (derive `<scope>` from path arg / feature name / branch / `recent`). Load via `tomlctl items list <ledger> --status open --verify-integrity` (or treat as first review when the file is missing). Extract items whose `file` overlaps current scope as the **prior findings context** — agents use this to skip already-resolved items, flag regressions, and avoid re-reporting open items.

Invoke the `backlog-capture` skill for the repo-scoped capture store's discipline, then load its open rows alongside the ledger's (drop `--area-prefix` when the scope is not a single directory):

```bash
tomlctl backlog list --open --area-prefix <scope-dir>
```

Hand each returned row's `summary` and `context` to the lens agents as prior context: a row is a known repo-scoped issue plus how to work around it, so an agent tripping over the same thing does not spend the round rediscovering it.

Invoke the `flow-contract-ledger-disposition-sweep` skill to load the orphan-surfacing and deferred-reopen sweep contracts. Run both before the agent dispatch: orphan-surface read-only via `tomlctl items orphans <ledger>`; deferred-reopen is **a user-engagement gate — the autonomy directive does not apply** (every reopen passes through the per-item prompt; non-interactive invocations surface candidates only and never mutate).

## Step 2: Launch Parallel Review Agents

Invoke the `flow-contract-task-visibility` skill for the run-scoped task-surface contract (view-not-store rule, subject prefix with lowercase `<ref>`, `activeForm`, lifecycle, granularity floor, silent degradation). Call `TaskCreate` once per lens (5 standard + conditional package-quality, OR 1 task for the small-diff shortcut at ≤ 3 files), subject prefix `<slug> /review · <lens>` (lens lowercased: `quality`, `security`, `architecture`, `completeness`, `testability`, `package-quality`); mint the whole set before launching, and `TaskUpdate` each `pending → in_progress` on launch and `→ completed` only after its Step-2.5 vet. Never mint a task per finding — the ledger owns per-item state. Launch **all** agents in a single response message via the Agent tool — concurrent execution is mandatory. Mixed-tier dispatch is non-negotiable: Agents 1 (Quality) and 3 (Architecture) use `subagent_type: "research-deep"` (judgement-licensed); Agents 2 (Security), 4 (Completeness), 5 (Testability), 6 (Package Quality, conditional) use `subagent_type: "research-lite"`. The conditional 6th lens fires when any in-scope path begins with `claude/commands/` or `claude/skills/`. Each agent receives the file list, classification, CLAUDE.md excerpts, and prior-findings context; each returns **up to 20 findings, target 15 — a ceiling and a target, never a quota**. Returning zero is a correct and expected result when a lens finds nothing; say in one line what you examined and ruled out. Thoroughness is measured by coverage, not count: name the files read in full and the checks run, and never emit a finding you could not defend at its stated evidence-grade. A padded finding costs more than a missed one — it is triaged, persisted, and re-raised every round. Findings carry the ledger-schema field set (no `id` / `first_flagged` / `rounds` / `status` — those are assigned in Step 3). Security caps hard at 5. Agent 5 owns the testing lens; Agent 4 defers tests-not-written observations to Agent 5. Agent 3 owns the `db` category. Do NOT silently downgrade the judgement-licensed agents to `research-lite`.

## Step 2.5: Vet agent output (orchestrator)

Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule).

**Per-carrier sample sizes**: spot-check ≥ 5 findings per `research-lite` agent (2, 4, 5, 6) and ≥ 3 per `research-deep` agent (1, 3), or all if the agent returned fewer. Lens names for the console line: `quality`, `security`, `architecture`, `completeness`, `testability`, `package-quality`. **Vet pass is NOT optional** — the Step 1 idempotency guards cannot retroactively remove a fabricated finding once persisted.

## Interim checkpoint

After Step 2.5 vetting, persist surviving items (plus any reopened items from the Step 1 deferred-reopen sweep) to the ledger in a single atomic `tomlctl items apply --ops -` call so an interrupted run doesn't lose agent output. Defer `last_updated` stamping and `rounds` increments on prior open items to Step 3. Skip the checkpoint when no transitions are pending — gate with `tomlctl items list <ledger> --status open --count --raw`.

## Step 3: Consolidate and Persist

Cross-reference all agent results; apply the dedup, merge, and regression rules per the ledger-schema skill (same `file` AND (same non-empty `symbol` OR exact `summary` match)). Mint new `R{n}` IDs as `max(existing) + 1`; never renumber. Regressions against `fixed` items get a new ID with `related = ["<old id>"]`. Open-item matches reuse the existing ID and increment `rounds`. Chronic items (`rounds >= 3` post-merge) MUST be called out in a dedicated escalation callout; `rounds >= 5` items get a concrete defer-or-wontfix recommendation in the callout.

Render the merged ledger state as severity-grouped markdown tables (Critical / Warnings / Suggestions) plus Still-Open / Resolved-Since / Regressions sub-groupings — **rendered inline in console output only, never persisted**. Persist the merged state via the parse-rewrite two-call pattern from the ledger-schema skill: one batched `tomlctl items add-many --ndjson -` (pure-add) or `tomlctl items apply --ops -` (mixed) for all mutations, followed by `tomlctl set <ledger> last_updated <YYYY-MM-DD>`. After the report, prompt the user with a concrete `/review-apply R{a},R{b}` invocation for the lowest-hanging quick wins, plus disposition syntax (`defer R{n} — reason — trigger`, `wontfix R{n} — rationale`).

**Observations outside the ledger's scope.** An agent finding whose `file` lies outside the review scope, and a cross-cutting observation that is not a finding against an in-scope file, are neither: do not mint an `R{n}` for them, and do not force a terminal disposition onto an item the ledger never owned. Capture each in the backlog instead — probe first, passing `check` the same `--kind` and `--area` the mint will use:

```bash
tomlctl backlog check --summary "<summary>" --kind <kind> --area <repo-relative-path>
```

then mint the ones the verdict allows, with provenance and a workaround:

```bash
tomlctl backlog add --summary "<summary>" --kind <kind> --area <repo-relative-path> --context "<how to work around it, or what the next reader should do first>" --origin review --flow <slug>
```

Sub-agents never write the store; the orchestrator mints from their surfaced candidates and lists the resulting ids on a `Backlog` line in the console report.

## Step 4: Handle Dispositions (if user responds)

**This is a user-engagement gate — the autonomy directive does not apply.** Apply dispositions only when the user replies with conversational commands in the same turn. Recognise `defer R{n} — reason — trigger` (set `status = "deferred"` + `defer_reason` + `defer_trigger`), `wontfix R{n} — rationale` (set `status = "wontfix"` + `wontfix_rationale`), and `fix R{n}` (route to `/implement` with the item's `file` / `line` / `summary` / `description` — **do NOT mutate status here**; the `fixed` transition happens when the fix actually lands).

Apply all dispositions in the reply as a single atomic batch via `tomlctl items apply <ledger> --ops -` (one heredoc, even for a single disposition — stdin form sidesteps shell-quoting hazards in user-supplied reason/trigger/rationale strings). Follow with `tomlctl set <ledger> last_updated <YYYY-MM-DD>`. Suggest `/review-apply` for the next round of automated fixes; `/implement` remains available for plan-task-driven work.
