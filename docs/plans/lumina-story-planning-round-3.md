# Plan: Lumina Story Planning — Round 3 (Research depth, plan-story enforcement, tier composer)

**Plan path**: `docs/plans/lumina-story-planning-round-3.md`
**Created**: 2026-05-26
**Status**: Draft
**Depends on**: Round-2 (`docs/plans/lumina-story-planning-round-2.md`) shipped (Phases 1-4 complete). Round-3 amends three round-2 skills (plan-story, set-task-spec, wire-task-deps, vet-research) and the backend types they consume.

## Context

Round-2 closed the structural gaps surfaced by the /plan-new comparison (orchestrator, vet, critique, task decomposition, backend gaps). A follow-up gap audit identified three remaining deficiencies against `/plan-new`:

- **Research depth (Phase 3 + Phase 5 parity)** — round-2's `/lumina:research-notes` is a sequential user-typed buffer; `/lumina:vet-research` triages what was already typed. `/plan-new` fires multi-agent parallel exploration (`flow-research-deep` × ≥4 lenses) in Phase 3, then runs a directed-research lap in Phase 5 to verify version pins / API claims after user decisions land. Lumina has neither.
- **Plan-story enforcement** — round-2's `/lumina:plan-story` is advisory (per R6: orchestration ≠ enforcement). Skipping a block silently bypasses prerequisites. The chained-runner UX needs hard phase gates with an explicit override-and-audit path; per-block invocation outside the runner stays unrestricted (R6 preserved).
- **Tier composer** — `Effort::{S,M,L}` (domain.rs:468) and `Complexity::{Low,Medium,High}` (domain.rs:482) exist with the right doc-intent ("drive batch sizing" / "model-tier assignment in the eventual composer"). The composer is unbuilt. `set_task_spec.dispatch` is a free-form `serde_json::Value` (mcp.rs:302) with no derivation rule and no consumer (round-3 renames this field to `tier`). Findings carry `severity` as untyped TEXT (migrations/0001_init.sql:108) — the closure-gate doesn't read it.

The end-state is `/lumina:plan-story` running a six-phase canonical sequence with hard gates, ingesting research from parallel-agent exploration, and producing a batch-scheduled dispatch plan that `/implement` (or a future `/lumina:run-batch`) can consume task-by-task with the correct model tier.

## Canonical lumina vocabulary

This round establishes the following terminology for lumina-internal use (the tomlctl flow ecosystem retains its own separate vocabulary):

| Term | Meaning | Example |
|------|---------|---------|
| **phase** | Plan-time / workflow-time logical grouping. The six canonical phases of `/lumina:plan-story` are Frame / Explore / Decide / Verify-design / Decompose / Closure. Also used as the top-level grouping in lumina implementation plans (replaces "wave"). | "Phase 2 — Research skills"; `Phase::{Frame, Explore, …}` enum |
| **batch** | Parallel-dispatch unit produced by a topological sort. One batch is a set of tasks that can be dispatched simultaneously after the previous batch's deps clear. Output of `compute_task_batches`. (Replaces "wave" when the meaning is "parallel-dispatch group".) | "Batch 1 (foundation, 3 tasks)"; `Batch = Vec<TierEntry>` |
| **sprint** | Execution-time concept (lumina UI / composer). The currently-active queue of tasks being dispatched. May span several phases or only part of a phase. NOT a plan-time term. | `<SprintComposer>` in the SPA; "ADD TO SPRINT" |
| **tier** | Model-tier value for an executor (`Tier::{Lite, Deep}`). The output of `compute_tier(effort, complexity, files_touched_count, cross_repo)`. Field name on tasks. (Replaces the free-form `dispatch` field; `dispatch` remains as the verb only.) | `tasks.tier`, `SetTaskSpecParams.tier: Option<Tier>` |
| **dispatch** | The verb — the act of starting agents. Also used as a verb-derived noun in compound phrases ("dispatch plan", "dispatch budget"). | `get_task_dispatch_plan` (read tool), "dispatch 4 agents" |
| **lens** | A research-exploration vocabulary axis. Canonical 5: `codebase / library / risk / completeness / domain`. Documented in CONVENTIONS.md §k.1. | `research_notes.lens = "library"` |
| **step** | Procedural micro-step inside a skill or phase. | "§b 5-step idempotency" |
| **round** | Re-run iteration counter (round 1, 2 of /lumina:vet-research; round 1, 2 of /review-plan). | `round = N` |
| **severity** | Finding-severity enum: `Severity::{Low, Medium, High, Critical}` (documented §k.2). | `findings.severity = "high"` |
| **effort** | Task batch-sizing enum: `Effort::{S, M, L}` (wire `s|m|l`). Already canonical. | `set_effort(id, Effort::M)` |
| **complexity** | Task model-tier-input enum: `Complexity::{Low, Medium, High}` (wire `low|medium|high`). Already canonical. | `set_complexity(id, Complexity::High)` |

**Skill role types** (informal taxonomy documented in CONVENTIONS.md §a):

| Role | Description |
|------|-------------|
| **writer** | Single-write skill — captures user content and writes one row/attribute (e.g. `problem-statement`, `risks`, `alternatives`). |
| **advisor** | Read-only, model-discoverable skill that recommends next action (e.g. `next-block`). |
| **runner** | Orchestrator skill that walks a sequence with gates (e.g. `plan-story`). |
| **vet** | Research-verification skill (e.g. `vet-research`). |
| **critique** | Audit skill that emits findings (e.g. `story-review`). |
| **composer** | Future role — reads tier+batch data and produces an execution dispatch plan (e.g. round-4 `/lumina:run-batch`). |

**Cross-system note**: The tomlctl flow ecosystem (`/plan-new`, `/implement`, `/review-plan`, `flow-contract-*`) uses its own severity vocab (`critical|warning|suggestion`), effort vocab (`trivial|small|medium`), and grouping vocab ("phases/waves" interchangeable). Lumina does NOT reconcile to that — the two systems serve different artefacts (markdown plan files vs SQLite work_items). Plan documents *about* lumina (this file, round-2) sit in the tomlctl flow ecosystem and follow lumina's vocab for prose, while the `plan-review-findings.toml` ledger they emit uses tomlctl-flow severity (critical/warning/suggestion).

## Scope

- **In scope**:
  - **Lumina backend**: migration 0006 (`work_items.tier TEXT CHECK (tier IN ('lite','deep') OR tier IS NULL)`; `findings.severity` typed via CHECK constraint mirroring the Severity enum); new domain types (`Tier`, `Severity` enum, `Phase` enum); new repo functions (`compute_tier`, `get_task_dispatch_plan`, `set_finding_severity_typed`, optional `set_finding_tier_hint`); typed `SetTaskSpecParams.tier: Option<Tier>` (was `Option<serde_json::Value>`); typed `add_finding.severity: Severity`; new MCP read tool `get_task_dispatch_plan`; new e2e tests; sqlx cache regen.
  - **Plugin skills**: 2 new SKILL.md (`research-explore`, `research-directed`); 4 amended SKILL.md (`plan-story` — six-phase enforcement + override-audit; `set-task-spec` — captures effort+complexity, computes tier; `wire-task-deps` — renders batch dispatch counts + agent budget; `vet-research` — parallelised spot-checks); CONVENTIONS.md new §k "Tier derivation rule" + new §l "Six-phase canonical sequence" (override-audit contract); mcp catalogue + README + plugin.json (0.2.0 → 0.3.0).
  - **Cross-references**: `lumina/CLAUDE.md` updated with new MCP surface; repo-root `CLAUDE.md` `## lumina` paragraph updated.
  - **End-to-end smoke test** validating six-phase enforcement on a real story + batch dispatch plan emission.
- **Out of scope**:
  - `/lumina:run-batch` — the batch executor itself is the bigger artefact-bridge problem (round-4 follow-up); round-3 only ships the *data* it would read.
  - Markdown render of a story to a `/plan-new`-shaped plan.md (round-4 follow-up).
  - SPA changes (api.ts schema additions for `tier` / typed severity) — the strict-zod gap is documented but lives with round-2's follow-up UI plan.
  - Migration of existing free-form `set_task_spec.tier` values — round-3 ships a forward-only typed schema; any pre-existing free-form values become validation errors on next write (the round-2 plan was DB-canonical so no production data exists yet).
  - Any change to `/plan-new` carrier itself.
- **Affected areas**:
  - `claude/plugins/lumina-story-blocks/**`
  - `lumina/**`
  - `CLAUDE.md`
- **Estimated file count**: ~18 unique files (1 migration + 4 lumina source files + 1 sqlx cache batch + 2 new SKILL.md + 4 amended SKILL.md + 1 CONVENTIONS.md + 3 plugin meta + 2 cross-ref CLAUDE.md). Within the 15-file ceiling once Phase 1 is excluded (the lumina/* edits are file-overlap-sequential within their phase).

## Exploration Notes

### Confirmed data-model state (from lumina source)

- `Effort::S | M | L` (`lumina/src/domain.rs:468`, snake_case wire `s|m|l`). Doc-comment: "drives batch sizing in the eventual composer. Distinct from `Complexity` (which drives model tier). NOTE the wire divergence: the serde/JSON wire form is lowercase `s|m|l`; the plan-doc `S/M/L` is a display-only convention." — **vocabulary already aligned with `/plan-new`; no extension needed on this axis.**
- `Complexity::Low | Medium | High` (`lumina/src/domain.rs:482`, snake_case wire). Doc-comment: "drives model-tier assignment in the eventual composer." — **also already aligned.**
- `set_task_spec` accepts `dispatch: Option<serde_json::Value>` (`lumina/src/mcp.rs:302`) — free-form, no derivation, no validation, no consumer. **This is what round-3 types.**
- `findings.severity TEXT` (`lumina/migrations/0001_init.sql:108`) — no CHECK constraint, no enum. `repo::add_finding` accepts it as a string. **Round-3 types this.**
- `set_effort` and `set_complexity` are task-scoped only (reject story; `repo.rs:3648-3667` tests). Round-3 keeps this constraint.
- `closure_gate` (migration 0003) — hard mode blocks `task → done` when an acceptance criterion is unchecked. Round-3 extends to block on open critical/high findings.

### /plan-new research-phase mechanics to mirror

- **Phase 3 (parallel exploration)**: `/plan-new` dispatches 4 default `flow-research-deep` lenses (codebase / library / risk / completeness) in a single message. Each agent reads in full + returns ≥3 findings with evidence-grade. The orchestrator runs a vet-pass per finding (sample, verify cited URLs / file:line / library versions). The full contract lives in `flow-contract-vet-research`.
- **Phase 5 (directed research)**: post-AskUserQuestion-decision lap. Each user decision that named a library / API / version triggers a targeted `flow-research` (mechanical) or `flow-research-deep` (judgement) verification. Drift findings are surfaced via the standard finding format.
- **Vet-pass spot-check parallelism**: `/plan-new` runs N vet samples per agent in parallel (up to 4) via concurrent Read / Grep / WebFetch calls. Round-2's `/lumina:vet-research` does these sequentially today.

### Six-phase canonical sequence (derivation)

| Phase | Blocks | Hard precondition before entry |
|-------|--------|--------------------------------|
| 1. Frame | problem-statement, user-interrogation | none (story exists) |
| 2. Explore | research-explore, vet-research, research-directed | `problem_statement_set == true` |
| 3. Decide | alternatives, approach, not-doing, edge-cases, risks | `accepted_research_count ≥ 1` AND `unresolved_questions == 0` |
| 4. Verify-design | verification-commands, acceptance-criteria, story-review | `has_approach == true` |
| 5. Decompose | decompose-tasks, set-task-spec, wire-task-deps | `acceptance_criteria_count ≥ 1` AND `verification_commands_set == true` |
| 6. Closure | closure-gate, relevance | all tasks have `effort` + `complexity` + `tier` set; zero open findings with severity ∈ {high, critical} |

All preconditions are computable from existing `get_story_readiness` booleans + one new derived field (`open_blocking_findings_count`) that round-3 adds.

### Tier derivation rule (proposed)

```
compute_tier(effort, complexity, files_touched_count, has_cross_repo):
    if complexity == High:          return Deep
    if effort == L:                 return Deep
    if files_touched_count > 3:     return Deep
    if has_cross_repo:              return Deep
    else:                           return Lite
```

Mirrors `/implement`'s dispatch heuristic (deep for cross-file refactors / security-sensitive / judgement; lite for mechanical fully-specified). The `> 3` ceiling matches the apply-flow 3-file-per-item cap (anything beyond is cross-file and warrants Opus).

## Research Notes

**Vet outcomes** (per `flow-contract-vet-research`):
- This is a follow-up plan grounded entirely in round-2's exploration + the lumina source already read in this conversation. No new external research lenses required; the `/plan-new` mechanics are documentary citations of existing contracts (`flow-contract-vet-research`, `flow-contract-plan-output-format`) that are already authoritative in this repo.
- Any new agent-time research for individual tasks happens via the round-2 `/lumina:vet-research` skill (now extended in T9) plus the new `/lumina:research-explore` in T7.

### Phase 3 findings — Research-phase architecture

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R30 | `/plan-new` Phase 3 dispatches all parallel-exploration agents in a single message; per-agent context is the prompt body alone (no shared scratchpad). Findings flow back through the orchestrator who runs vet-pass + dedupes. | `flow-contract-vet-research` body | high | `/lumina:research-explore` body must dispatch all N agents in one message (Agent tool with parallel `<invoke>` blocks). Each agent's prompt is self-contained — no inter-agent dependency. |
| R31 | `/plan-new` Phase 5 fires research per *decision*, not per *claim*. A decision (e.g. "use Pinia") triggers one agent to verify the library claim set; an undecided alternative triggers nothing. | `flow-contract-plan-output-format` `User Decisions` section | high | `/lumina:research-directed` reads `attributes.execution_strategy` + the answered `open_questions` rows; iterates decisions, not claims. |
| R32 | The existing `research_notes.lens` column accepts free-form strings (no enum). `/plan-new` uses `codebase`, `library`, `risk`, `completeness`, `domain` as the canonical five. | `lumina/migrations/0003_planning_and_decisions.sql` (research_notes table); `flow-contract-vet-research` lens vocabulary | high | Round-3 documents the canonical five-lens vocabulary in CONVENTIONS.md §k.1; does NOT migrate `lens` to an enum (low cardinality, free-form is fine, future lenses are additive). |
| R33 | Hard-gate enforcement with override-and-audit is the published pattern in HumanLayer's create_plan.md template (cited in round-2 R23). Each phase has automated + manual verification; phase transitions are gated. Override is allowed but recorded. | https://github.com/humanlayer/humanlayer/blob/main/.claude/commands/create_plan.md (round-2 vet ✓) | medium | Phase-gate override in `/lumina:plan-story` records `record_task_activity { entry_type: "execution", summary: "skip_override: <block>", origin: "plan" }` — the existing activity log is the audit trail; no new table needed. |
| R34 | The Lite/Deep model dispatch enum mirrors the existing `flow-implement-{lite,deep}` agent split documented in CLAUDE.md (Build & test section). The split criteria (`flow-contract-apply-vet-flow-implement-lite`) include: ≤2 files mechanical → lite; cross-file refactor / security / judgement → deep. | `flow-contract-apply-vet-flow-implement-lite` skill body | high | `compute_tier` rule should mirror these criteria. The `> 3` files threshold is a slight relaxation of the apply-flow 3-file cap, because a task naturally touches up to its own cap before becoming cross-file. |
| R35 | Per-agent token-budget caps for `flow-research-deep` (Opus, 1M context) are unconstrained; per-agent prompt depth in the `/plan-new` Phase 3 dispatch is ~700-1200 words for the lens prompt. Round-2's plan-review-finding P10 (R26 citation drift) is the canonical example of why agent prompts MUST cite source URLs verbatim. | round-2 plan + this conversation's review pass | medium | `/lumina:research-explore` per-lens prompt template MUST instruct the agent to cite URLs verbatim and grade evidence (high/medium/low) per the contract. Inline the citation discipline in the skill body. |

## User Decisions

> Recorded verbatim from this turn's `AskUserQuestion` responses.

**Q1: Where should this work land?**
> Answer: **New round-3 plan** (recommended option).

**Q2: Tier derivation — where should the rule live?**
> Answer: **Server-side typed enum + `compute_tier`** (recommended option). `Tier` enum + migration 0006 + `repo::compute_tier`. Composer reads via `get_task_dispatch_plan`. Single source of truth in the DB; UI and any agent consumer share the same derivation.

**Q3: How strict should plan-story phase enforcement be?**
> Answer: **Hard gates with override-and-audit** (recommended option). Run hidden when prereq fails; Skip with override is allowed but records an audit event surfaced by `/lumina:story-review`. Best balance of structure + user agency.

## Approach

### Architecture — three phases on a dependency spine that bridges to round-2

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 1 — Backend foundation                                            │
│  Migration 0006 → typed domain (Tier + Severity + Phase) →     │
│  repo (compute_tier / get_task_dispatch_plan; typed severity   │
│  on findings) → MCP (typed tier on SetTaskSpecParams; typed             │
│  severity on AddFindingParams; new read tool) → tests + sqlx regen      │
└─────────────────────────────────────────────────────────────────────────┘
                              │ blocks all Phase 2-3 skills
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 2 — Research skills (parallel, 3 agents)                          │
│  research-explore (NEW, multi-agent fan-out) | research-directed (NEW,  │
│  post-decision lap) | vet-research amendment (parallelise spot-checks)  │
└─────────────────────────────────────────────────────────────────────────┘
                              │ blocks Phase 3 plan-story rewrite (the
                              │ phase-2 entry in plan-story dispatches
                              │ research-explore → vet-research → directed)
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 3 — Enforcement + dispatch consumers                              │
│  plan-story rewrite (six-phase) → set-task-spec amendment (captures     │
│  effort+complexity, computes tier) → wire-task-deps amendment           │
│  (renders batch dispatch budget) → CONVENTIONS §k+§l → mcp catalogue +  │
│  README + plugin.json closure                                           │
└─────────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 4 — Integration / verification                                    │
│  Cross-ref CLAUDE.md updates; end-to-end smoke test                     │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key design decisions

1. **Typed `Tier` enum stored in a dedicated column, not in attributes JSON** (Q2). Migration 0006 adds `work_items.tier TEXT CHECK (tier IN ('lite','deep') OR tier IS NULL)`. The composer's read query (`get_task_dispatch_plan`) becomes a simple `SELECT id, effort, complexity, tier FROM work_items WHERE …` instead of `JSON_EXTRACT(attributes, '$.tier')`. Trade: one more migration; one more schema column. Rejected: keeping in attributes — slow cross-task queries + no validation. Rejected: client-side derivation in skill body — derivation drifts across skill versions.

2. **`compute_tier` is a pure function on existing inputs** — no I/O, no events. Lives in `repo.rs` (where it can be called from both MCP write tools and the upcoming composer) and is documented in CONVENTIONS.md §k. The rule is intentionally simple (`complexity=high OR effort=L OR files>3 OR cross-repo → deep`) so the SKILL.md authors transcribe rather than invent. Trade: no fancy per-domain calibration. Rejected: probabilistic / weighted score — premature optimisation; we have no calibration data yet.

3. **Findings `severity` becomes a typed enum** (`Severity::{Low,Medium,High,Critical}`) via migration 0006's CHECK constraint on `findings.severity`. The closure-gate's hard mode is extended to block on open critical/high findings; the existing AC-based gate continues to fire too. Mirrors round-2 T12's wording that critical/high finding gate forward progress. Trade: forward-only typing — existing rows (none in production yet) are validated on next write.

4. **Six-phase canonical sequence is encoded in `/lumina:plan-story`'s body, not the DB** (Q3). Phase entry preconditions are computed from existing `get_story_readiness` booleans + one new derived field (`open_blocking_findings_count`). Override is allowed via "Skip with override" but recorded via `record_task_activity { entry_type: "execution", origin: "plan", summary: "skip_override: <block>" }` so `/lumina:story-review` can surface it later. Trade: phase definitions are skill-author-maintained, not server-validated; future round-4 may extend `get_story_readiness` with `current_phase: Phase` if drift becomes a problem. Rejected: storing phase server-side in `attributes.plan_story_phase` — over-mechanises a UX concern that should stay in the chained-runner body.

5. **Multi-agent research dispatch follows `/plan-new` Phase 3 contract verbatim** (R30, R35). `/lumina:research-explore` body cites `flow-contract-vet-research` for the sampling + verification methodology; does not duplicate it inline. Each lens-agent prompt is self-contained (no inter-agent dependency), MUST instruct the agent to cite URLs verbatim and grade evidence high/medium/low, MUST emit ≥3 findings. Default 4 lenses (codebase / library / risk / completeness); +5th "domain" when story `complexity=high`. Trade: a story with limited scope may overfit to 4 lenses; user can re-run with `/lumina:research-explore <id> --lens codebase,library` (lens-subset arg) after round-3 ships.

6. **`/lumina:research-directed` iterates decisions, not claims** (R31). Reads answered `open_questions` + `execution_strategy`; for each decision that mentioned a library/API/version, fires one verification agent. Drift findings become `add_finding { kind: "research-drift" }` + supersede the stale `research_note` via `supersede_research_note`. Mirrors `/plan-new` Phase 5.

7. **Vet-research parallelisation is bounded** (R30, R34). `/lumina:vet-research` already exists in round-2; round-3 amends it to dispatch up to 4 concurrent verification sub-agents per pass (one per sampled note). Default sample size unchanged (`max(3, 30% of state=proposed count)`). Trade: above 4 the orchestrator chokes on returning agent count; cap matches the apply-flow agent-per-batch limit.

### Reuse map

- Migration 0006 follows the lumina migration idiom (forward-only, CHECK constraints for enum-shaped columns, nullable on ADD COLUMN per R16).
- `Tier` / `Severity` / `Phase` enum shapes mirror existing `Effort` / `Complexity` / `Relevance` (snake_case wire, `JsonSchema` derive, `serde rename_all = "snake_case"`).
- `compute_tier` follows the read-only repo function pattern (no events, no `pool.begin()`); `get_task_dispatch_plan` is also read-only.
- `research-explore` body reuses the round-2 forked-context idiom (`context: fork`, `agent: general-purpose`) — same as `/lumina:decompose-tasks` and `/lumina:story-review`.
- `vet-research` parallelisation reuses the round-2 pattern from `/lumina:decompose-tasks`'s "multi-agent fan-out heuristic" (T16) — dispatch sub-agents within the fork via the Agent tool.
- `plan-story` six-phase contract reuses the round-2 R6 budget cap (≤200 instructions); each phase block is ≤30 instructions.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo nextest run --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
sqlx-check: cargo sqlx prepare --check --manifest-path lumina/Cargo.toml
shared-blocks: bash scripts/verify-shared-blocks.sh
```

## Tasks

### Phase 1 — Backend foundation

#### 1. Migration 0006 — tier column + typed severity CHECK [M]
- **Files**: `lumina/migrations/0006_tier_and_severity.sql`
- **Depends on**: — (round-2 Phase 1 completed)
- **Action**: Forward-only migration. `ALTER TABLE work_items ADD COLUMN tier TEXT CHECK (tier IN ('lite','deep') OR tier IS NULL)` (R16: nullable required when CHECK applied on ADD COLUMN). Add a CHECK to `findings.severity` retro-actively: SQLite cannot ADD CHECK to existing table without table rebuild — instead document the constraint at the Rust layer (`Severity` enum + `validate_severity` in repo) and rely on validate-before-write at every entry point. Header comment: `-- Requires SQLite ≥3.38 (forward-only typing of tier + severity)`.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; `sqlx migrate run` against a fresh in-memory DB succeeds; `sqlx::migrate!()` picks up the file.

#### 2. Domain types — Tier, Severity, Phase enums [S]
- **Files**: `lumina/src/domain.rs`
- **Depends on**: 1
- **Action**: Add `Tier::{Lite, Deep}` enum (snake_case wire, derives `Serialize, Deserialize, JsonSchema, sqlx::Type`). Add `Severity::{Low, Medium, High, Critical}` enum (same derives). Add `Phase::{Frame, Explore, Decide, VerifyDesign, Decompose, Closure}` enum (kebab-case wire). Extend `Finding` struct: type `severity` field from `String` to `Severity`. Extend `WorkItem` (or task-detail variant): add `tier: Option<Tier>`.
- **Detail**: Match existing enum pattern (Effort/Complexity). `Phase` does not get persisted to a column in this round; it's a domain-layer derivation aid for plan-story.
- **Acceptance**: `cargo build` succeeds; clippy clean.

#### 3. Repo extensions — compute_tier, get_task_dispatch_plan, typed severity [M]
- **Files**: `lumina/src/repo.rs`
- **Depends on**: 1, 2
- **Action**: Three logically distinct edits: (a) add `compute_tier(effort: Option<Effort>, complexity: Option<Complexity>, files_touched_count: usize, has_cross_repo: bool) -> Tier` — pure function per the §k derivation rule; (b) add `get_task_dispatch_plan(pool, story_id) -> Result<Vec<Batch>, AppError>` returning `Batch = Vec<TierEntry>` where each entry has `{task_id, effort, complexity, tier, files_touched_count}` — composes `compute_task_batches` (round-2) + per-task spec reads + `compute_tier` per row; (c) tighten `add_finding` / `update_finding` to accept typed `Severity` (compile-time enforcement); add `set_finding_tier_hint(tx, id, hint: Option<Tier>)` (optional, attributes JSON merge).
- **Detail**: `get_task_dispatch_plan` is read-only — no event. `set_finding_tier_hint` is a single-mutation-path write — `pool.begin()` → `set_work_item_attributes(id, {tier_hint: hint})` → `record_event("finding.updated")` → commit. The `tier` column lives on `work_items`, not on `findings` (findings only carry the hint).
- **Acceptance**: `cargo build` succeeds; clippy clean; `compute_tier` has unit-test coverage for each branch of the rule.

#### 4. MCP layer — typed tier on SetTaskSpec, typed severity on add_finding, new read tool [M]
- **Files**: `lumina/src/mcp.rs`
- **Depends on**: 1, 2, 3
- **Action**: (a) Type `SetTaskSpecParams.tier` from `Option<serde_json::Value>` to `Option<Tier>` — the mcp tool now rejects free-form values at param-deserialise time. (b) Type `AddFindingParams.severity` to `Severity` — same. (c) Add new `#[tool]` `get_task_dispatch_plan` (annotations(read_only_hint = true, open_world_hint = false)). (d) Add `set_finding_tier_hint` write tool. (e) Optional convenience: `set_task_tier(id, tier)` write tool that wraps `set_work_item_attributes` to write the `tier` column directly.
- **Detail**: Existing `dispatch` callers (if any free-form values exist in attributes JSON from round-2 dev work) need to migrate to the enum. Round-2 plan was DB-canonical so no production data; tests need updating if any used the free-form shape.
- **Acceptance**: `cargo build` succeeds; new tools appear in `list_tools`; clippy clean.

#### 5. E2E tests — tier + severity coverage [M]
- **Files**: `lumina/tests/e2e.rs`
- **Depends on**: 4
- **Action**: Add thread tests for: (a) `compute_tier` returns Deep for each Deep-triggering input (complexity=high; effort=L; files=4; cross-repo); (b) `compute_tier` returns Lite for the residual case; (c) `set_task_spec` with `tier: "lite"` round-trips; (d) `set_task_spec` with legacy `dispatch: <anything>` is rejected (unknown-field error — the field was renamed to `tier`); (e) `add_finding` with `severity: "critical"` round-trips; (f) `add_finding` with `severity: "INVALID"` is rejected at deserialise time; (g) `get_task_dispatch_plan` on a 3-task story returns batches with correct tier per task; (h) closure-gate hard mode blocks `task → done` when an open finding has severity=high (regression for the new gate).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml` passes including the new cases; existing round-2 tests still pass.

#### 6. sqlx offline cache regen + lumina/CLAUDE.md update [S]
- **Files**: `lumina/.sqlx/*` (auto-generated), `lumina/CLAUDE.md`
- **Depends on**: 3, 4, 5
- **Action**: Run `cargo sqlx prepare -- --all-targets` inside `lumina/` to regenerate the offline cache. Update `lumina/CLAUDE.md`'s MCP-surface paragraph to enumerate the new tools (`get_task_dispatch_plan`, `set_finding_tier_hint`, optional `set_task_tier`) and the typing tightening on `set_task_spec` + `add_finding`.
- **Acceptance**: `cargo sqlx prepare --check --manifest-path lumina/Cargo.toml` exits 0 (benign warning OK); lumina/CLAUDE.md cites the new tools.

### Phase 2 — Research skills (parallel batch)

#### 7. New research skill — research-explore (multi-agent fan-out) [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/research-explore/SKILL.md`
- **Depends on**: 4 (consumes typed Severity for finding emission downstream of vet-research)
- **Action**: Author SKILL.md (~220 lines, body ≤200 instructions per R6). Frontmatter: 6 keys (4 mandatory + `context: fork`, `agent: general-purpose`). Body: (1) read story state via `get_work_item` + `get_story_readiness`; (2) select lenses (default `codebase, library, risk, completeness`; +5th `domain` when `complexity=high`); (3) dispatch all lens-agents in one Agent-tool message (parallel `<invoke>` blocks), one `flow-research-deep` per lens; (4) each per-lens prompt MUST instruct: read story content verbatim, cite URLs verbatim, grade evidence high/medium/low, return ≥3 findings; (5) compose findings into `add_research_note { state: "proposed", lens, source, body }` calls (batched); (6) emit console summary: `research-explore: <N> agents dispatched, <M> proposed notes added across {lens-list}; run /lumina:vet-research <story_id> to triage`.
- **Detail**: Cite `flow-contract-vet-research` for the sampling + verification methodology (do not duplicate inline). Lens vocabulary documented in CONVENTIONS.md §k.1 (T13). Per-lens prompt template included verbatim in the skill body as a fenced block — the SKILL.md is the template authority.
- **Acceptance**: 6-key frontmatter (4 + fork pair); body ≤200 instructions; cites `flow-contract-vet-research`; lens vocabulary matches the canonical 5; calls only `get_work_item`, `get_story_readiness`, `add_research_note`, `record_task_activity` (and within the fork: `Agent` tool, `Read`, `Grep`, `WebSearch`, `WebFetch`).

#### 8. New research skill — research-directed (post-decision lap) [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/research-directed/SKILL.md`
- **Depends on**: 4
- **Action**: Author SKILL.md (~150 lines). Frontmatter: 6 keys (4 + fork pair). Body: (1) read accepted research_notes + answered open_questions + execution_strategy via `get_work_item`; (2) extract decision claims (library / API / file:line / version pin) — heuristic regex over decision text; (3) for each decision, fire one verification agent (`flow-research` for mechanical lookup, `flow-research-deep` for ambiguous); (4) on drift: call `supersede_research_note { old_id, new_id }` and `add_finding { kind: "research-drift", severity: <typed Severity> }`; (5) on confirmation: call `update_research_note { id, confidence: "high" }`; (6) emit summary: `research-directed: <N> decisions verified, <M> drifts, <K> confirmations`.
- **Detail**: Cites R31 for "iterate decisions, not claims". The extraction heuristic is inline pseudocode in the skill body (not a regex constant — the skill author refines per-story content shape).
- **Acceptance**: 6-key frontmatter; calls only `get_work_item`, `supersede_research_note`, `update_research_note`, `add_finding` (typed Severity), `record_task_activity`.

#### 9. Vet-research amendment — parallelise spot-checks [S]
- **Files**: `claude/plugins/lumina-story-blocks/skills/vet-research/SKILL.md`
- **Depends on**: round-2 task 11 (creation of vet-research)
- **Action**: Amend the existing vet-research SKILL.md to dispatch up to 4 concurrent verification sub-agents per pass (one per sampled note). Replace any sequential "for each sampled note, verify" loop with a single Agent-tool message containing N parallel `<invoke>` blocks. Cap N at 4 (R30 — apply-flow agent-per-batch limit).
- **Detail**: Keep the existing sampling rule (`max(3, 30% of state=proposed count)`). Cite R30 for the cap.
- **Acceptance**: Body cites the 4-agent cap; the verification loop is single-message parallel dispatch.

### Phase 3 — Enforcement + dispatch consumers

#### 10. plan-story rewrite — six-phase canonical sequence with hard gates [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/plan-story/SKILL.md`
- **Depends on**: round-2 task 15 (plan-story creation), 7, 8, 9
- **Action**: Rewrite the existing plan-story SKILL.md to the six-phase shape (~220 lines, body ≤200 instructions per R6). Frontmatter unchanged (`disable-model-invocation: true`). Body structure: (1) read `get_story_readiness` at entry; (2) for each phase 1-6, evaluate the phase entry precondition (see §l in CONVENTIONS); (3) per-block within the phase: if precondition met → AskUserQuestion with `Run / Skip (warn) / Inspect / Abort`; if failed → AskUserQuestion with `Resolve prereq (sub-dispatch upstream skill) / Skip with override / Abort` (Run hidden); (4) Skip-with-override records `record_task_activity { entry_type: "execution", origin: "plan", summary: "skip_override: <block_slug>", details: { phase, prereq_failed } }`; (5) after each block, re-read `get_story_readiness` (handles user-side state changes); (6) Phase-6 closure-gate dispatch reads the open-findings count from `list_findings` and refuses if any has severity ∈ {high, critical}.
- **Detail**: The canonical sequence (problem-statement → user-interrogation → research-explore → vet-research → research-directed → alternatives → approach → not-doing → edge-cases → risks → verification-commands → acceptance-criteria → story-review → decompose-tasks → set-task-spec → wire-task-deps → closure-gate → relevance) walks through the six phases. Cites CONVENTIONS §l verbatim for the phase boundary preconditions.
- **Acceptance**: 4-key frontmatter; body walks 6 phases; per-block AUQ shape changes based on prereq state; skip-with-override emits the audit activity; body ≤200 instructions.

#### 11. set-task-spec amendment — capture effort+complexity, compute dispatch [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/set-task-spec/SKILL.md`
- **Depends on**: round-2 task 17 (set-task-spec creation), 4
- **Action**: Amend the existing set-task-spec SKILL.md to (a) capture `effort` (S/M/L) + `complexity` (low/medium/high) in the per-task AUQ sequence alongside the existing `execution_detail / files_touched / outcome / dispatch` capture; (b) call `set_effort` + `set_complexity` per task BEFORE calling `set_task_spec`; (c) call `repo::compute_tier` via a new read-only MCP wrapper (`compute_tier_preview`) — or recompute client-side per §k rule — to derive `dispatch` and present it to the user for confirmation; (d) the existing free-form `dispatch` capture is replaced with this derived value.
- **Detail**: The skill author transcribes the §k rule verbatim from CONVENTIONS.md so the client-side derivation matches the server-side. If `compute_tier_preview` is added as a read tool in T3/T4, prefer that over client-side recompute (single source of truth).
- **Acceptance**: Skill captures effort + complexity + dispatch in one walk; cites §k for the rule; calls `set_effort`, `set_complexity`, `set_task_spec` (with typed dispatch).

#### 12. wire-task-deps amendment — render batch dispatch counts + agent budget [S]
- **Files**: `claude/plugins/lumina-story-blocks/skills/wire-task-deps/SKILL.md`
- **Depends on**: round-2 task 18 (wire-task-deps creation), 4
- **Action**: Amend the existing wire-task-deps SKILL.md to call `get_task_dispatch_plan` after `compute_task_batches` and render the batch schedule with dispatch counts and an agent-budget summary, e.g.:
  ```
  Batch 1 (foundation, 3 tasks): T1 [L/high/deep], T2 [M/low/lite], T3 [S/medium/lite]
  Batch 2 (parallel, 2 tasks):   T4 [M/medium/lite], T5 [M/low/lite]
  Batch 3 (after T4):            T6 [L/high/deep]
  Agent budget: 2 deep (Opus) + 4 lite (Sonnet) across 3 batches
  ```
- **Detail**: Cite R34 + §k for the tier-derivation rule. The agent-budget summary helps the user / `/implement` carrier plan parallel dispatch (apply-flow caps at 4 agents per batch — verify the batch fits).
- **Acceptance**: Body cites `get_task_dispatch_plan`; the batch-render format matches the spec; calls only the listed read/write tools.

#### 13. CONVENTIONS.md amendments — §k Dispatch derivation + §l Six-phase sequence [M]
- **Files**: `claude/plugins/lumina-story-blocks/CONVENTIONS.md`
- **Depends on**: 4 (typed `Tier` and `Severity` are decided in code)
- **Action**: Add new §k "Tier derivation rule": canonical pseudocode of `compute_tier` (matches T3 implementation verbatim); §k.1 documents the canonical 5-lens vocabulary for `research_notes.lens` (codebase / library / risk / completeness / domain); §k.2 documents the typed `Severity` enum and the closure-gate's new severity-blocking behaviour. Add new §l "Six-phase canonical sequence": phase 1-6 boundaries with their hard preconditions (the table in this plan's Exploration Notes is the authoritative form); the "Skip with override" audit contract (`record_task_activity { entry_type: "execution", summary: "skip_override: …" }`); the per-block invocation-outside-plan-story carve-out (no enforcement outside the chained runner — R6 spirit).
- **Detail**: Keep §a–§j numbering intact — add §k and §l. Round-2 added §i + §j; round-3 appends §k + §l in alphabetical order.
- **Acceptance**: §a–§j still cross-reference correctly; §k + §l exist with the documented sub-sections.

#### 14. mcp catalogue + README + plugin.json closure [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`, `claude/plugins/lumina-story-blocks/README.md`, `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json`
- **Depends on**: 7, 8, 9, 10, 11, 12, 13
- **Action**: mcp/SKILL.md catalogue updated with `get_task_dispatch_plan` + `set_finding_tier_hint` (+ optional `set_task_tier`) under a new "Tier tools" section; updated `SetTaskSpecParams.tier` shape to `Tier` enum; updated `AddFindingParams.severity` shape to `Severity` enum. README.md skill-list table gains 2 new rows (`research-explore`, `research-directed`); existing rows for plan-story, set-task-spec, wire-task-deps, vet-research get an `amended in round-3` marker. plugin.json `version` bumped 0.2.0 → 0.3.0.
- **Detail**: 3 files in one task — under the apply-flow 3-file cap. Bundling permitted because all three are plugin-meta closure for round-3.
- **Acceptance**: README skill-list has 21 rows total (round-2's 19 + 2 round-3); mcp catalogue mentions every new tool; plugin.json shows 0.3.0.

### Phase 4 — Integration / verification

#### 15. Cross-reference updates — lumina/CLAUDE.md + repo-root CLAUDE.md [S]
- **Files**: `lumina/CLAUDE.md`, `CLAUDE.md`
- **Depends on**: 14
- **Action**: lumina/CLAUDE.md "Story-block skills plugin" section updated with the round-3 skill additions (point at the new README.md). Repo-root CLAUDE.md `## lumina` MCP-surface paragraph: add one sentence on dispatch composer + typed severity.
- **Detail**: Both updates additive; preserve all existing content.
- **Acceptance**: Both files mention the round-3 surface; existing paragraphs unchanged.

#### 16. End-to-end smoke test [M] — human-gated checklist (NOT dispatchable to /implement)
- **Files**: (none — manual; treated as a release-checklist item executed by the human after T15)
- **Depends on**: 1-15
- **Action**: Walk a real test story end-to-end through `/lumina:plan-story <id>`: (1) verify Phase 1 entry from a fresh story; (2) verify Phase 2 entry blocked until problem-statement is set; (3) verify `/lumina:research-explore` dispatches multiple parallel agents and adds proposed notes; (4) verify `/lumina:vet-research` triages in parallel; (5) verify `/lumina:research-directed` runs after approach decisions and surfaces drift findings; (6) verify Phase 5 entry blocked until acceptance-criteria is non-empty; (7) verify `/lumina:set-task-spec` captures effort+complexity and computes dispatch; (8) verify `/lumina:wire-task-deps` renders the batches + dispatch budget; (9) verify Phase 6 closure-gate blocks when an open finding has severity=high; (10) verify Skip-with-override records an audit activity entry; (11) re-invoke `/lumina:plan-story` and verify resumption from the current phase. Run the 5 guardrails at the end.
- **Acceptance**: Each phase boundary enforces the documented precondition; Skip-with-override emits the audit activity; dispatch-plan render matches the documented format; closure-gate blocks correctly; all 5 guardrail commands exit 0.

## Dependency Graph

- **Phase 1 (sequential — same-file overlap forces order)**: T1 → T2 → T3 → T4 → T5; T6 runs after T5.
- **Phase 2** (gated by Phase 1):
  - Batch 2.1 (parallel, 3 agents): T7 (research-explore), T8 (research-directed), T9 (vet-research amendment).
- **Phase 3** (gated by Phase 2):
  - Batch 3.1 (single): T13 (CONVENTIONS amendments — gates Phase-3 skills which cite §k/§l).
  - Batch 3.2 (parallel, 2 agents): T11 (set-task-spec amendment), T12 (wire-task-deps amendment) — file-disjoint.
  - Batch 3.3 (single): T10 (plan-story rewrite — depends on T11+T12 surface being stable since plan-story dispatches them).
  - Batch 3.4 (single): T14 (catalogue + README + plugin.json closure batch).
- **Phase 4** (gated by Phase 3):
  - Batch 4.1 (single): T15 (CLAUDE.md cross-refs).
  - Batch 4.2 (manual, single): T16 (end-to-end smoke).

Total batches: 8 (7 automatic + 1 manual). Parallelism peaks at 3 agents (Batch 2.1).

## Verification

- **Build guardrail**: `cargo build --manifest-path lumina/Cargo.toml` exits 0 after each Phase 1 batch and at end-of-plan.
- **Test guardrail**: `cargo nextest run --manifest-path lumina/Cargo.toml` exits 0 after T5 (new e2e tests) and at end-of-plan.
- **Lint guardrail**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — no new warnings.
- **sqlx cache integrity**: `cargo sqlx prepare --check --manifest-path lumina/Cargo.toml` exits 0 (benign warning OK) after T6.
- **Shared-block parity**: `bash scripts/verify-shared-blocks.sh` exits 0 (no-op for this plan's file set).
- **Plugin load**: `claude --plugin-dir claude/plugins/lumina-story-blocks` lists 21 commands prefixed `lumina:` (round-2's 19 + 2 new).
- **YAML frontmatter validity**: per-task acceptance step (P17 carry-over from round-2 review).
- **Manual smoke test** (T16): six-phase enforcement + dispatch plan render + closure-gate severity blocking + Skip-with-override audit trail.

## Risks

- **Risk**: `Tier` enum tightening breaks any free-form `set_task_spec.tier` values in dev databases. — **Mitigation**: round-2 is DB-canonical with no production data; T5 includes a regression test that the old free-form shape is now rejected so the breaking-but-additive nature is explicit.
- **Risk**: `Severity` typing on `add_finding` rejects any pre-existing string-typed call sites that pass a non-canonical value (e.g. `severity: "blocker"`). — **Mitigation**: Migration 0006 documents the canonical values; T5 covers the negative case; any caller drift surfaces immediately at deserialise time.
- **Risk**: Six-phase enforcement may frustrate users who want to write a quick story without research. — **Mitigation**: Skip-with-override path is explicit + audited, not blocked; the `/lumina:story-review` skill (round-2 T12) calls out override events so they surface during review rather than vanishing.
- **Risk**: Multi-agent fan-out in `/lumina:research-explore` consumes Opus tokens fast (4 agents × ~600-word prompt × ~30k-token research output). — **Mitigation**: Document the cost in the skill body; offer a lens-subset arg as a future extension; default 4 lenses matches `/plan-new`'s baseline so cost is comparable.
- **Risk**: `/lumina:research-directed` regex-based decision extraction may miss claims phrased in prose. — **Mitigation**: Fall back to a `flow-research-deep` extraction agent if regex matches < N=2 claims; document the heuristic in the skill body so future tuning is local.
- **Risk**: Closure-gate hard-mode now blocks on severity-high findings, which may surprise users who'd expect only critical findings to block. — **Mitigation**: CONVENTIONS §k.2 documents the rule explicitly; user can downgrade a finding's severity via `update_finding` if they accept the risk. Acknowledged in T13.
- **Risk**: `compute_tier` derivation rule may need calibration once real workloads land. — **Mitigation**: Pure function, single source in CONVENTIONS §k, easy to retune in a future round; T5 unit tests lock the current rule so changes are deliberate.
- **Risk**: Round-3 depends on round-2 completion. If round-2 ships with unresolved review-finding items (e.g. round-2 P2 SQL json_patch validator path), round-3 backend work may step on the same lines. — **Mitigation**: round-3 backend edits are in disjoint regions (new column, new domain types, new repo functions, new MCP tools) — no overlap with round-2 T3's `set_work_item_attributes` refactor surface. Verify at bootstrap.
- **Risk**: 18-file scope is at the upper edge of the per-flow ceiling. — **Mitigation**: Phase 1 is 6 sequential files with cargo-build checkpoint after each; Phases 2-4 are parallel-friendly. Recovery is per-phase.

## Future-round notes

Out-of-scope for round-3 but flagged for future planning:

- **`/lumina:run-batch <story_id>`** — the batch executor that reads `get_task_dispatch_plan` and dispatches Batch-N tasks to `flow-implement-lite` / `flow-implement-deep` agents in parallel-batches. Round-4 candidate. (Working name was `/lumina:implement-wave`; renamed for vocab consistency.)
- **`/lumina:render-plan <story_id>`** — markdown render of a story to the `/plan-new` plan.md format. Bridges to existing `/implement`, `/tdd`, `/plan-update` carriers. Round-4 candidate.
- **Lens-subset arg** — `/lumina:research-explore <id> --lens codebase,library` for targeted re-exploration. Round-4 polish.
- **api.ts schema additions** — `dispatch`, typed `severity`, and the round-2 sub-tables (risks / rejected_alternatives / task_dependencies) all need `.optional().default(...)` entries on `WorkItemDetailWireSchema`. Round-4 (UI sweep) or whenever the SPA gains rendering.
- **Phase persistence** — `attributes.plan_story_phase` for cross-session resumption if user telemetry shows churn. Defer until needed.
