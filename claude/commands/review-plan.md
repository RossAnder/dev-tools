---
description: Review an implementation plan for feasibility, completeness, risks, and agent-executability
argument-hint: [path to plan file or directory]
---

# /review-plan — plan review across four lenses

> Skim-readable orchestrator. Full contract bodies load on demand via skill invocations.

Reviews an implementation plan document against the actual codebase: validates the plan's assumptions, scope completeness, task executability, and dependency ordering. Works with any plan format — structured work packages, wave outlines, task lists, or prose; agents adapt to whatever they encounter. Findings persist to the flow's `plan-review-findings.toml` keyed by stable `P{n}` IDs; re-runs increment `round` and dedup against prior open items.

> **Agent count**: `/review-plan` uses 4 lens-agents (Feasibility, Completeness, Executability, Risk) — distinct from `/review`'s 5 code-review lenses, because plan review and code review answer different questions.

> **Effort**: Requires `xhigh` or `max` — lower effort may reduce agent spawning and tool usage.

## Step 0: Pre-flight (flow resolution + doctor)

Invoke the `flow-contract-flow-context` skill to load the flow-bootstrap envelope contract (input/output shapes, `envelope.ok` gating, `envelope.resolved.*` and `envelope.doctor.*` binding rules, no-flow fallback, doctor-fail handling, staleness reconciliation, status vocabulary, slug derivation, canonical artifacts, and the mandatory bootstrap-summary console line).

Build the input envelope and dispatch `flow-bootstrap`:

```bash
tomlctl flow envelope build \
  --command review-plan \
  --branch "$(git branch --show-current)" \
  --worktree "$(git rev-parse --show-toplevel)" \
  --cwd "$(pwd)"
```

On detached HEAD, omit `--branch` so the envelope records `branch:null`. Pass `--flow-override <slug>` when the user supplied `--flow`, and `--path-arg <p>` once per `$ARGUMENTS` path token. Dispatch via the Task tool with `subagent_type: "flow-bootstrap"` and the printed JSON as the prompt. Gate on `envelope.ok`; bind `slug`, `context_path`, `artifacts.*` (incl. `plan_review_findings`), and `doctor.ok` for downstream phases. Emit the bootstrap-summary line before any other action.

## Step 0.5: First-use `plansDirectory` prompt (per-carrier)

Invoke the `flow-contract-plansdirectory-prompt` skill to load the first-use prompt contract (gate on `envelope.plans_directory == null`, option-list construction, single-select AUQ ordering, headless empty-answer in-memory binding, `Don't ask again` sentinel arbitration, free-text follow-up, persist-via-`tomlctl json set`, and downstream binding). The wording is shared verbatim across `/plan-new`, `/plan-update`, and `/review-plan`.

# Plan Review

## Step 1: Load the Plan

**Reason thoroughly through plan analysis** before dispatching agents. Resolve the plan to review: (1) if `$ARGUMENTS` is a file path, read it; (2) if a directory, treat as a **multi-file plan** — read all markdown files, classify each by role (outline/master = primary; numbered detail docs = actionable tasks; progress/status = completion + deviation context; diagrams/supporting = reference), build a document map, share it with all agents; (3) if empty, locate the active plan in order: just-produced-in-conversation → Step-0 resolved flow's `plan_path` (via `tomlctl get <context_path> plan_path`; flag prominently when `envelope.resolved.stale == true`, >14 days) → recently-modified file in the plans directory (ask if multiple) → ask the user. Read the full content of every in-scope document.

## Step 2: Launch Parallel Review Agents

Launch **all four** review agents in a single response message (concurrent execution mandatory) via the Agent tool with `subagent_type: "research-deep"`. `research-deep` across all four is non-negotiable — plan critique is pure judgement and the fetch-and-summarise contract produces superficial findings that miss real defects; **do NOT reduce the count or downgrade any agent to `research-lite`**. Each agent reads the plan in full, explores the actual codebase to validate the plan's claims (read referenced files, search assumed patterns, verify paths and line numbers), and returns ≥ 3 findings with references to specific plan sections.

The four lenses:
- **Agent 1 — Feasibility, Codebase Alignment & Dependencies**: do referenced files/classes/methods/paths exist and look as the plan assumes (summarise any drift)? Are changes feasible given current architecture, and APIs/versions current? Are task/phase dependencies correct, with no hidden ordering hazards, broken-state failures, or parallel tasks modifying the same file? Broadest scope — if >10 findings, prioritise implementation-failure / data-loss items and merge related ones.
- **Agent 2 — Completeness & Scope**: affected-but-unmentioned files/components/consumers, missing tests, config/migration/build changes, cross-cutting concerns (logging, error handling, authz, cache invalidation), and same-pattern code elsewhere needing the same treatment. Search for usages and dependents of everything the plan touches.
- **Agent 3 — Agent-Executability & Clarity**: clear imperative actions, exact files named, verifiable acceptance criteria, right-sized tasks, no executor-time architectural decisions, parallelisable with no file overlap. Suggest restructuring if prose-format.
- **Agent 4 — Risk & External Validity**: use Context7 to verify API signatures/parameters/options against versions in use; WebSearch for deprecations/advisories/breaking changes; known pitfalls, realistic scope/effort, adequate rollback, and unaddressed performance/security/back-compat risks.

## Step 2.5: Vet agent output (orchestrator)

Invoke the `flow-contract-vet-research` skill to load the universal vet-pass procedure (triage by source+evidence-grade, `ESCALATE-TO-DEEP` honouring, drop-low-confidence rule, spot-check sampling, drop/downgrade-with-rationale, the canonical `[[vet_events]]` append heredoc, the mandatory `vet: Agent-{n} (<lens>) — N sampled, M dropped, K downgraded` console line, and the >30% systemic-failure re-dispatch rule).

**Per-carrier sample size**: spot-check ≥ 3 findings per agent (or all if fewer). Lens names: `feasibility`, `completeness`, `executability`, `risk`. **Verify every "stale reference" / "file does not exist" / "API has changed" claim BEFORE sampling** (file-does-not-exist → `ls`/Glob; API-changed → re-query Context7; signature mismatch → Read at cited line); drop any claim whose verification fails, with the verification evidence as the `[[vet_events]]` `rationale`. This verification is the highest-value step for plan critique — a reviewer that flags non-existent stale references is worse than no reviewer.

## Step 3: Consolidate Results

**Reason thoroughly through consolidation.** Cross-reference all surviving (post-vet) findings against the plan, resolve conflicting assessments, deduplicate across agents, and synthesise a single consolidated report. For every critical issue, include what the agent found in the codebase that contradicts the plan. An empty review is valid — a well-written plan may have no issues. The report header is `## Plan Review: [plan name/path]` followed by **Plan scope**, **Plan age** (flag if >14 days), and **Overall assessment** (Ready to execute | Needs revision | Major gaps), then severity-grouped sections (Critical / Warnings / Suggestions, each entry `[plan section/task] (area) Description`), a **Stale References** section (plan-assumes-vs-codebase-shows per item, or "All references verified current."), and an **Executability Assessment** (file coverage / dependency graph / parallel safety / acceptance criteria / stale references).

## Step 3.5: Persist Findings

After Step 3 and before Step 4, persist findings to the flow's `plan-review-findings.toml` so subsequent runs dedup and Step 4 has a single source of truth.

1. Resolve `plan_review_findings_path` from `envelope.resolved.artifacts.plan_review_findings`; for legacy flows derive `.claude/flows/<slug>/plan-review-findings.toml` per the `flow-contract-flow-context` self-healing contract and write it back to `[artifacts]` on the next TOML write.
2. If the file does not exist, bootstrap it with two lines: `schema_version = 1` / `last_updated = <today>` (no atomic dance — `/review-plan` is the sole writer).
3. Mint monotonic IDs via `tomlctl items next-id <path> --prefix P`.
4. Batch-write: `tomlctl items add-many <path> --defaults-json '{"review_round":<n>, "status":"open"}' --ndjson -`.
5. `tomlctl set <path> last_updated <today>` and `tomlctl set <path> round <n>`, where `<n>` is the current review round (1 on first run; increment per Re-run dedup).

### Artifact Schema: `plan-review-findings.toml`

Required fields: `id` (`P{n}` monotonic), `review_round` (int), `severity` ∈ {`critical`, `warning`, `suggestion`}, `category` ∈ {`feasibility`, `completeness`, `executability`, `risk`}, `plan_section` (markdown heading anchor, copied verbatim from the plan), `summary` (one line), `status` ∈ {`open`, `merged`, `discarded`}. Optional: `description`, `anchor_old` (exact substring already in the plan under `plan_section`), `anchor_new` (replacement). **The `anchor_old` + `anchor_new` pair is the mechanical merge contract — BOTH required for auto-merge; anchor-less findings are advisory-only and skipped by the merger.** Schema callouts: `tomlctl items find-duplicates` / `orphans` hardcode the review/optimise schema and MUST NOT run against this file; `next-id --prefix P`, `items list`, `add-many --ndjson -`, and `apply --ops -` are the supported subcommands.

## Step 4: Auto-Merge Offer (end of turn)

Let the user opt-in to a mechanical merge of selected-severity findings into a `.revised.md` sibling of the plan, then accept / keep both / discard.

1. **Count findings by severity.** If zero total, output `No findings — plan is clean.` and end.
2. **`AskUserQuestion` (Q1)** — `multiSelect` over `[Critical, Warning, Suggestion]`, default `[Critical, Warning]`. **Empty-answer rule**: if the response comes back empty (running in `acceptEdits` / skill-hosted / headless mode, per Claude Code issues [#29618](https://github.com/anthropics/claude-code/issues/29618) and [#29547](https://github.com/anthropics/claude-code/issues/29547)), treat as "zero selected" — **SKIP the merge step entirely and persist findings only. Do NOT proceed to Q2.**
3. **If zero severities selected** → persist only, no merge. Output: `Findings persisted; auto-merge skipped. Re-run interactively to merge.`
4. **Filter selected-severity findings** to those with **both `anchor_old` AND `anchor_new`**; advisory-only findings are skipped silently.
5. **Conflict detection** — group filtered findings by `plan_section`; if >1 in a group has non-empty `anchor_old`, emit `[conflict: plan_section="..."; findings=P3, P7] — manual merge required` and skip that whole group (other groups still apply).
6. **Mechanical merge** — for each survivor, locate `anchor_old` as a substring under its `plan_section` heading; if found exactly once, replace with `anchor_new`, else log `[merge-failed: P{n} — anchor_old not found uniquely in section "..."]` and skip. Apply in P-id monotonic order.
7. **Materialise** via `Write` to a sibling: replace the plan's trailing `.md` with `.revised.md` (do not append). For multi-file plans (`plan_path` → `<dir>/00-outline.md`), materialise only `<outline-dir>/00-outline.revised.md` — detail files are not rewritten by v1.
8. **Pre-existing sibling**: if `<plan>.revised.md` already exists, rename it to `<plan>.revised.prev.md` first (overwriting any older one). Cheap rollback.
9. **Console summary**: `N applied, K conflicts skipped, M merge-failures`; list `plan_section → summary` per applied finding.
10. **`AskUserQuestion` (Q2)** — `[Accept, Keep both, Discard]`. **Default `Keep both`** (NOT `Accept` — `Accept` is irreversible, and default-Accept + auto-mode empty-answer = silent overwrite). **Empty-answer rule**: empty → treat as `Keep both`.
11. **Apply chosen action**: **Accept** → `Write` revised content over the original, keep `<plan>.revised.md` one cycle, transition matching findings to `status = "merged"` via `tomlctl items apply <path> --ops -`. **Keep both** → no mutation; findings stay `open`. **Discard** → delete `<plan>.revised.md`, transition findings to `discarded`. The prior run's `<plan>.revised.prev.md` is deleted on the NEXT run's step 8 (one-cycle retention).
12. `tomlctl set <path> last_updated <today>`.

### Re-run dedup (subsequent invocations)

Subsequent runs increment `round`: read via `tomlctl get <path> round`, increment, write back via `tomlctl set <path> round <n>`. `merged` / `discarded` findings are ignored by lens-agents; `open` items from prior rounds are passed as prior context so agents avoid re-raising. Dedup key `(plan_section, anchor_old)` — a new finding matching an existing `open` item MUST NOT be added; update the existing item if severity/category changed, otherwise skip.
