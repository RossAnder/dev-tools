# Plan: Lumina Story Planning — Round 2 (Orchestration, Vet, Critique, Tasks, Backend gaps)

**Plan path**: `docs/plans/lumina-story-planning-round-2.md`
**Created**: 2026-05-26
**Status**: Draft

## Context

Round-1 (`docs/plans/lumina-story-planning-workflow.md`) shipped the 9-skill `lumina-story-blocks` plugin and survived a 64-item review (54 fixed, 8 verified-clean, 1 wontfix, 1 open). A subsequent deep-critique session (this conversation) compared it against `/plan-new` and the planning-research doc and surfaced **seven structural gaps**:

- **A** No orchestrator / "next block" advisor — the plugin advertises "run any block in any order" but every comparable system (Spec Kit, Kiro, HumanLayer, /plan-new) enforces ordering.
- **B** No research vet-pass — `/lumina:research-notes` writes self-graded notes; nothing samples/verifies them; `/lumina:approach` warns-and-continues on zero accepted notes.
- **C** No critique/review surface — 9 writers, 0 readers. Spec Kit has `/speckit.analyze`; /plan-new feeds /review-plan; lumina has nothing.
- **D** No task-decomposition skill — `/lumina:acceptance-criteria` writes onto task children but nothing creates them; the approach→tasks→AC chain has the middle missing.
- **E** No rejected-alternatives / risks / story-verification capture — three load-bearing /plan-new sections with no analogue.
- **F** `/lumina:not-doing` ships disabled (R1/R2: `update_work_item` does column-level COALESCE on `attributes`, not per-key merge).
- **G** Smaller items: lens drift risk, no context budgeting, no dedupe pass, prereq enforcement is "warn and continue".

The end-state is a Lumina web UI driving each block via a button, but this plan ships **the skill family + MCP/migration backing only** — agent↔UI interop (PTY supervisor, ACP) is a follow-up. The CLI surface must match `/plan-new`'s load-bearing strengths (vet, ordering, critique, decomposition) while remaining composable enough for the UI to drive per-block.

## Scope

- **In scope**:
  - **Lumina backend**: migration 0005 (new sub-tables `rejected_alternatives` + `risks` + `task_dependencies`; new column `work_items.task_kind`); `repo::set_work_item_attributes` refactored to SQLite `json_patch()`; widened `SetStoryPlanParams` (4 new optional fields: `not_doing`, `risks`, `alternatives`, `verification_commands` — only the JSON-merge ones; row-shaped data goes through new tools); new MCP tools (`add_risk` + CRUD; `add_rejected_alternative` + CRUD; `block_task_on_task` / `unblock_task_from_task` / `list_task_dependencies` / `compute_task_batches`; `get_story_readiness`; `set_task_kind`); `WorkItemDetail` fold extension; new e2e tests; sqlx cache regen.
  - **Plugin skills**: 10 new SKILL.md files (block writers: risks, alternatives, verification-commands; critique/vet: vet-research, story-review; orchestrators: next-block advisor, plan-story chained runner; task-decomposition family: decompose-tasks, set-task-spec, wire-task-deps); 2 amended SKILL.md (not-doing re-enable, approach hard-fail on zero accepted notes); CONVENTIONS.md amendments (vet entry_type exception, new §g.1 entries, story-review pattern section); mcp catalogue + research-notes cross-reference updates; README skill-list update; plugin.json version bump (0.1.0 → 0.2.0).
  - **Cross-references**: lumina/CLAUDE.md and repo-root CLAUDE.md updated with the new tool/skill surface.
  - **End-to-end smoke test** against a real story to validate the orchestrator and re-run tri-state behaviour.
- **Out of scope** (DO NOT scope-creep):
  - Web-UI work (PTY supervisor, ACP bridge, lumina/web/* SPA changes) — separate follow-up plan. **Note** (P7): `lumina/web/src/api.ts:459` `WorkItemDetailWireSchema` is plain `z.object()` (not `.strict()`), so the new Rust sub-table fields will be silently stripped — SPA won't throw, but `risks` / `rejected_alternatives` / `task_dependencies` remain invisible until the follow-up UI plan adds `.optional().default([])` entries.
  - Pinia / vue-router additions (memory note `feedback_lumina_web_state_management` applies whenever UI lands).
  - MCP elicitation (R14): kept as future-option documentation only; no implementation.
  - Marketplace listing for the plugin.
  - `/lumina:dedupe-notes` skill (smaller item G, deferred to a future round).
  - Any changes outside `claude/plugins/lumina-story-blocks/**`, `lumina/**`, and the two `CLAUDE.md` cross-refs.
- **Affected areas** (Phase 9 scope-glob derivation):
  - `claude/plugins/lumina-story-blocks/**`
  - `lumina/**`
  - `CLAUDE.md`
- **Estimated file count**: ~24 unique files (1 migration + 5 lumina source files + 1 sqlx cache batch + 10 new SKILL.md + 2 amended SKILL.md + 4 plugin meta files + 2 cross-ref CLAUDE.md). **Exceeds the 15-file ceiling — see Approach for the phase-split.** **Open decision (P23)**: at Phase 9 bootstrap, consider splitting into two flows — `round-2a` (Phase 1 backend only, ~6 files) and `round-2b` (Phases 2-4 plugin, ~18 files) — so backend lands + ships with its own commit-and-test cycle before plugin authors depend on the new MCP surface. Existing phase-sequencing already gates plugin behind backend completion, so the split is purely about commit/test cadence, not integrity.

## Exploration Notes

### Backend (lumina) — MCP / repo / migration surface

- **The merge primitive exists**: `repo::set_work_item_attributes(pool, id, patch: &Value)` (`lumina/src/repo.rs:1524`) does per-key JSON merge atomically. `set_story_plan` (`mcp.rs:986`) and `set_task_spec` (`mcp.rs:1021`) compose on it. **The `/lumina:not-doing` fix is a one-line widening** — either expose `set_work_item_attributes` directly or widen `SetStoryPlanParams` / introduce `SetStoryMetaParams` to accept `not_doing`.
- **`update_work_item`** (`repo.rs:1376`, `mcp.rs:929`) uses column-level COALESCE on `attributes` — the documented hazard. Not to be called directly with `attributes` from any skill.
- **Single-mutation-path invariant**: every mutation = `pool.begin()` → domain write → `record_event(aggregate_type, id, event_type, payload)` → commit. Helpers: `record_event(repo.rs:3010)`, `normalise_object(repo.rs:91)`, `validate_entry_kind(repo.rs:71)`.
- **Migration 0003 child-table pattern**: TEXT PK, parent FK ON DELETE CASCADE, `seq` monotonic, UNIQUE(parent, seq), (parent, seq) index, `created_at` default. Established by `acceptance_criteria` / `research_notes` / `open_questions` / `question_options`. Migration 0004 added `repo_links` and `findings.repo_id`.
- **WorkItemDetail fold**: `repo::get_work_item_detail` reads each sub-table into a `WorkItemDetail.X: Vec<...>` field; `export::export_pending(pool, root)` (`src/export.rs:111`) re-renders the whole detail to TOML on every event — new sub-tables propagate to the git-export trail automatically.
- **`record_task_activity`**: `TaskActivityType{Execution|Vet|Comment}` (param-edge enum at `mcp.rs:364`; backend `ActivityType` at `domain.rs:406` validated by `validate_entry_kind` at `repo.rs:71` — accepts `vet` today, no backend change needed for T11). The plugin's blanket `vet`-ban (CONVENTIONS.md §c) is a self-imposed convention — `vet` is a valid backend channel and is the right type for a `/lumina:vet-research` skill.
- **`check_acceptance_criterion`** (params at `mcp.rs:553`; tool fn at `mcp.rs:1360`) internally appends a `verification` activity entry — that's why CONVENTIONS.md §c reserves `verification` from skill use.
- **E2E test pattern**: `lumina/tests/e2e.rs` uses `db::connect_in_memory()` + MCP helpers + `export::export_pending` direct call + `tower::ServiceExt::oneshot` against the axum router. No socket bind, no sleep — every new MCP tool gets a thread test in this file.
- **Verification**: `cargo build`, `cargo nextest run`, `cargo clippy --all-targets`, `cargo sqlx prepare --check` (all `--manifest-path lumina/Cargo.toml`); coverage gate `cargo llvm-cov nextest --fail-under-lines 80 --fail-under-regions 70`.

### Plugin internals — `claude/plugins/lumina-story-blocks/`

- **9 skills**: problem-statement, research-notes (forked), user-interrogation, acceptance-criteria, approach (draft-then-confirm with prereq survey), not-doing (DISABLED banner), edge-cases, relevance, closure-gate.
- **CONVENTIONS.md** §a–§h is mature; §g is split into §g.1 (attribute-key) and §g.2 (column-value lens) with promotion policies. The verbatim §b-supersession `AskUserQuestion` template is single-sourced and cited from every skill.
- **mcp/SKILL.md** is the absorbed read-only catalogue (R4 resolution) — model-discoverable, the one §a exception to `disable-model-invocation`.
- **No automated tests of skill bodies** — convention compliance is human/review-driven. The round-1 review caught 64 items, which is both the strength of the review process and the weakness of having no automated parity tests.
- **Cross-refs**: repo-root CLAUDE.md:47 cites the catalogue; lumina/CLAUDE.md lines 34–54 cite the plugin install + prereqs.

### Ecosystem — flow-contract skills and command patterns to reuse

- **The `flow-contract-*` skill family is the canonical source** for vet-pass procedure (`flow-contract-vet-research`), plan-output structure (`flow-contract-plan-output-format`), ledger schema (`flow-contract-ledger-schema`), execution-record (`flow-contract-execution-record-schema`), flow context (`flow-contract-flow-context`), and plansDirectory prompt (`flow-contract-plansdirectory-prompt`). Round-2 lumina skills **should CITE these contracts** rather than reinventing the procedures inline — `/lumina:vet-research` is `flow-contract-vet-research` applied to lumina's research_notes table.
- **`verification` agent** (Haiku) runs ordered build/test/lint commands and short-circuits on fail; forbidden-working-tree-ops block applies. Reusable for a `/lumina:verify-story` skill that runs declared story-level commands.
- **`commit-conventions` skill** resolves the project's dialect (currently default Conventional Commits — no `.claude/commit-conventions.toml`).
- **`tomlctl flow init` is atomic**: creates `.claude/flows/<slug>/{context,execution-record}.toml` + both sidecars + active-flow registry entry in one CLI call.
- **Pre-commit hook surface**: `scripts/shared-blocks.toml` manages exactly one block (`forbidden-working-tree-ops` in `flow-implement-{deep,lite}.md`); no parity check applies to the lumina plugin files.

### Reusable patterns from existing plans

- `docs/plans/lumina-schema-deepening.md` is the closest precedent for "add typed sub-table + MCP tool family + skill consumer" — its 4-table addition (acceptance_criteria, research_notes, open_questions, question_options) is exactly the shape we'd repeat for any new sub-table this plan needs.
- `docs/plans/lumina-mcp-schema-foundation.md` establishes the migration→domain→repo→mcp→export 6-step idiom.

### Constraints surfaced

- All lumina backend writes route through one `repo::*` function (single mutation path) — no skill may compose mutations client-side.
- The COALESCE bug on `update_work_item.attributes` is the load-bearing reason `/lumina:not-doing` is broken — no skill may invoke `update_work_item` with `attributes` until an audit confirms it doesn't clobber siblings.
- Estimated round-2 file count: ~18–25 files across plugin SKILL.md, CONVENTIONS.md, README.md, lumina migrations, lumina src/mcp.rs + src/repo.rs + tests/e2e.rs, lumina/CLAUDE.md, repo-root CLAUDE.md. **Multi-phase shape is required**; a multi-file plan (`00-outline.md` + per-phase detail docs) is the candidate output shape if total scope exceeds the ~15 unique-file ceiling at the scope-check.

## Research Notes

**Vet outcomes** (per `flow-contract-vet-research`):
- Agent-1 (orchestration-deep, Opus) — 3 sampled / 0 dropped / 1 downgraded (R2 evidence high→medium: issue is community-reported, not Anthropic-maintainer-confirmed).
- Agent-2 (mcp-sqlite-mechanical, Sonnet) — 3 sampled / 0 dropped / 1 downgraded (R18 evidence high→medium: crates.io spot-check fetch returned empty content; confirm version pin at use-time).
- No `ESCALATE-TO-DEEP` flags raised. No systemic >30% failure → no re-dispatch.
- `[[vet_events]]` heredoc append deferred to Phase 9 (no ledger exists yet pre-bootstrap; this prose record is the durable trace per round-1 precedent).

### Phase 3 findings — Orchestration / advisor / state-machine architecture

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R1 | Superpowers `using-superpowers/SKILL.md` is the canonical "advisor + N mutators" precedent. Master skill is model-discoverable (NO `disable-model-invocation`), recommends siblings via decision-flowchart prose + named categories — NOT via structured output or `AskUserQuestion`. | https://github.com/obra/superpowers/blob/main/skills/using-superpowers/SKILL.md (vet ✓) | high | The `/lumina:next-block` advisor skill follows this shape: model-discoverable read-only skill, body enumerates `/lumina:*` siblings BY NAME (because siblings have `disable-model-invocation: true`, the model cannot auto-load their descriptions). |
| R2 | `disable-model-invocation: true` still injects skill descriptions into the system-reminder skill list — it blocks auto-trigger but NOT context-token cost. (~200–300 tokens per conversation for a 6-skill family per issue.) Issue #43875 separately reports the flag sometimes hides the skill *entirely* from invocation. | https://github.com/anthropics/claude-code/issues/31935 (closed as duplicate, no maintainer confirmation; vet ✓ but evidence downgraded high→medium) | medium | Keep sibling `description:` lines terse (≤140 chars) since they ride in context regardless. The advisor's body can reference siblings by slash name without re-describing. Plan for the `disable-model-invocation` semantics to stay stable (worst case both ways: still in context OR also blocking slash invocation). |
| R3 | GitHub Spec Kit `check-prerequisites.sh` enforces ordering purely via artefact-presence (hard-exit 1 if `plan.md` missing, `tasks.md` missing under `--require-tasks`). No state file. No protection against re-running a command after a downstream command already ran. | https://github.com/github/spec-kit/blob/main/scripts/bash/check-prerequisites.sh (vet ✓) | high | Lumina mirrors this with a single MCP read tool (`get_story_readiness`) returning structured booleans. The advisor skill reads it and decides; no separate prerequisite script. Re-running a skill that has populated downstream blocks is permitted (matches lumina's idempotency-with-confirmation contract). |
| R4 | AWS Kiro's "change propagation" is user-triggered manual buttons (Sync Files / Refine), NOT an automatic cascade. No stale-markers. Public blog posts overclaim. | https://kiro.dev/docs/specs/best-practices/ | high | Do NOT implement a stale-marker cascade. When upstream blocks change, the advisor's recommendation surfaces a "re-run /lumina:approach?" hint; user triggers. |
| R5 | The "skill chains / plays" pattern is documentary in CLAUDE.md prose, NOT a first-class artefact format. Spec Kit's per-feature `tasks.md` is the closest first-class precedent; no shop publishes a reusable `plays.yaml`. | https://code.claude.com/docs/en/skills + community posts | medium | Do NOT add a `plays.yaml` / play-runner skill. The advisor reading story state IS the play; the DB is the state. |
| R6 | Alex Lavaee's RPI→QRSPI write-up documents three failure modes of under-orchestrated skill plays: (a) instruction-budget overflow (~150–200 instructions), (b) magic-word dependency, (c) plan-reading illusion. QRSPI's fix is MORE stages with explicit human gates (8), not fewer. | https://alexlavaee.me/blog/from-rpi-to-qrspi/ | high | Cap the advisor body ≤150 instructions. Keep each `/lumina:*` block independently runnable (no orchestrator-only gates) so power users bypass — orchestration is recommendation, not enforcement. |
| R7 | No published precedent for DB-enforced "unread until vetted" gating of agent-generated content. Vet stays a behavioural contract. | `agent-reasoning` after searches in Spec Kit / Kiro / HumanLayer / Superpowers | medium | Vet enforcement lives at the *read* layer (advisor only counts `state="accepted"` notes toward readiness), not at the DB layer. Cheaper, reversible. |
| R8 | Structured `get_story_readiness` MCP output (booleans + `next_recommended_action` enum) is genuinely novel — Spec Kit returns exit codes (text), Kiro shows UI badges, no precedent returns JSON consumable by both an advisor skill and a future UI panel. | `agent-reasoning` (novelty cuts both ways — design-space judgement call) | medium | Add `get_story_readiness` returning raw fields PLUS one `next_recommended_action` enum so the advisor body stays a thin pass-through. Computing the recommendation server-side prevents the advisor body from becoming a rule engine that drifts from the schema. |

### Phase 3 findings — MCP / SQLite / JSON-merge mechanics

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R11 | SQLite has built-in `json_patch(T, P)` implementing RFC-7396 MergePatch (replace present, leave absent, null-deletes key); `jsonb_patch()` variant returns binary JSONB. Available in SQLite since the 3.x json1 era. | https://sqlite.org/json1.html (vet ✓) | high | Two viable paths for the `/lumina:not-doing` fix: (a) single-statement `UPDATE work_items SET attributes = json_patch(attributes, ?)` in SQL — eliminates read-modify-write round-trip; (b) Rust-side merge via `json-patch` crate (R18). Prefer (a) for atomicity + simpler code. NULL-key deletion semantics MUST be tested — callers must not inadvertently null out keys. |
| R12 | sqlx 0.9 offline-cache regen: `cargo sqlx prepare -- --all-targets --all-features`; verify with `cargo sqlx prepare --check`. Benign "potentially unused queries" warning on `--check` is expected when cache was prepared with `--all-targets`. | https://github.com/launchbadge/sqlx/blob/main/sqlx-cli/README.md | high | Matches lumina/CLAUDE.md guidance — no change. Plan tasks adding new `query!`/`query_as!` macros must include "regen `.sqlx/` cache" in acceptance criteria. |
| R13 | rmcp 1.7 `#[tool]` macro pattern: `#[tool(description = "…", annotations(read_only_hint = true))]` on `&self async fn`; params via `Parameters<T>` where `T: Deserialize + JsonSchema`; no-param tools use `Parameters(())`. `#[tool_router]` decorates the `impl`. | https://docs.rs/rmcp/latest/rmcp/ + https://github.com/modelcontextprotocol/rust-sdk/blob/main/examples/servers/src/common/calculator.rs | high | Any new read tool (`get_story_readiness`, `list_story_blocks`) should carry `annotations(read_only_hint = true)`. Matches existing lumina pattern. |
| R14 | MCP 2025-06-18 spec defines `elicitation/create` — server-driven mid-execution request for structured user input; client renders form, returns accept/decline/cancel. rmcp 1.7 ships an `elicitation_stdio.rs` example. | https://workos.com/blog/mcp-elicitation + https://dzone.com/articles/mcp-elicitation-human-in-the-loop-for-mcp-servers (spec.modelcontextprotocol.io fetch failed cert-error; relying on secondary sources — kept at medium) | medium | Out of scope for this plan (the skill layer drives interactive prompts via `AskUserQuestion` at the orchestrator level, not from inside an MCP tool). Document as a future option for the Vapor UI's "request" surface. Don't take a dependency on it. |
| R15 | The `superseded_by` nullable self-FK pattern is the idiomatic SQLite append-only-history primitive (no `SYSTEM VERSIONING` clause in SQLite). Alternative (effective-dated rows) is overkill for low-cardinality chains. | https://sqlite.org/foreignkeys.html + https://www.sqliteforum.com/p/sqlite-and-temporal-tables | medium | Reuse the pattern for any new sub-table this plan introduces (e.g. `rejected_alternatives` if added). Matches existing `research_notes.superseded_by` and `findings.superseded_by`. |
| R16 | SQLite `ALTER TABLE … ADD COLUMN … REFERENCES …` REQUIRES the column's default value to be NULL when FK constraints are enabled — any other default is rejected. | https://sqlite.org/lang_altertable.html | high | Any migration adding an FK column to an existing table must use a nullable column (no `NOT NULL`, no non-NULL default). Matches lumina migration 0004's pattern. |
| R17 | `tower::ServiceExt::oneshot` against `axum::Router` is still the canonical in-process router test pattern in axum 0.8 / tower 0.5 — no socket bind. | https://github.com/tokio-rs/axum/blob/main/examples/testing/src/main.rs | high | Reuse for every new MCP tool's e2e test in `lumina/tests/e2e.rs`. Matches existing pattern. |
| R18 | `json-patch` crate v4.x exposes `json_patch::merge(doc: &mut Value, patch: &Value)` implementing RFC-7396 in-place on `serde_json::Value`; only depends on `serde_json`. | https://docs.rs/json-patch/ (crates.io fetch returned empty content — verify pinned version at use-time; downgraded high→medium) | medium | Alternative to SQLite-side `json_patch()` (R11) if the merge must happen Rust-side for validation. R11's SQL approach is preferred (atomic, single round-trip); R18 is the fallback if validation-before-write is required. |
| R19 | `SqlitePool::connect("sqlite::memory:")` with default pool size has a hard gotcha: each connection opens a separate in-memory DB. Use `max_connections(1)` or `sqlite:file::memory:?cache=shared&mode=memory` URI for shared-state tests. `#[sqlx::test]` handles this automatically. | https://github.com/launchbadge/sqlx/issues/2510 + https://github.com/launchbadge/sqlx/issues/362 | high | The existing `lumina/src/db.rs:connect_in_memory()` helper must be verified to pin `max_connections(1)` or use the shared-cache URI. If a new test introduces parallelism with a fresh pool, it must follow the same idiom. |
| R20 | Vue 3 module-singleton composables (state declared at module scope) is the official Vue-docs-blessed shared-state pattern — no Pinia, no `provide/inject` needed for an SPA. | https://vuejs.org/guide/reusability/composables | high | Documentary only — UI work is out of scope for this plan. Recorded for the follow-up UI plan. |

### Directed research additions (Phase 5: task-decomposition architecture)

**Vet outcome**: 2 sampled, 0 dropped, 0 downgraded. Both spot-checks fully verified.

| # | Finding | Source | Grade | Impact on plan |
|---|---------|--------|-------|----------------|
| R21 | Spec Kit `/speckit.tasks` uses single-line markdown `- [ ] T### [P?] [USn?] Description with file path`. NO explicit "Depends on" field — dependencies are implicit via phase ordering (Setup → Foundational → User Stories(P1..) → Polish). Foundational phase is the documented exception ("blocking prerequisites for all user stories"). | https://github.com/github/spec-kit/blob/main/templates/commands/tasks.md (vet ✓) | high | Lumina's chosen `task_dependencies(task_id, depends_on_id)` table is MORE expressive than Spec Kit's implicit ordering — confirms the option-3 design choice. A future renderer can convert the DAG back to ordered phases for human-readable export, but the canonical store is the edge table. |
| R22 | AWS Kiro builds a dependency graph from `tasks.md` and executes in **waves** (topological-sort batched execution) — Wave N runs all tasks whose deps were satisfied by Waves <N. Closest published precedent for the explicit-DAG model. (Kiro's term "wave" is retained when citing their pattern; lumina adopts the concept under the term "phase".) | https://kiro.dev/docs/specs/best-practices/ + https://kiro.dev/docs/specs/ | medium | `/lumina:wire-task-deps` MUST encode dependencies as edges; `/implement` consumes the DAG via topological sort and dispatches Phase-N tasks in parallel. Adopt phase-batched execution as the documented consumer pattern; document it in the skill body. |
| R23 | HumanLayer's atomic unit is a **Phase** (not a task) with explicit dual-track success criteria — `Automated Verification` (executable commands) + `Manual Verification` (human checks). Dependencies between phases are IMPLICIT via sequential ordering. | https://github.com/humanlayer/humanlayer/blob/main/.claude/commands/create_plan.md (vet ✓) | high | Extend lumina's `set_task_spec` to carry a structured dual-track `outcome` field (split into automated + manual lists) rather than free-text. HumanLayer's phase ≈ lumina's story, not task — don't directly mimic phase format. |
| R24 | Vertical-slice rule has a documented foundation-first exception: shared schema migrations, type defs, and base abstractions MUST precede consumers — the rule degrades to horizontal layering at the foundation boundary. | https://arxiv.org/html/2601.22667v1 + HumanLayer plan template (orderings imply this) | medium | `/lumina:decompose-tasks` should bias toward vertical slices but flag "foundation tasks" (migrations, shared types) as a distinct task-kind that other slices depend on. Encode `task_kind ∈ {foundation, vertical-slice, polish}` (or similar) so the phase scheduler can hoist foundation tasks. |
| R25 | No published agent-side pattern for "agent materialises exhaustive file list into the task spec and refuses completion until every listed file is touched". The tooling side is solved (ast-grep / semgrep / jscodeshift), and Codemod MCP exposes this to agents. Spec Kit's per-task explicit file path is the partial precedent but doesn't enforce list COMPLETENESS. | https://codemod.com/blog/jssg + https://www.hypermod.io/blog/4-jscodeshift-vs-ast-grep + https://semgrep.dev/docs/writing-rules/autofix | medium | Add task-kind `pattern_replacement` to `set_task_spec` whose `files_touched` is populated by an upfront `Grep --files_with_matches` call recorded as a research-note source. Completion gate: every listed file must appear in the task's git-diff. **Novel cross-cutting contribution**; document in `/lumina:decompose-tasks` body. |
| R26 | Multi-agent fan-out for decomposition is justified when the task has **separable independent dimensions**; AdaptOrch formalises this as topology selection from the dependency graph. Empirical gain is ~36% wall-clock reduction in one cited workflow; 5–10% of multi-agent code generation produces semantic conflicts requiring merge. | https://www.anthropic.com/engineering/multi-agent-research-system + arxiv 2503.07675 + arxiv 2602.16873 | medium | For `/lumina:decompose-tasks`, fan out ONLY when the story spans >1 foundation-disjoint module (separate crates, separate top-level dirs with no shared types). Single-module stories: one Opus pass. Document this heuristic in the skill body. Post-merge dedup pass required after multi-agent decomposition. |
| R27 | Empirical: agent reliability degrades **super-linearly** with task complexity (jointly determined by duration + domain structure) across 23,392 episodes. Failed trajectories are consistently longer with higher variance than successful ones. | arxiv 2511.00197 + arxiv 2603.29231 + arxiv 2505.05115 | high | `/lumina:decompose-tasks` should bias aggressively toward smaller tasks (single-file scope where possible). Reuse lumina's existing `complexity` column as the size signal; `/lumina:wire-task-deps` flags any task with `complexity="high"` for explicit user confirmation that it shouldn't split further. The complementary gate belongs in wire-deps, not in decompose itself. |
| R28 | Decomposition idempotency on re-run is unsolved in published SDD frameworks. Spec Kit acknowledges "stable regeneration is difficult" and mentions a `Supersedes` field but offers no protocol for in-flight or completed tasks. Community workaround: mark tasks `done` so the AI leaves them alone. | https://github.com/github/spec-kit/blob/main/spec-driven.md + https://tessl.io/blog/spec-driven-development-10-things-you-need-to-know-about-specs/ | low (hypothesis with verification step: read actual Spec-Kit tasks.md regeneration in a working repo) | Tri-state task disposition on re-run: (a) `status=done` tasks are immutable and copied through; (b) `status=in_progress` tasks abort the re-run with a confirm-prompt; (c) not-started tasks are superseded en-masse with an `events`-table `decomposition_regenerated` entry pointing to the new batch. Architectural extension of lumina's existing supersession pattern; explicitly framed as such (not a published SDD convention). |
| R29 | **Cross-cutting**: Spec Kit + HumanLayer use IMPLICIT dependency ordering via phase sequencing; Kiro uses EXPLICIT edges enabling wave-batched execution (their term). Lumina's three-skill split (`decompose-tasks` / `set-task-spec` / `wire-task-deps`) aligns with Kiro's model under lumina's "phase" terminology. | (synthesis of R21–R23) | high | Document the explicit-DAG choice in the plugin README as the load-bearing differentiator from Spec Kit / HumanLayer — costs one extra `wire-task-deps` step that markdown-based frameworks elide, gains phase-batched parallel execution + first-class re-run idempotency via R28's tri-state. |

## Approach

### Architecture — three phases on a fixed dependency spine

The 24-file scope and the inter-pillar dependencies (skill bodies need MCP tools; orchestrator skills need readiness reads; task family needs the dependency table) force a phase-sequential layout. Within each phase, file-disjoint tasks parallelise up to the 4-agent-per-batch cap.

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 1 — Lumina backend foundation                                     │
│  Migration 0005 → domain types → repo CRUD → MCP tools → tests + sqlx   │
│  Output: not_doing-safe attributes; risks/alternatives/task_deps tables;│
│  get_story_readiness; widened set_story_plan; SQL json_patch primitive  │
└─────────────────────────────────────────────────────────────────────────┘
                              │ blocks all Phase 2-3 skills
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 2 — New block writers + critique + vet (parallel batches)         │
│  CONVENTIONS update first; then risks / alternatives / verification-cmd │
│  / vet-research / story-review writers; re-enable not-doing; harden     │
│  approach with hard-fail-on-zero-accepted-notes (closes critique B)     │
└─────────────────────────────────────────────────────────────────────────┘
                              │ blocks Phase 3 orchestrators (advisor needs
                              │ every block's slash command name; chained
                              │ runner walks the full sequence)
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 3 — Orchestration + task decomposition                            │
│  next-block advisor (model-discoverable) → plan-story chained runner →  │
│  decompose-tasks (forked, multi-agent fan-out heuristic) →              │
│  set-task-spec → wire-task-deps; mcp catalogue + research-notes + README│
│  + plugin.json version bump in one closing batch                        │
└─────────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────────────┐
│  Phase 4 — Integration / verification                                    │
│  Cross-ref CLAUDE.md updates; end-to-end smoke test                     │
└─────────────────────────────────────────────────────────────────────────┘
```

### Key design decisions (with rationale and rejected alternatives)

1. **SQL-side `json_patch()` over Rust-side merge** (Q1, R11). Replace `repo::set_work_item_attributes`'s read-modify-write with a single `UPDATE work_items SET attributes = json_patch(COALESCE(attributes,'{}'), ?) WHERE id = ?`. Eliminates the round-trip; semantics are RFC-7396 (replace-present / leave-absent / null-deletes-key). **Trade**: null-key-deletion is callable — Phase 1 tests must cover both "passing `{x: null}` deletes x" and "passing `{}` leaves all keys" to lock the contract. **Rejected**: Rust-side `json-patch` crate (R18). Same semantics but more code, more deps, and no validation gain over the SQL form since lumina already validates the merged JSON in the per-kind validator at write time.

2. **Widen `SetStoryPlanParams` for JSON-merge-shaped story-meta only; row-shaped data gets its own tools** (Q2, Q3). `not_doing` and `verification_commands` are small structured values that ride the attributes JSON via the widened `set_story_plan`. `risks` and `rejected_alternatives` are multi-row content with supersession history requirements — they get typed sub-tables (mirror `research_notes` shape) and their own MCP tool families. **Rejected**: a generic `set_work_item_attributes` MCP tool (Q2 option 3). Any skill could write any key without lumina-side validation — trades safety for flexibility. **Rejected**: lens conventions on `research_notes` for risks + alternatives (Q3 option 3). The §g lens-drift warning applies strongly here — conflating distinct content under one table impairs per-kind queries.

3. **Advisor + chained-runner hybrid** (Q4, R1 + R5 + R6). `/lumina:next-block <id>` is model-discoverable, reads `get_story_readiness`, prints recommendation prose. `/lumina:plan-story <id>` is `disable-model-invocation: true`, walks the canonical sequence with per-block AskUserQuestion gates. Both delegate to the same per-block skills; neither hides the per-block surface from the UI. Cap each body ≤200 instructions (R6 budget). **Rejected**: chained-runner-only — loses Spec-Kit/Superpowers-style discoverability. **Rejected**: advisor-only — CLI users still want a "run the whole thing" affordance.

4. **`/lumina:story-review` writes findings via `add_finding`** (Q5). Existing `findings` table from migration 0001 already has supersession + UI surface + the `kind` discriminator (we use `kind="story-review"`). Forked context (multi-step audit + isolate noise per §d). Rubric covers contradictions across blocks, ungrounded approach claims, AC-vs-problem-statement mismatch, uncovered edge cases, silently-assumed-answered open questions, unsplit complexity=high tasks. **Rejected**: console-only — the future UI would have nothing to display.

5. **Task-decomposition family: 3 skills + new `task_dependencies` table** (Q6, R21–R29). `/lumina:decompose-tasks` (forked, deep judgement, multi-agent fan-out when story spans >1 foundation-disjoint module per R26), `/lumina:set-task-spec` (per-task author with dual-track outcomes per R23), `/lumina:wire-task-deps` (writes task→task edges, runs Kahn topological sort via `compute_task_batches`, flags cycles, hosts the complexity-high split gate per R27). New `task_kind` column lets the phase scheduler hoist foundation tasks (R24). `pattern_replacement` task-kind requires exhaustive file enumeration via `Grep --files_with_matches` recorded in `set_task_spec.files_touched` (R25 — novel cross-cutting contribution). Re-run idempotency uses R28's tri-state: `status=done` immutable / `status=in_progress` AbortWithConfirm / not-started superseded en-masse via an `events`-table `decomposition_regenerated` row. **Rejected**: single decompose skill — under-fits Q6's elaborated user steer. **Rejected**: implicit phase ordering à la Spec Kit — loses phase-batched execution and re-run history.

6. **Approach skill hard-fails on zero accepted research-notes** (critique B). The existing warn-and-continue stance lets approach draft from hallucinations. Replace with abort + one-line message pointing the user at `/lumina:vet-research <id>`. This closes the integrity boundary identified in the critique without requiring DB-level enforcement (R7: vet stays a read-layer behaviour).

7. **CONVENTIONS.md amendments stay backward-compatible** with §a–§h numbering. New content: §c amendment to permit `entry_type: "vet"` for the `vet-research` skill only (one-skill exception, explicitly named); §g.1 row for `attributes.verification_commands` and a DELETION of the disabled `attributes.not_doing` row (now safe via SQL json_patch); new §i "Story-review pattern" documenting `kind="story-review"` findings + the phase-batched task execution contract.

### Reuse map (existing patterns to mirror, not reinvent)

- Migration shape (TEXT PK + parent FK ON DELETE CASCADE + `seq` UNIQUE(parent,seq) + indexes + `created_at` default + supersession self-FK): mirror migration 0003 verbatim for `risks` and `rejected_alternatives`.
- `repo::record_event` single-mutation-path: every new CRUD method wraps `pool.begin()` → domain write → `record_event` → commit.
- `set_story_plan` already composes on `set_work_item_attributes` (repo.rs:986); just add four optional fields to `SetStoryPlanParams` and pass them through unchanged.
- `WorkItemDetail` fold pattern (repo.rs `get_work_item_detail`): mirror existing `research_notes` / `acceptance_criteria` fold for new sub-tables.
- E2E thread test idiom (`tests/e2e.rs`): in-process pool, MCP helpers, `export_pending` direct call, `tower::ServiceExt::oneshot` against `app::build_router`. Reuse for every new MCP tool (R17).
- CONVENTIONS §b 5-step idempotency, §b-supersession verbatim phrasing, §c provenance template, §d forked-context (use for `research-notes`, `story-review`, `decompose-tasks` — the three deep multi-step skills), §e Sentry pattern.
- `flow-contract-vet-research` is the procedure source for the new `/lumina:vet-research` skill — cite by skill name, do not duplicate inline.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo nextest run --manifest-path lumina/Cargo.toml
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets
sqlx-check: cargo sqlx prepare --check --manifest-path lumina/Cargo.toml
shared-blocks: bash scripts/verify-shared-blocks.sh
```

`shared-blocks` is a no-op guardrail for this plan's file set (the manifest covers only `flow-implement-{deep,lite}.md`); it runs to confirm no accidental drift was introduced.

## Tasks

### Phase 1 — Lumina backend foundation

#### 1. Migration 0005 — new sub-tables and task_kind column [M]
- **Files**: `lumina/migrations/0005_round2_planning.sql`
- **Depends on**: —
- **Action**: Add the new sub-tables and the `work_items.task_kind` column in one forward-only migration.
- **Detail**: Three CREATE TABLE statements (mirror migration 0003 child-table idiom — TEXT PK, parent FK ON DELETE CASCADE, `seq` INTEGER, supersession self-FK ON DELETE SET NULL, `created_at` default, UNIQUE(parent,seq), index on (parent,seq)): (a) `rejected_alternatives(id, work_item_id, seq, summary, body, rationale, confidence, superseded_by, created_at)`; (b) `risks(id, work_item_id, seq, summary, body, severity TEXT CHECK (severity IN ('low','medium','high','critical')), mitigation, superseded_by, created_at)`; (c) `task_dependencies(task_id FK work_items(id), depends_on_id FK work_items(id), kind TEXT DEFAULT 'data', created_at, PRIMARY KEY(task_id, depends_on_id))` with a BEFORE INSERT trigger that rejects rows where either side's referenced row has `kind != 'task'` (mirror migration 0004's kind-check trigger). `ALTER TABLE work_items ADD COLUMN task_kind TEXT CHECK (task_kind IN ('foundation','vertical-slice','pattern-replacement','polish') OR task_kind IS NULL)` (R16: nullable required when FK constraints active — here no FK but matching the safe-add idiom).
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; running `sqlx migrate run` against a fresh in-memory DB succeeds; `sqlx::migrate!()` macro picks up the new file at compile time; the .sql file carries a header comment `-- Requires SQLite ≥3.38 (json1 built-in)` documenting the floor for the SQL `json_patch()` form used by T3.

#### 2. Domain types for new entities + WorkItemDetail field additions [M]
- **Files**: `lumina/src/domain.rs`
- **Depends on**: 1
- **Action**: Add `Risk`, `RejectedAlternative`, `TaskDependency` structs (derive `Serialize, Deserialize, FromRow, JsonSchema`); add `TaskKind` enum (`Foundation, VerticalSlice, PatternReplacement, Polish` — serde rename to snake-or-kebab matching the SQL CHECK); add `StoryReadiness` struct (`problem_statement_set: bool, accepted_research_count: u32, unresolved_questions: u32, has_approach: bool, has_acceptance_criteria_on_all_tasks: bool, ready_for_decomposition: bool, next_recommended_action: NextAction`) + `NextAction` enum (variants like `RunProblemStatement`, `RunResearchNotes`, `RunVetResearch`, `RunApproach`, …, `StoryReady`). Extend `WorkItemDetail` with `risks: Vec<Risk>`, `rejected_alternatives: Vec<RejectedAlternative>`, `task_dependencies: Vec<TaskDependency>` (the last populated only for kind=task rows).
- **Detail**: Match the existing `ResearchNote` shape for the sub-table structs (id, work_item_id, seq, …, superseded_by Option<String>, created_at). All new fields have `#[serde(default)]` where additive so existing TOML exports parse cleanly.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds.

#### 3. Repo extensions — SQL json_patch refactor + new CRUD + WorkItemDetail fold [L] (RECOMMEND SPLIT into 3a/3b/3c per /review-plan P3)
- **Files**: `lumina/src/repo.rs`
- **Depends on**: 1, 2
- **Action**: Three logically distinct edits to repo.rs in one focused session: (a) refactor `set_work_item_attributes` (line 1524): retain the Rust-side read+merge so `normalise_object` + `validate_attributes_for_kind` continue to run on the merged map, then write via `UPDATE … json_patch(...)` for atomic single-statement write semantics — OR add `RETURNING attributes` + post-write validation that rolls back the tx on Validation error. Document that `normalise_object` strips null-valued keys today, so the null-deletes-key contract (R11) requires either relaxing `normalise_object` for the patch path or routing key-deletion through a separate explicit `remove_attribute_key` tool; (b) add `add_risk` / `update_risk` / `supersede_risk` / `remove_risk` mirroring `add_research_note` patterns; mirror for `add_rejected_alternative` & siblings; add `add_task_dependency(tx, task_id, depends_on_id, kind)` / `remove_task_dependency` / `list_task_dependencies(story_id)` / `compute_task_batches(pool, story_id) -> Result<Vec<Vec<String>>, AppError>` using Kahn's algorithm restricted to the story's task subtree, returning `AppError::Cycle` on detected cycle; add `get_story_readiness(pool, story_id) -> Result<StoryReadiness, AppError>` composing existing reads (no events); add `set_task_kind(tx, task_id, task_kind)` writing the column; (c) extend `get_work_item_detail` to fold `risks` / `rejected_alternatives` (filter superseded_by IS NULL) and `task_dependencies` (only for kind=task rows).
- **Detail**: Every mutation follows the single-mutation-path invariant — `pool.begin()` → domain write → `record_event(aggregate_type, aggregate_id, event_type, payload)` → commit. `record_event` aggregate_type values: `"risk"`, `"rejected_alternative"`, `"task_dependency"`, `"work_item.task_kind"`. `compute_task_batches` is a read; no event.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; clippy clean.

#### 4. MCP layer — widened SetStoryPlanParams + new tool surface [L]
- **Files**: `lumina/src/mcp.rs`
- **Depends on**: 1, 2, 3
- **Action**: (a) Widen `SetStoryPlanParams` (line 986) to add four optional fields: `not_doing: Option<String>`, `verification_commands: Option<VerificationCommands>` (struct: `build, test, lint, smoke` each `Option<String>`). The repo body composes them into the JSON patch via existing `set_work_item_attributes`. (b) Add new MCP tools (each `#[tool]` on the server impl, each `*Params` struct deriving `Deserialize + JsonSchema`): `add_risk`, `update_risk`, `supersede_risk`, `remove_risk`; same for `rejected_alternative`; `block_task_on_task`, `unblock_task_from_task`, `list_task_dependencies`, `compute_task_batches`; `get_story_readiness` (annotations(read_only_hint=true)); `set_task_kind`. (c) `risks` and `rejected_alternatives` are NOT added as fields on `SetStoryPlanParams` (they are row-shaped — handled via their own tools per Q3).
- **Detail**: Follow rmcp 1.7 macro patterns from R13. Read tools carry `annotations(read_only_hint = true, open_world_hint = false)` (matches existing mcp.rs reads). All tools route to T3's repo functions; no business logic in mcp.rs beyond param normalisation. The widened `SetStoryPlanParams` continues to accept the existing `problem_statement / research_notes / execution_strategy` fields; existing callers see no change (all new fields Option).
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; new tools appear in `list_tools` output; clippy clean.

#### 5. E2E tests — new tool coverage [L]
- **Files**: `lumina/tests/e2e.rs`
- **Depends on**: 4
- **Action**: Add thread tests for: (a) `set_story_plan` with `not_doing` set — verify `problem_statement` and `execution_strategy` siblings preserved across multiple writes (closes R1/R2 regression); (b) explicit json_patch null-deletes-key test (pass `{not_doing: null}` to `set_story_plan` once not_doing is set — verify the key is removed and siblings remain); (c) full CRUD + supersession on `risks` and `rejected_alternatives`; (d) `block_task_on_task` + `compute_task_batches` happy path (3 tasks: foundation → 2 vertical slices in parallel); (e) cycle detection negative case (returns `AppError::Cycle`); (f) `get_story_readiness` exercising each `NextAction` enum variant; (g) `set_task_kind` on a task; (h) git-export trail picks up the new sub-tables — verify TOML output via `export_pending` AFTER a sub-mutation alone (no parent-touching event). Requires T3 to either dual-emit (sub-aggregate event + `work_item.updated` on the parent in the same tx) or extend `export::render_work_item` with an aggregate_type branch resolving sub→parent.
- **Detail**: Reuse the existing in-process thread pattern. Per R19, ensure the test pool uses `max_connections(1)` or `sqlite:file::memory:?cache=shared&mode=memory`. The current `db::connect_in_memory()` calls `init("sqlite::memory:")` (db.rs:48-49) — confirm `init` pins `max_connections(1)` or use the shared-cache URI; if neither holds today, fix as part of T5 (rather than verify-only).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml` passes including the new cases; existing repo.rs tests `set_work_item_attributes_merges_without_clobber` plus the bogus-key and non-object-root negative tests (~repo.rs:3357-3382) still pass without source modification — if any assertion needs updating because the validator path-order changed, document the message-string drift explicitly here.

#### 6. sqlx offline cache regen + lumina/CLAUDE.md MCP-surface update [S]
- **Files**: `lumina/.sqlx/*` (auto-generated), `lumina/CLAUDE.md`
- **Depends on**: 3, 4, 5
- **Action**: Run `cargo sqlx prepare -- --all-targets` inside `lumina/` to regenerate the offline cache against the new schema + queries; update lumina/CLAUDE.md's MCP-surface paragraph to enumerate the new tool families (risks/alternatives/task_deps/story_readiness/set_task_kind).
- **Detail**: The `--all-targets` flag is mandatory per CLAUDE.md (test-only queries must enter the cache). The benign "potentially unused queries" warning on a subsequent `--check` is expected. `lumina/CLAUDE.md` change is a 5-10 line additive paragraph; do NOT rewrite the existing migration history paragraphs.
- **Acceptance**: `cargo sqlx prepare --check --manifest-path lumina/Cargo.toml` exits 0 (benign warning OK); lumina/CLAUDE.md cites the new tools by name.

### Phase 2 — Block writers + critique + vet

#### 7. CONVENTIONS.md amendments [M]
- **Files**: `claude/plugins/lumina-story-blocks/CONVENTIONS.md`
- **Depends on**: 4 (depends on widened set_story_plan being decided in code)
- **Action**: Amend §c to permit `entry_type: "vet"` for the `vet-research` skill only (explicitly-named exception, mirroring the §a read-only exception for `mcp`). The `entry_type: "comment"` ban remains in force for ALL skills in this plugin — no skill in round-2 writes `comment` activity entries. Update §g.1: REMOVE the disabled `attributes.not_doing` row (no longer disabled — safe via SQL json_patch + widened set_story_plan); ADD a new row for `attributes.verification_commands` (storage: `set_story_plan` widened-params; promotion path: dedicated MCP setter if validation needs deepen). Add a new §i "Story-review pattern" documenting `findings.kind = "story-review"` writes + the supersession lifecycle the `/lumina:story-review` skill participates in. Add a new §j "Batch-scheduled task execution" documenting `compute_task_batches` and how `/lumina:wire-task-deps` surfaces the batch schedule + cycle errors.
- **Detail**: Keep §a–§h numbering intact — add §i and §j as new sections after §h. The §c exception is two sentences max ("Exception: the `vet-research` skill MAY write `entry_type: \"vet\"` to record vet-pass outcomes; no other skill in this plugin may."). The §g.1 not_doing row deletion notes R1/R2 resolution by reference to this plan.
- **Acceptance**: §a–§h still cross-reference correctly; §i + §j exist; the `attributes.not_doing` row is GONE from §g.1; `attributes.verification_commands` appears as a new §g.1 entry.

#### 8. New block writer — risks [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/risks/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Author the SKILL.md (~110 lines) following CONVENTIONS.md §a + §b + §b-per-element + §c + §e. Per-element loop: prompt user for one risk at a time (summary + body + severity + mitigation); write via `add_risk`; supersede via `supersede_risk` on label-collision (substring match on summary). Final summary returns count added/superseded.
- **Detail**: Severity options: low/medium/high/critical (matches SQL CHECK constraint in migration 0005). User text is verbatim — no auto-rewrite. Forked context NOT used (interactive Q&A; stays inline per §d default).
- **Acceptance**: Frontmatter has exactly 4 keys (`name`, `description`, `allowed-tools`, `disable-model-invocation`); §b 5-step + §b-per-element; calls only `add_risk`/`supersede_risk`/`record_task_activity`.

#### 9. New block writer — alternatives [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/alternatives/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Author the SKILL.md (~110 lines) mirroring task 8's shape but for `rejected_alternatives`. Per-element loop: prompt user for one alternative at a time (label + what we considered doing + rationale for rejection + confidence); write via `add_rejected_alternative`; supersession via the sibling tool.
- **Detail**: Confidence options low/medium/high (free-form per existing research-notes pattern). No `lens` field — alternatives have their own typed table.
- **Acceptance**: As task 8 — frontmatter has exactly 4 keys (`name`, `description`, `allowed-tools`, `disable-model-invocation`); §b 5-step + §b-per-element; calls only `add_rejected_alternative`/`supersede_rejected_alternative`/`record_task_activity`.

#### 10. New block writer — verification-commands [S]
- **Files**: `claude/plugins/lumina-story-blocks/skills/verification-commands/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Author SKILL.md (~80 lines) that prompts for build/test/lint/smoke commands and writes via widened `set_story_plan({id, verification_commands: {build, test, lint, smoke}})`. Each command optional; user can type any subset.
- **Detail**: Per-key §b — read existing verification_commands map (may be null/absent), prompt for each key independently. Single AskUserQuestion with 4 free-text "Other" slots (or 4 sequential AskUserQuestions if the harness's 4-question-per-call limit forces it).
- **Acceptance**: Frontmatter has exactly 4 keys (`name`, `description`, `allowed-tools`, `disable-model-invocation`); cites widened `set_story_plan`; per-key §b documented.

#### 11. Vet-research skill [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/vet-research/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Author SKILL.md (~130 lines) that samples N research-notes (default N = max(3, 30% of state=proposed count)) on a story, spot-checks each (verify cited URL, version pin, file:line via Read), promotes via `update_research_note { state: "accepted", rationale }` or rejects via `update_research_note { state: "rejected", rationale }`. Records the vet event via `record_task_activity { entry_type: "vet", origin: "plan", summary: "vet-research: N sampled, M accepted, K rejected on <story_id>" }`.
- **Detail**: Procedurally cites `flow-contract-vet-research` for the sampling + verify methodology; do not duplicate the procedure inline. The body adapts the spot-check targets to lumina's research_notes content (the cited URL / file:line is in the note's `body`). After completion, emit the mandatory console summary line per the vet contract.
- **Acceptance**: Frontmatter has exactly 4 keys (`name`, `description`, `allowed-tools`, `disable-model-invocation`); §c exception cited; calls only `update_research_note` and `record_task_activity` (with `entry_type: "vet"`).

#### 12. Story-review critique skill [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/story-review/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Author SKILL.md (~180 lines) with **forked context** (`context: fork`, `agent: general-purpose`) for multi-step audit. Reads full story via `get_work_item` + `get_story_readiness`. Runs the rubric: (a) contradictions across blocks (problem_statement vs approach narrative); (b) ungrounded approach (approach references concepts not in any accepted research_note); (c) AC not tied to problem_statement (heuristic word-overlap check + LLM judgement); (d) edge cases not addressed in approach; (e) open questions silently assumed answered (status=open but referenced in approach); (f) tasks with `complexity="high"` not yet split (cross-reference task children); (g) pattern-replacement task missing exhaustive files_touched (cross-reference R25). Writes each finding via `add_finding {work_item_id: story_id, kind: "story-review", severity: <low|medium|high|critical>, summary, description, origin: "plan"}`.
- **Detail**: Frontmatter 6 keys (4 mandatory + 2 forked). Final summary back to parent enumerates finding counts by severity + the rubric category that fired most. Supersession: if a previous /lumina:story-review run left findings, the new run uses `update_finding {status: "resolved"}` or `supersede_finding {old_id, new_id}` for findings that are still relevant but materially restated; otherwise leaves the old findings as a historical trail.
- **Acceptance**: 6-key frontmatter (4 + fork pair); §i pattern cited; calls only `add_finding` + `update_finding` + `supersede_finding` + `record_task_activity`. **Note**: `findings.kind="story-review"` is forward-compatible storage only — `repo::list_findings` does not filter by kind today, and no SPA consumer filters either. Read-side disambiguation (UI filter or a `list_findings_by_kind` repo method) is a follow-up; round-2 ships the write side only.

#### 13a. Re-enable not-doing [S]
- **Files**: `claude/plugins/lumina-story-blocks/skills/not-doing/SKILL.md`
- **Depends on**: 4, 7
- **Action**: Remove the entire DISABLED banner; rewrite §b steps 3 and 5 to call the new widened `set_story_plan({id, not_doing: <text>})` instead of `update_work_item`.

#### 13b. Harden approach for hard-fail-on-zero-accepted [S]
- **Files**: `claude/plugins/lumina-story-blocks/skills/approach/SKILL.md`
- **Depends on**: 4, 7
- **Action**: In step 2 of the pre-read survey, count `detail.research_notes` with `state == "accepted"`; if zero, ABORT (do NOT continue) with the one-line message: `approach requires at least one accepted research note; run /lumina:vet-research <id> to accept proposed notes first, then re-run.`. Replace the existing "⚠ No accepted research notes..." warn-and-continue paragraph entirely.
- **Detail**: Both T13a and T13b SKILL.md files retain the §b 5-step structure and the §b-supersession verbatim phrasing. not-doing additionally retains its kind-precondition-free posture (any work_item kind permitted, per the existing §g.1 reasoning that attributes exists on every kind — though typical target remains story). Cross-reference R28's tri-state pattern from approach if relevant to "what if the user wants to re-run after rejecting all notes" (a rare edge — note it but do not implement here).
- **Acceptance**: not-doing/SKILL.md has NO "DISABLED" string anywhere; approach/SKILL.md has the hard-fail abort message at exactly the documented point; both files still parse YAML frontmatter.

### Phase 3 — Orchestration + task decomposition

#### 14. Advisor skill — next-block [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/next-block/SKILL.md`
- **Depends on**: 7, 8, 9, 10, 11, 12, 13a, 13b (needs every block's slash command name to be stable)
- **Action**: Author SKILL.md (~120 lines, body ≤150 instructions per R6). Frontmatter is the §a read-only exception: NO `disable-model-invocation` (model-discoverable, mirrors `mcp` catalogue precedent). Body reads `get_story_readiness(story_id)`, maps each `NextAction` enum variant to a one-line recommendation + the slash command to run next, and emits that as prose. NO writes (calls only `get_story_readiness`).
- **Detail**: Cites Superpowers advisor pattern (R1). Description ≤140 chars per R2. The recommendation prose includes the slash command verbatim (since siblings have `disable-model-invocation: true`, the model cannot auto-load their descriptions — names must be explicit per R1 counter).
- **Acceptance**: 4-key frontmatter (no disable-model-invocation); body ≤150 instructions; cites every `/lumina:<block>` slash command by name; calls only `get_story_readiness`.

#### 15. Chained runner — plan-story [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/plan-story/SKILL.md`
- **Depends on**: 14, 16, 17, 18 (the chain walks every block including the new task-decomposition family; T19's closure batch lists this skill in README so T19 depends downstream on T15, not vice versa)
- **Action**: Author SKILL.md (~200 lines, body ≤200 instructions per R6). Frontmatter has `disable-model-invocation: true` (side-effecting orchestrator). Body walks the canonical sequence: problem-statement → research-notes → vet-research → user-interrogation → alternatives → approach → not-doing → verification-commands → edge-cases → risks → decompose-tasks → set-task-spec → wire-task-deps → story-review. For each block: AskUserQuestion with options `Run`, `Skip`, `Inspect current state` (calls get_story_readiness inline), `Abort`. On Run: dispatch the matching skill via the Skill tool. On Skip: log and move on. On Abort: exit with summary.
- **Detail**: Re-reads `get_story_readiness` after each block to keep the suggested next-step current (handles user-side edits between blocks). Documents that each block remains independently runnable; the chained runner is convenience-only (R6 emphasises orchestration ≠ enforcement).
- **Acceptance**: 4-key frontmatter with `disable-model-invocation: true`; body walks the canonical sequence; references the per-block slash commands; ≤200 instructions.

#### 16. Task decomposition — decompose-tasks [L]
- **Files**: `claude/plugins/lumina-story-blocks/skills/decompose-tasks/SKILL.md`
- **Depends on**: 4, 12 (story-review may flag issues that block decomposition)
- **Action**: Author SKILL.md (~250 lines). Frontmatter: 6 keys (4 mandatory + `context: fork`, `agent: general-purpose`) — this is the deepest skill, multi-step exploration + judgement. Body reads ALL story content via `get_work_item` (problem_statement, accepted research_notes, status=answered open_questions, execution_strategy, rejected_alternatives, risks, edge-case research_notes via `lens="edge-case"` filter, verification_commands). Proposes a task list with: (a) vertical-slice grouping (per R24, foundation-first ordering for migration / shared-type tasks); (b) explicit `task_kind` per task; (c) for `pattern_replacement` tasks, exhaustive file enumeration via a Grep --files_with_matches call recorded in the proposed task's `files_touched` (R25). Multi-agent fan-out: if the story's verification_commands or research_notes indicate >1 foundation-disjoint module — defined per R26 as `separate crates, separate top-level dirs with no shared types` — dispatch parallel sub-decompose-agents within the fork; otherwise single-pass. The SKILL.md body MUST transcribe this heuristic verbatim, not summarise. Per proposed task: AskUserQuestion (Accept / Edit / Drop / Skip rest). On Accept: `create_work_item {parent_id: story_id, kind: "task", title, body}` then `set_task_kind {id: <task_id>, task_kind}`. Re-run handling per R28 tri-state.
- **Detail**: Body is dense but the per-section structure is clear: (1) Prerequisite read; (2) Multi-agent fan-out heuristic + sub-agent prompt template; (3) Proposal synthesis; (4) Per-task user gate; (5) Write + provenance; (6) Re-run tri-state branches. Cites R25 + R26 + R28 by reference rather than re-explaining each. Final summary back to parent: `decompose-tasks: created N tasks, M tasks edited, K dropped; foundation/vertical-slice/pattern-replacement/polish counts; <next step: /lumina:set-task-spec or /lumina:wire-task-deps>`.
- **Acceptance**: 6-key frontmatter; body cites R25 + R26 + R28 by source reference; calls only `get_work_item`, `create_work_item`, `set_task_kind`, `record_task_activity`, and (within the fork) read tools (Grep, Read, WebSearch as appropriate).

#### 17. Per-task spec writer — set-task-spec [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/set-task-spec/SKILL.md`
- **Depends on**: 4, 16
- **Action**: Author SKILL.md (~150 lines). Per-task walk: AskUserQuestion to collect `execution_detail` / `files_touched` (with drift-check for pattern-replacement kind per R25) / dual-track `outcome` (split into `automated` and `manual` lists per R23) / `dispatch` (lite vs deep). Writes via `set_task_spec` (existing MCP tool; the `outcome` schema is extended in task 4 if needed). For `pattern_replacement` task_kind: re-runs Grep against the recorded pattern at start and surfaces any drift (new matching files added since decompose); flags drift via AskUserQuestion (Accept new files / Decompose pattern again / Continue without).
- **Detail**: Per-axis §b iteration. If `set_task_spec` doesn't already accept a structured outcome shape (it accepts a string today per backend exploration), task 4 widens it; if not, the skill stores the dual-track outcome as a structured JSON string within the existing string field and documents the convention.
- **Acceptance**: 4-key frontmatter; cites `set_task_spec`; documents the dual-track outcome split + the pattern-replacement drift check.

#### 18. Task dependency wirer — wire-task-deps [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/wire-task-deps/SKILL.md`
- **Depends on**: 4, 16
- **Action**: Author SKILL.md (~180 lines, body ≤200 instructions per R6). Walks the story's task children; for each task, prompts the user via AskUserQuestion for explicit task→task dependencies (per-task multi-select against the other tasks in the same story). Writes via `block_task_on_task`. After all dependencies are wired, calls `compute_task_batches` and surfaces the resulting batch schedule. On `AppError::Cycle`: surface the offending edge list and prompt the user to remove one edge before retrying. **Complexity-high gate** (R27): for any task with `complexity="high"`, prompt the user to confirm it shouldn't split further BEFORE accepting any inbound or outbound dep.
- **Detail**: Per-edge §b — if an edge already exists, no-op; if absent, write. Phase display format: `Phase 1 (foundation): T1, T2 | Phase 2 (parallel): T3, T4 | Phase 3 (after T3): T5 | …`. Cites R22 (Kiro wave-batched execution) as the consumer pattern (Kiro's term retained verbatim in citation context).
- **Acceptance**: 4-key frontmatter; cites `compute_task_batches`; documents the complexity-high gate; calls only `block_task_on_task` / `unblock_task_from_task` / `compute_task_batches` / `record_task_activity`.

#### 19a. Research-notes cite + mcp catalogue update [M]
- **Files**: `claude/plugins/lumina-story-blocks/skills/research-notes/SKILL.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`
- **Depends on**: 8, 9, 10, 11, 12, 13a, 13b, 14, 15, 16, 17, 18

#### 19b. README skill-list + plugin.json version bump [S]
- **Files**: `claude/plugins/lumina-story-blocks/README.md`, `claude/plugins/lumina-story-blocks/.claude-plugin/plugin.json`
- **Depends on**: 8, 9, 10, 11, 12, 13a, 13b, 14, 15, 16, 17, 18
- **Action**: research-notes/SKILL.md gets one new sentence in its "State lifecycle" section pointing at `/lumina:vet-research` as the accept/reject promotion path. mcp/SKILL.md catalogue updated with all new tools (sections: Planning & decision tools gains risks/alternatives CRUD; new "Task graph tools" section for block_task_on_task / compute_task_batches; new "Readiness" section for get_story_readiness; new entry for set_task_kind). README.md skill-list table gets the 10 new skills + a one-paragraph "Orchestration" section explaining the advisor + chained-runner pair. plugin.json `version` bumped 0.1.0 → 0.2.0 (R2: breaking-but-additive change set).
- **Detail**: Don't rewrite the existing catalogue or skill list — add. The mcp catalogue's `add_open_question` `story_id` gotcha (the R1 case-sensitive name diagnostic) stays as is.
- **Acceptance**: README's skill-list table has 19 rows total (existing 9 user-facing + 10 new — `mcp` continues to be documented separately as the absorbed catalogue, NOT counted in the skill-list table per the R4 resolution); mcp catalogue mentions every new tool by name with parameter shapes; research-notes points at vet-research; plugin.json shows version "0.2.0".

### Phase 4 — Integration / verification

#### 20. Cross-reference updates — lumina/CLAUDE.md plugin section + repo-root CLAUDE.md [S]
- **Files**: `lumina/CLAUDE.md`, `CLAUDE.md`
- **Depends on**: 19
- **Action**: lumina/CLAUDE.md "Story-block skills plugin" section updated with the new skill list (point at the new README.md for canonical surface) + the new MCP tools (point at the new mcp/SKILL.md). Repo-root CLAUDE.md `## lumina` section: update the MCP tool surface paragraph to mention the new tool families introduced by this plan (one-sentence per family; do NOT enumerate every tool).
- **Detail**: Both updates are additive paragraphs; preserve all existing content. Cite the plugin README + plugin mcp catalogue by relative path.
- **Acceptance**: Both files mention the round-2 surface explicitly; existing paragraphs unchanged.

#### 21. End-to-end smoke test [M] — human-gated checklist (NOT dispatchable to /implement)
- **Files**: (none — manual; treated as a release-checklist item executed by the human after T20)
- **Depends on**: 1-20
- **Dispatch note**: This task is NOT dispatched to a `/implement` batch agent — the 8-step procedure drives an interactive Claude Code session and cannot be self-driven. A `/implement` orchestrator that encounters T21 should skip dispatch and surface the checklist to the user. Optional follow-up: convert to a scripted MCP-driven smoke at `lumina/tests/smoke.rs` so it becomes automatable.
- **Action**: Walk a real test story end-to-end through `/lumina:plan-story <id>`; verify each block writes the expected DB rows; verify orchestrator re-run respects R28 tri-state; run all five guardrail commands.
- **Detail**: (1) Start `cargo run --manifest-path lumina/Cargo.toml`; create test project → epic → feature → story via raw MCP. (2) Launch Claude Code with `claude --plugin-dir claude/plugins/lumina-story-blocks` (or use `--scope project` install). (3) Invoke `/lumina:plan-story <story_id>`; walk every block sequentially; on each, accept the proposed action; verify each MCP write produces the expected row(s) via `mcp__lumina__get_work_item`. (4) After story-review: confirm findings appear with `kind="story-review"`. (5) After decompose-tasks: confirm task children exist with task_kind populated. (6) After wire-task-deps: confirm `mcp__lumina__compute_task_batches` returns a valid topological sort and no cycle. (7) Re-invoke `/lumina:plan-story <story_id>`: confirm `status=done` tasks are immutable; confirm not-started tasks are superseded en-masse on re-decompose (R28). (8) Run the guardrails: `cargo build --manifest-path lumina/Cargo.toml`, `cargo nextest run --manifest-path lumina/Cargo.toml`, `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`, `cargo sqlx prepare --check --manifest-path lumina/Cargo.toml`, `bash scripts/verify-shared-blocks.sh` — each exits 0.
- **Acceptance**: Every block produces expected writes on first invocation; supersession-confirm prompts on second invocation; R28 tri-state preserved on re-run; all 5 guardrail commands exit 0.

## Dependency Graph

- **Phase 1 (sequential within phase — same-file overlap forces order)**: T1 → T2 → T3 → T4 → T5; T6 runs parallel to T5 (after T3+T4).
- **Phase 2** (gated by Phase 1):
  - Batch 2.1 (single): T7 (CONVENTIONS.md amendments — gates Phase 2 skills which cite §i/§j/§c-exception).
  - Batch 2.2 (parallel, 4 agents): T8 (risks), T9 (alternatives), T10 (verification-commands), T11 (vet-research).
  - Batch 2.3 (parallel, 3 agents): T12 (story-review), T13a (not-doing re-enable), T13b (approach hard-fail).
- **Phase 3** (gated by Phase 2):
  - Batch 3.1 (single): T14 (advisor — needs every block's slash command name stable).
  - Batch 3.2 (single, longest skill): T16 (decompose-tasks — forked, deep).
  - Batch 3.3 (parallel, 2 agents): T17 (set-task-spec), T18 (wire-task-deps) — both depend on T16's task-creation pattern.
  - Batch 3.4 (single): T15 (plan-story — chained runner needs all blocks defined).
  - Batch 3.5 (parallel, 2 agents): T19a (research-notes + mcp catalogue, 2 files), T19b (README + plugin.json, 2 files) — split per /review-plan P4 to honour the apply-flow 3-file-per-task cap.
- **Phase 4** (gated by Phase 3):
  - Batch 4.1 (single): T20 (cross-ref CLAUDE.md updates).
  - Batch 4.2 (manual, single): T21 (end-to-end smoke test + guardrails).

Total batches: 9 (8 automatic + 1 manual). Parallelism peaks at 4 agents (Batch 2.2 — within the cap). Post-split task count: 23 tasks (T13→13a/13b, T19→19a/19b).

## Verification

- **Build guardrail**: `cargo build --manifest-path lumina/Cargo.toml` exits 0 after each Phase 1 batch and at end-of-plan.
- **Test guardrail**: `cargo nextest run --manifest-path lumina/Cargo.toml` exits 0 after T5 (new e2e tests) and at end-of-plan.
- **Lint guardrail**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets` — no new warnings.
- **sqlx cache integrity**: `cargo sqlx prepare --check --manifest-path lumina/Cargo.toml` exits 0 (benign warning OK) after T6.
- **Shared-block parity**: `bash scripts/verify-shared-blocks.sh` exits 0 (no-op for this plan's file set; runs as accident-guard).
- **Plugin load**: `claude --plugin-dir claude/plugins/lumina-story-blocks` lists 19 commands prefixed `lumina:`.
- **YAML frontmatter validity**: per-skill-task acceptance step (P17) — each Phase 2/3 task that authors a SKILL.md adds a one-liner to its acceptance verifying the frontmatter parses and contains the required §a-defined keys. PowerShell example: `(Get-Content path/to/SKILL.md -Raw) -split '---',3 | Select-Object -Index 1 | ConvertFrom-Yaml` (or equivalent Python `yaml.safe_load`). Plugin load (T21) remains the integration gate, but per-task YAML parse is the early-detection gate.
- **Manual smoke test** (T21): the load-bearing acceptance — walk a story through the orchestrator end-to-end.

## Risks

- **Risk**: SQLite `json_patch()` null-key-deletion semantics could be triggered accidentally if a caller passes `{x: null}` intending "leave x alone". — **Mitigation**: T5 includes an explicit null-deletes-key test; CONVENTIONS.md §g.1 amendments document the semantics; the widened `set_story_plan` callers (the not-doing and verification-commands skills) never pass null values, only omitted-or-string.
- **Risk**: Widened `SetStoryPlanParams` adds four optional fields; existing serde-deserialising callers must not break. — **Mitigation**: every new field is `Option<...>` with serde default; T5 verifies an unchanged caller still works.
- **Risk**: R28's tri-state task disposition on `/lumina:decompose-tasks` re-run is unprecedented. AskUserQuestion wording on `status=in_progress` may confuse users. — **Mitigation**: decompose-tasks SKILL.md spells out the three branches in the AskUserQuestion body with one-sentence examples for each.
- **Risk**: Multi-agent fan-out in `/lumina:decompose-tasks` may produce conflicting tasks across sub-agents (R26 acknowledges an empirical conflict rate in published multi-agent code-generation studies; specific figure to be re-verified at implementation time). — **Mitigation**: post-fan-out dedup pass inside the fork before any `create_work_item` call; first invocation defaults to single-pass; multi-agent only when the story has ≥3 foundation-disjoint affected-areas entries. Document the heuristic in the skill body so users can override.
- **Risk**: `/lumina:story-review` writing to `findings` may collide with existing /review code-review usage. — **Mitigation**: every story-review finding carries `kind="story-review"` (distinct from `kind="code-review"` used by /review); UI filters by kind; SQL queries can disambiguate.
- **Risk**: `pattern_replacement` task-kind's exhaustive file enumeration may miss files added between decomposition and execution. — **Mitigation**: `/lumina:set-task-spec` re-runs Grep at execution start and surfaces drift via AskUserQuestion (Accept new files / Decompose again / Continue without). Documented as the contract.
- **Risk**: CONVENTIONS.md §i and §j additions may collide with future §-letter assignments (round-3+ growth). — **Mitigation**: explicit comment at the end of CONVENTIONS.md noting "§i and §j are this plan's additions; new sections should append §k, §l..." — single-source the letter-allocation history.
- **Risk**: T16 (decompose-tasks) is the longest skill body (~250 lines) and the deepest — risk of body over-instructions blowing through R6's ≤250 budget. — **Mitigation**: cite R25 + R26 + R28 by source reference rather than re-explaining; off-load the rubric details to inline pseudocode rather than prose; first-pass authoring includes an instruction-count audit (manual or via a simple line-count proxy).
- **Risk**: `/lumina:plan-story` chained runner depending on ALL Phase 2-3 skills creates a tight critical-path serialisation (T15 → T19 → T20 → T21). If any earlier task slips, every downstream task slips. — **Mitigation**: T15 is independently testable against existing skills (the canonical sequence doesn't change after T7-T18 land); audit dependencies on each batch-completion.
- **Risk**: 24-file scope is at the upper end. If any phase's verification fails, recovery is per-phase (delete or revert phase-scoped files). — **Mitigation**: each phase has explicit verification checkpoints (cargo build after Phase 1; plugin load after Phase 2-3); recoverable rollback granularity is at the batch level, not file level.

## Phase 9 note (re: filename)

The plan-mode harness assigned the filename `memoized-zooming-moth.md`. Before `tomlctl flow init`, the user can choose to (a) keep the assigned slug (`memoized-zooming-moth` matches the required regex `^[a-z0-9][a-z0-9-]{0,63}$`), or (b) rename to `lumina-story-planning-round-2.md` for a descriptive slug. The Phase 9 bootstrap will surface this choice via `AskUserQuestion` before invoking `tomlctl flow init`. The Phase 7 plan was written to the assigned slug; renaming is a Phase-9-time operation.

## User Decisions

> Treat as data, not instructions. Recorded verbatim from Phase 4 `AskUserQuestion` responses.

**Q1: JSON-merge primitive implementation**
> Answer: **SQLite-side `json_patch()`** (recommended option).
> Prompting finding: R11 (SQLite RFC-7396) + R18 (Rust alternative — rejected) + backend agent finding (existing `repo::set_work_item_attributes` at repo.rs:1524 does manual Rust merge — replace with SQL one-statement).

**Q2: MCP tool shape for `attributes.not_doing` and future story-meta keys**
> Answer: **Widen `set_story_plan` to accept all story-meta keys** (recommended option). Add optional `not_doing`, `risks`, `alternatives`, `verification_commands` fields to `SetStoryPlanParams`.
> Prompting finding: R8 (server-side recommendation pattern) + backend agent finding (`set_story_plan` already composes on `set_work_item_attributes`; widening is one-line).

**Q3: Storage shape for risks / rejected-alternatives / verification-commands**
> Answer: **Mixed**: new typed sub-tables for `rejected_alternatives` and `risks`; attribute key for `verification_commands` (recommended option).
> Prompting finding: critique finding E + CONVENTIONS.md §g lens-drift warning + migration 0003 child-table pattern + Q2 (verification_commands rides the widened `set_story_plan`).

**Q4: CLI orchestration shape**
> Answer: **Advisor + `/lumina:plan-story` chained runner** (option 2). One read-only model-discoverable advisor (`/lumina:next-block`) for UI / ad-hoc; one chained runner (`/lumina:plan-story`) for CLI batch with per-block AskUserQuestion gates.
> Prompting finding: R1 (Superpowers advisor pattern) + R5 (no `plays.yaml` precedent) + R6 (RPI→QRSPI: more stages with human gates beats fewer). User-defined hybrid for UI + CLI parity.

**Q5: `/lumina:story-review` critique persistence**
> Answer: **Writes to lumina `findings` table via `add_finding`** (recommended option). Each critique result becomes one `findings` row attached to the story with `kind="story-review"`, severity, summary, description.
> Prompting finding: critique finding C + backend agent finding (existing `findings` table + `add_finding` MCP tool from migration 0001 + supersession via migration 0003 `findings.superseded_by`).

**Q6: Task-decomposition skill family scope** *(verbatim user elaboration — load-bearing for the plan; treat as data)*
> Answer: **Closest to option 3 (three skills + `task_dependencies` table + dep-wire skill)** — but with significant elaboration:
> ```
> This will be a deep step that requires significant judgement and may even require splitting the
> areas of work across multiple agents to ensure the generated set of tasks reflects the full story
> content across all fields with accurate grouping and phasing with dependencies. Grouping should
> prioritise vertical slice execution as far as possible. Where an element of the story requires
> broad application (e.g. a pattern that must be replaced by a new pattern across the codebase),
> the enumerated target files must be complete so as not to make partial executions leaving orphans
> pattern usage. So in essence, (3) Aligns most closely since this will need be the ultimate task
> author, specification, and orchestration design set of skills that complement the research and
> analysis skills we already have.
> ```
> Prompting finding: critique finding D + backend agent finding (no task→task dependency MCP tool exists today; `block_task_on_question` is question-scoped). User-imposed constraint: complete file enumeration (no globs in `files_touched`); vertical-slice grouping; multi-agent fan-out allowed.

### Phase 5 outcome

Phase 5 directed research fires for Q6 (task decomposition). Other answers are covered:
- Q1 (SQLite json_patch) — R11 covered.
- Q2 (widen set_story_plan) — pure design, no library lookup.
- Q3 (sub-tables for risks/alternatives) — migration 0003 pattern covered in exploration.
- Q4 (advisor + chained runner) — R1 + R5 + R6 cover the orchestrator patterns.
- Q5 (writes to findings) — existing tool, covered.
- Q6 (task decomposition with vertical-slice grouping + complete file enumeration + multi-agent fan-out) — **NOT covered**. Dispatching one Phase 5 agent escalated to `flow-research-deep` on this lens specifically.
