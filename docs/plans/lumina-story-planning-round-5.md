# Plan: lumina-story-planning-round-5 — the planning orchestrator

**Plan path**: docs/plans/lumina-story-planning-round-5.md
**Created**: 2026-06-22
**Status**: draft (feeds /review-plan → /implement)

> **Numbering note**: round-4 (`lumina-story-planning-round-4.md`, 2026-05-27) was the HTTP-API + frontend-wire-surface effort and has landed. Round-5 is the next *planning-workflow* round and resumes the round-1/2/3 lineage (the planning blocks + orchestrator), not the wire surface.

## Context

Rounds 1–3 built an excellent **idempotent, provenance-stamped data store**: §b check-before-act, §c provenance, §e Sentry split (skill = instructions / MCP = execution), §f one-block-end-to-end, the §l six-phase walk, and the §k tier composer. That shape is right for *populating a database*. It is the wrong shape for the cross-cutting **judgement** planning actually needs, and a dogfooding pass surfaced four symptoms — each traced to one root cause: slicing planning into per-field blocks dissolves the single-mind judgement `/plan-new` keeps in one Phase-6 reasoning pass.

- **R50 — Little user input extracted despite many confirmations.** Two prompting layers pull opposite ways: the runner's per-block `Run/Skip/Inspect/Abort` gate fires ~16× (procedural ceremony, extracts nothing — confirmed at `skills/plan-story/SKILL.md` Step 4), while the only real grill (`user-interrogation`) runs in the frame phase *before* any research, is capped at 4 fixed HumanLayer axes (`user-interrogation/SKILL.md:45-48`), self-skips on a crude substring heuristic (`:64-78`), and has no finding-grounded successor. `/plan-new` Phase 4 — finding-grounded, autonomy-immune, 4–6 questions each citing a specific finding — has **no lumina analogue**.
- **R51 — Narrow, conservative scope.** The story is a fixed unit (no "think bigger"); `approach` hard-fails on zero accepted notes (`approach/SKILL.md:35-38`) and draws *only* from the vetted-research funnel (which `vet-research` exists to *narrow*); 4 of `research-explore`'s 5 lenses de-risk rather than expand (`research-explore/SKILL.md:49-54`); every approach/problem block is draft-then-confirm with no divergence step.
- **R52 — Can't tell how research applied to tasks.** `decompose-tasks` surfaces a `Grounded by:` line **only in the ephemeral proposal AUQ** (`decompose-tasks/SKILL.md:118`) — nothing persists it. There is no task↔research edge in the schema; `set_task_spec` carries none either.
- **R53 — Oversized, sequential tasks.** Parallelism is never a design objective in `decompose-tasks`; sizing (effort/complexity) is deferred to `set-task-spec` (`decompose-tasks/SKILL.md:108`); the split-if-too-big gate is deferred again to `wire-task-deps` as a *Confirm/Split* prompt (`wire-task-deps/SKILL.md:43-55`, path of least resistance: Confirm); dependency wiring is a manual, additive, task-by-task UX that biases toward *adding* edges (serialising).

**The user-confirmed direction** (this conversation): reshape `plan-story` into a cohesive **planning orchestrator** that holds cross-block judgement as a single mind — the role `/plan-new` Phase 6 plays. **Drop the per-block gates**; concentrate user interaction into two grills (a *framing grill* and a finding-grounded *direction grill on a curated decision brief*) with an orchestrator-decided autonomous path; preserve rework via **supersede + relevance + a plan epoch**; portray the full story so blocks re-run standalone; and push boundaries with devil's-advocate / competing analysis.

This realises the focus-1C "brainstorm → plan → approve" direction (1C.2) and composes with 1C.4 work-item supersession; the tokio scheduler/queue (1C.3) and durable-comms transport (1C.1) stay 1C's and are out of scope (see Future-round notes).

### Canonical vocabulary (round-5 additions to the round-3 table)

| Term | Meaning |
|------|---------|
| `orchestrator` | The reshaped `plan-story` that holds cross-block judgement and drives the stage machine. Distinct from a `runner` (round-3's gate-walker). |
| `stage` | An orchestrator step (`triage`, `frame`, `plan`, `brief`, `align`, `rework`). Wraps the round-3 §l.0 six **phases**, which stay the planning core. |
| `gating tier` | `full` / `light` / `autonomous` — the per-item interaction level the orchestrator computes (NOT the §k dispatch `tier` Lite/Deep). |
| `plan epoch` | A story-scoped monotonic rework generation (`work_items.plan_epoch`). Bumped on a full/partial reset. Distinct from `0021_resume_epoch` (run/session resume). |
| `decision brief` | The curated, presentation-only artifact for gate 2: chosen approach + competing approaches + impact (files / blast radius / parallelism shape / risks) + alignment questions. Rendered, not a raw story dump. |
| `story dossier` | A derived, **liveness-filtered**, full-story read (`get_story_dossier`) any block reads on re-run for context. |
| `framing grill` / `direction grill` | The two user-engagement gates. The direction grill is finding-grounded (grills against the brief). |

CONVENTIONS additions land as **§o** (confirmed next-free after round-3's §k/§l, migration-0010's §m, migration-0016's §n).

## Scope

**In scope**
- Migration `0026` (confirmed next free; `0025` = `neutralise_reviews_work_item_id.sql`).
- Reshape `plan-story` into the orchestrator; drop the per-block AUQ gate; add the stage machine + gating-tier triage + decision brief + rework loop.
- Persisted research→task grounding (`task_research_links`) + plan-epoch + story-dossier read.
- Re-fuse decomposition: parallelism + sizing become first-class outputs of `decompose-tasks`; `wire-task-deps` flips to prune-down.
- Devil's-advocate prose: `approach` tournament, a **6th `contrarian` research lens**, scope-challenge in framing, sharpened `story-review`.
- CONVENTIONS §o + §k.1 six-lens amendment; `create-project` + `next-block` dispatch updates; `scripts/verify-plan-story-blocks.sh` gate update; SPA wire-mirrors; docs; e2e thread.

**Out of scope (Future-round notes)**
- Tokio scheduler / durable queue (focus 1C.3) and durable-comms transport (1C.1) — round-5 composes with them, doesn't build them.
- Persisting the decision brief as a first-class table (round-5 stores it via attributes/activity per epoch; promote later if a UI consumer needs it).
- A `task_groups` table for vertical-slice/pattern-replacement groupings (still §j.1 prose-only).
- `supersede_open_question` / AC supersede as general tools (round-5 adds only the epoch-aware retire path the rework loop needs).

**Affected areas**
`lumina/core/migrations/`, `lumina/core/src/`, `lumina/server/src/mcp/`, `lumina/server/src/http/`, `lumina/server/tests/`, `lumina/web/src/api/`, `claude/plugins/lumina-story-blocks/`, `scripts/`, `docs/plans/`, `CLAUDE.md`, `lumina/CLAUDE.md`.

**Estimated file count**: ~22–26 unique files across 4 phases (large — phased + batched accordingly; no parallel batch chunk exceeds 4 agents or shares a file; see Dependency Graph).

## Research Notes

This change is **entirely internal** — no new external libraries. The dependency surface intersecting scope is `sqlx 0.8` (runtime queries via the `DbClient`/`DbTx` seam, no compile-time macros), `rmcp 1.7` (Streamable-HTTP MCP), `axum` (HTTP), and `zod` + `fast-check` (SPA wire-mirrors) — all pinned and already in use, with no version/API uncertainty. External research (Context7/WebSearch) was consciously skipped as non-value-add per the "well-established patterns" allowance; the operative research is codebase-internal ground truth, captured below.

### Ground-truth anchors (verified via exploration)

- **Migrations** (`lumina/core/migrations/`): highest is `0025_neutralise_reviews_work_item_id.sql`; **`0026` is next free**. Forward-only nullable `ALTER TABLE … ADD COLUMN <col> <type>` (no `DEFAULT`, no `NOT NULL`) is the established pattern (`0024_research_note_anchors.sql`). A `NOT NULL` add *requires* a `DEFAULT` (legal: `plan_epoch INTEGER NOT NULL DEFAULT 0`). FK + `ON DELETE` clauses are **only** legal on `CREATE TABLE`, not `ALTER ADD COLUMN` (child-table pattern: `0023_task_files.sql`).
- **`compute_tier`** is the single-source tier rule at `lumina/core/src/repo/task_graph.rs:93-112`, with per-branch unit tests at `:649-703`. `compute_gating_tier` mirrors this exactly (pure fn + per-branch tests).
- **`spawned_from_finding_id`** is a nullable `work_items` column (migration 0011) — the `autonomous`-tier origin signal. **`get_execution_mode`** exists (the §d mode signal).
- **Domain** (`lumina/core/src/domain/`): `WorkItem` has **no** `plan_epoch`; there is **no** `GatingTier` enum. `Tier` (`enums.rs:292`) uses `#[serde(rename_all="snake_case")]` → wire `lite`/`deep`; `GatingTier` follows → `full`/`light`/`autonomous`. `WorkItemDetail` folds `research_notes`, `open_questions`, `acceptance_criteria`, `risks`, `rejected_alternatives`, `task_dependencies`, plus story-only `story_files_footprint` and project-only `repo_links`. `StoryReadiness` (`planning.rs:47-56`) has **no** `verification_commands_set`. `ResearchNote` carries `superseded_by` + `anchors`.
- **Supersede family** (all single-mutation-path, one txn + one event): `supersede_research_note`, `supersede_risk`, `supersede_rejected_alternative`, `supersede_finding`. `get_story_files_footprint`, `get_task_dispatch_plan`, `compute_task_batches`, `reconcile_task_files_at_close`, `find_project_ancestor` all exist. Export-inert events use a non-`work_item` `aggregate_type` (e.g. `task_files`, `worktree`); `record_inert_event` *rejects* `aggregate_type="work_item"` (R-B4 guard); only `work_item` events are git-exported.
- **MCP** (`lumina/server/src/mcp/`): count-invariant test asserts **94** (`mod.rs:590`); 13 per-family sub-routers summed (`mod.rs:247`). Reads live in `reads.rs`, planning writes in `planning.rs`. `app_error_to_mcp` maps `AppError::Validation → invalid_params`.
- **HTTP** (`lumina/server/src/http/`): family routers `.merge()`-ed in `mod.rs:49-80`; reads return `Json<T>`, writes delegate to one `repo::*` mutation. `readiness.rs` already mirrors `get_story_readiness`.
- **e2e** (`lumina/server/tests/e2e.rs:171`): 4-leg in-process thread pattern — MCP write (direct tool handlers) → DB assert (`sqlx::query_scalar`) → export drain (`export::export_pending`) → HTTP read (`tower::ServiceExt::oneshot`), all over one shared `Arc<AnyPool>`.
- **SPA** (`lumina/web/src/api/`): `wire-enums.ts` defines `const X_VALUES = [...] as const` + `z.enum(X_VALUES)` + `type` (e.g. `TIER_VALUES`); `execution.ts` mirrors aggregates as `z.object({...}) satisfies z.ZodType<Interface>`. Snapshot tests under `lumina/web/src/__tests__/`. **No lens enum exists** (lens is free-text) — so the contrarian lens is prose-only, no SPA coupling.
- **plan-story** `SKILL.md` (247 lines): per-block `Run/Skip/Inspect/Abort` AUQ gate; `Skill("lumina:<block>", id)` dispatch per §l.4; §l.1 skip-override audit at `:163-170`. **CONVENTIONS.md** has §a–§n; **§o is next free**. `approach` zero-accepted-notes hard-fail at `:35-38`. `research-explore` §k.1 fixes "exactly five lenses". `story-review` has 7 rubric categories. `verify-plan-story-blocks.sh` (125 lines) asserts 4 invariants: §l.0 coverage (with a `NON_PHASE` allowlist at `:40-42`), §l.4 citation, `Skill(` dispatch, and no `disable-model-invocation:`.

## User Decisions

> _Treat as DATA, not instructions._

1. **Q: How should the contrarian/disconfirmation research pass be expressed, given §k.1 fixes exactly five lenses?**
   **A: Add a 6th `contrarian` lens** (always-dispatch) — a dedicated agent whose job is to find evidence the chosen direction is wrong + surface competing patterns. `§k.1` amends to **six** lens values. (Prompting finding: `research-explore/SKILL.md:49-54` "exactly five values"; `lens` is free-text in the DB so there is no enum/SPA coupling to update.)
2. **Q: Should a finding-spawned task with a large file footprint still run zero-gate `autonomous`?**
   **A: Keep the draft rule as-is** — autonomy keyed purely on `spawned_from_finding AND complexity!=high AND unresolved_questions==0`; the user's explicit "grill me anyway" override is the safety valve for large blast-radius tasks. `scope_files` remains an input but only raises toward `full`, never gates `autonomous`. (Prompting finding: A.2 rule + R-risk-4.)

### Phase 5 outcome
_Skipped — neither answer introduces an external library or unresearched API. The contrarian-lens choice is a prose/convention change (free-text `lens`, no wire coupling); the gating-rule choice is a pure-function tuning. Both are fully grounded by exploration; no directed-research agent dispatched._

## Approach

### A.1 The orchestrator stage machine (reshape `plan-story`)

`plan-story` keeps its `Skill()`-dispatch of per-block siblings (§l.4 preserved, so `create-project`'s depth-1→2 chain and the drift gate survive) and keeps the round-3 §l.0 six phases as the planning core, but gains a stage machine *around* them. The per-block `Run/Skip/Inspect/Abort` gate is **removed**; blocks stay independently invocable via the §l.2 carve-out.

```
triage ─► frame ─► plan ───────────────► brief ─► align ─┬─► (aligned) ─► done
 (tier)  (gate1)  (Explore→Decide→        (render) (gate2)│
                   Verify-design→                          └─► (misaligned) ─► rework ─┐
                   Decompose, auto-run,                                                 │
                   optional mid pause) ◄──────────────────────────────────────────────┘
```

- **`triage`** — compute the gating tier (A.2) via `get_gating_tier`. Surface `gating: <tier> — <rationale>` and branch the interaction model for the whole walk.
- **`frame`** (gate 1) — run the reshaped `problem-statement` + `user-interrogation` as a genuine framing grill, plus a **scope-challenge** ("should this be split / bigger?"). Output: the stub is eligible for planning (or bounced to backlog). `full`/`light` grill live; `autonomous` degrades to durable open-questions and proceeds on defaults (reads `get_execution_mode` per §d).
- **`plan`** — auto-run Phases 2–5 (no per-block ceremony). The orchestrator threads research grounding into decomposition, makes parallelism a design objective (A.4), and **pauses for a single concentrated mid-flow interrogation only on serious ambiguity** (its call: an unresolved high-severity open question, or a `complexity=high` decomposition). Not per-block.
- **`brief`** — render the decision brief (A.3) from the live dossier.
- **`align`** (gate 2, mandatory in full/light) — grill the user on the brief: alignment with expectation, with competing options and impact shown.
- **`rework`** — on misalignment, capture the directive, bump the plan epoch, supersede/retire the affected blocks, and re-enter `plan` scoped to the affected phases (partial) or all (full). (A.5)

**Single-file note**: the brief render and rework loop are implemented inside `plan-story/SKILL.md` in the same task as the stage-machine skeleton (T7), because all three deeply interleave in one markdown file — splitting them across parallel agents that can't see each other's edits to the same file is the real risk.

Provenance: the orchestrator records **one §c activity per stage transition** (origin `plan`, entry_type `execution`), in addition to each dispatched block's own §c. The §l.1 skip-override audit is retired with the per-block gate; rework is audited instead (A.5).

### A.2 Gating-tier triage (the orchestrator decides)

A server-side `compute_gating_tier` (single source of truth, mirroring §k.0 `compute_tier`) keyed on:
- **origin / source**: a finding-spawned task (`work_items.spawned_from_finding_id` set, migration 0011) ⇒ bias `autonomous`; a fresh human feature story ⇒ bias `full`.
- **ambiguity**: `unresolved_questions > 0`, `complexity == high`, or `scope_files > 6` ⇒ raise the tier toward `full`.
- **mode** (§d corroborated, read via `get_execution_mode`): `autonomous` execution mode never *lowers* required human gating — it *degrades* full-tier grills to durable open-questions; `interactive` mode honours the computed tier live.

```text
compute_gating_tier(spawned_from_finding, complexity, unresolved_questions, scope_files):
    if spawned_from_finding AND complexity != "high" AND unresolved_questions == 0:  autonomous
    if complexity == "high" OR unresolved_questions > 0 OR scope_files > 6:           full
    else:                                                                             light
```

(Per User Decision 2, `scope_files` is intentionally **not** a guard on the `autonomous` branch.) Exposed via `get_gating_tier(story_id)` AND folded into `get_story_readiness`. The orchestrator surfaces the tier + rationale and may be overridden by the user ("grill me anyway" / "just run it"). Pure fn, retunes in one place — exactly the §k.0 discipline, with per-branch unit tests mirroring `compute_tier`'s.

### A.3 The decision brief (gate 2 presentation)

Distinct from the raw story dump. Rendered by `get_story_dossier(story_id)`, then composed into a brief with five sections:
1. **Problem** (problem_statement) and **what we're NOT doing** (not_doing).
2. **Chosen approach** (execution_strategy) — and **the competition**: the `rejected_alternatives` the tournament produced, each with its score/rationale, so the user sees the options and *why* this one won.
3. **Impact**: `get_story_files_footprint` (migration 0023) + `get_task_dispatch_plan` parallelism shape ("3 batches; max 4 parallel; 2 deep / 6 lite") + open risks (severity-sorted).
4. **Grounding**: each task with its `task_research_links` notes ("T4 implements R-note 'pinia-ssr-hydration'") — the persisted answer to R52.
5. **Alignment questions** — finding-grounded questions the orchestrator wants confirmed before committing.

The brief text + the align outcome are recorded per epoch (attributes/activity) for audit and resume.

### A.4 Re-fuse decomposition (R52 + R53)

- `decompose-tasks` becomes where **sizing and parallelism are decided together**: it sets `effort`/`complexity` inline (not deferred), targets ≤~3 files / one-agent-session per task (so tier stays Lite unless genuinely deep), forbids file overlap between would-be-parallel tasks, and **writes `task_research_links` on create** so grounding persists. The vertical-slice heuristic is demoted from "one big task" to "a grouping label over several small parallel tasks". `set-task-spec` keeps tier derivation and treats inbound effort/complexity as idempotent (no double-prompt).
- `wire-task-deps` flips from **build-up to prune-down**: it *proposes* a maximally-parallel graph by deriving candidate edges from **file-overlap (`get_story_files_footprint`) and foundation-consumption** (two tasks touching the same file ⇒ candidate serialisation), surfaces the Kahn batches, and asks the user to *prune or confirm* — not to add edges from zero. The R27 complexity-high gate stays but now defaults toward *split*.

### A.5 Rework: supersede + relevance + plan epoch (corrected liveness model)

`work_items.plan_epoch` (story-scoped, `NOT NULL DEFAULT 0`) is the rework generation. Planning child rows (`research_notes`, `risks`, `rejected_alternatives`, `open_questions`, `acceptance_criteria`, child `tasks`) carry a **nullable `plan_epoch`** stamp at creation.

**Liveness vs epoch — the corrected model** (resolves an inconsistency in the original draft):
- **Liveness** is the *only* dossier filter: a row is live ⟺ **not superseded / not rejected / not cancelled / not retired**.
- **Epoch** is **provenance + rework-scoping ONLY — never a dossier filter.** A row that survives a rework keeps its original (older) epoch and stays live (no forced re-stamp). The dossier returns each row's `plan_epoch` as *metadata* so the brief can annotate "this generation", but it does not exclude live rows by epoch.
- This is what makes "preserve work without stale noise" sound: surviving live rows (any epoch) render; invalidated rows are excluded *because they were marked superseded/retired/cancelled*, not because of their epoch.

**A rework** (from the align grill):
1. `bump_plan_epoch(story_id)` → `plan_epoch += 1`.
2. For each block the directive invalidates: supersede the stale rows (`supersede_research_note` / `supersede_risk` / `supersede_rejected_alternative` / `supersede_finding`), flip stale not-started tasks to `cancelled` (R28 path), and `unlink_task_research` the grounding edges of any superseded note / cancelled task (the dossier also join-filters these, but unlinking keeps the brief's Grounding from ever citing a dead note — else R52 returns).
3. **Retire stale open-questions / ACs** (these two tables lack a `superseded_by`): `retire_open_question(id)` sets the new nullable `open_questions.retired_at` — and the dossier filters open_questions on `retired_at IS NULL` **AND** the pre-existing `status != 'cancelled'` (a `resolve_open_question`-cancelled question keeps `retired_at` NULL, so the column alone is insufficient); ACs are **hard-DELETEd** via `remove_acceptance_criterion` under a confirm — `acceptance_criteria` has no liveness column, so ACs are the one hard-delete exception (no supersede provenance; §o documents this and the AC `plan_epoch` stamp annotates live rows only). Both stamped into the rework audit.
4. One rework audit activity records `{from_epoch, to_epoch, reset_kind, affected_phases, superseded_ids, retired_ids}`.

**Partial vs full**: the orchestrator diffs which phases the directive touches — a scope/problem change is a full reset (re-enter at `frame`); an approach disagreement re-enters at Decide; a decomposition complaint re-enters at Decompose.

### A.6 Devil's-advocate prose (R51)

- **`approach` → tournament**: draft ≥2 distinct approaches, score each on consistency / complexity-risk / parallelism / reversibility, present the competition, write the winner as `execution_strategy` AND auto-populate `rejected_alternatives` with the losers + rationale (feeding the brief directly). **Relax the zero-accepted-notes hard-fail** (`approach/SKILL.md:35-38`) to a warning, so the tournament's divergent thinking can run from the dossier even when the vetted-research funnel is sparse (still grounded, just not blocked).
- **`research-explore` → 6th `contrarian` lens** (User Decision 1): an always-dispatch lens that actively seeks evidence the chosen direction is wrong and surfaces competing patterns. §k.1 amended to six lenses.
- **`story-review`**: add a rubric category that *argues against* the plan (steelman the rejected alternatives) and one that flags **scope conservatism** as a finding.
- **`user-interrogation` (framing)**: add the scope-challenge axis; make the already-covered heuristic stricter (don't suppress an axis on a single keyword hit — require fuller coverage before skipping).

### A.7 Full-story portrayal (dossier-first reads)

§o mandates that the reshaped orchestrator-driven blocks **read `get_story_dossier` first** for context (full-story incl. persisted task↔research links), so a block re-run mid-flow or weeks later has the whole picture rather than a bag of fields. The dossier is **derived** (no new table); it composes existing `WorkItemDetail` + `task_research_links` + the footprint/dispatch reads, filtered by liveness (A.5).

### Reuse map
- §k.0 `compute_tier` (`task_graph.rs:93-112`) → `compute_gating_tier` (pattern + tests).
- R28 task-supersession-via-cancelled → rework task path.
- migration 0011 `spawned_from_finding_id` + `get_execution_mode` → autonomous-tier origin + mode signals.
- migration 0023 `get_story_files_footprint` / `get_task_dispatch_plan` → brief impact + file-overlap edge derivation.
- existing `supersede_*` tools → rework block supersession.
- `record_inert_event` (non-`work_item` aggregate) → epoch/link/retire events (export-inert, like `task_files`).

### Deliberate trade-off (export-inert epoch)
`bump_plan_epoch` mutates a `work_items` column but records an **export-inert** event (non-`work_item` aggregate), so the git-export snapshot's `plan_epoch` may lag. This is intentional and mirrors the `task_files`/`worktree` inert-event precedent — `plan_epoch` is internal planning metadata, not part of the exported audit semantics. Documented in §o.

## Verification Commands
```
build:  $env:CARGO_INCREMENTAL=0; cargo build --workspace --manifest-path lumina/Cargo.toml
test:   cargo nextest run --workspace --manifest-path lumina/Cargo.toml --profile ci
lint:   cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets
audit:  cargo audit --file lumina/Cargo.lock
gates:  rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src   # must be 0
        cargo tree --manifest-path lumina/Cargo.toml -p lumina-server -e normal | rg -i '\b(git2|gix)'   # must be 0
        bash scripts/verify-plan-story-blocks.sh
spa:    cd lumina/web; bun run type-check; bun test
```

## Tasks

### Phase 1: Backend foundation

#### T1: Add migration 0026 (plan epoch + retire + task↔research links)
- **Files**: `lumina/core/migrations/0026_plan_epoch_and_links.sql` (new)
- **Depends on**: none
- **Action**: Add `work_items.plan_epoch INTEGER NOT NULL DEFAULT 0`. Add nullable `plan_epoch INTEGER` to `research_notes`, `risks`, `rejected_alternatives`, `open_questions`, `acceptance_criteria`. Add nullable `open_questions.retired_at TEXT` (the rework liveness signal). Create `task_research_links(task_id TEXT NOT NULL REFERENCES work_items(id) ON DELETE CASCADE, research_note_id TEXT NOT NULL REFERENCES research_notes(id) ON DELETE CASCADE, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP, PRIMARY KEY(task_id, research_note_id))`.
- **Detail**: Forward-only nullable `ADD COLUMN` (no `DEFAULT` on the nullable adds); `NOT NULL` adds carry a `DEFAULT` (`plan_epoch` on `work_items`). FK clauses are only legal on the `CREATE TABLE` (mirror `0023_task_files.sql`). Do **NOT** edit any applied migration (memory: never edit an applied migration — breaks the sqlx checksum).
- **Acceptance**: `cargo build -p lumina-core` clean; fresh `db::init` applies 0026; `sqlx::migrate!` embedded-checksum set stable.
- **Effort**: S

#### T2: Domain types — gating tier, epoch, dossier, readiness/detail folds
- **Files**: `lumina/core/src/domain/` (`enums.rs`, `work_items.rs`, `planning.rs`; `findings.rs` only if the dossier folds findings — the enumerated edits below do not touch it, so drop it otherwise)
- **Depends on**: T1
- **Action**: Add `GatingTier::{Full, Light, Autonomous}` with `#[serde(rename_all="snake_case")]` + `JsonSchema` (wire `full`/`light`/`autonomous`), mirroring `Tier` at `enums.rs:292`. Add `plan_epoch: i64` to `WorkItem` (so `WorkItemDetail.item` carries it). Fold `task_research_links` (note ids/summaries) into the task's detail. Add a new `StoryDossier` struct (story `WorkItemDetail` + per-task research links + `story_files_footprint` + dispatch-plan shape + readiness). Extend `StoryReadiness` (`planning.rs:47-56`) with `plan_epoch: i64`, `gating_tier: GatingTier`, and the long-missing `verification_commands_set: bool`.
- **Detail**: Keep `GatingTier` strictly distinct from `Tier` (§k dispatch tier) — different concern, per the `Severity`/`RiskSeverity` non-unification precedent. `StoryReadiness` is `Serialize`-only (no `JsonSchema`), so make `gating_tier` non-`Option` (every call site populates it) and decide whether `get_gating_tier` returns via `Content::json` (no schema) or `Json<T>` (needs `JsonSchema`) — that decides whether `GatingTier`'s `JsonSchema` derive is load-bearing rather than cargo-culted.
- **Acceptance**: types compile; serde round-trip unit test asserts `GatingTier` wire = `full`/`light`/`autonomous` and the three new readiness fields serialize.
- **Effort**: M

#### T3: Repo layer — epoch, links, gating, dossier, retire, readiness
- **Files**: `lumina/core/src/repo/` — `compute_gating_tier` + `bump_plan_epoch` + `link_task_research`/`unlink_task_research` + `get_story_dossier` in `task_graph.rs` (beside `compute_tier`) or a new `repo/dossier.rs`; `retire_open_question` in `repo/open_questions.rs`; the readiness-query extension in `repo/readiness.rs`. (There is NO `repo/planning.rs` — do not look for one.)
- **Depends on**: T2
- **Action**: `bump_plan_epoch(story_id)`; `link_task_research`/`unlink_task_research` (`unlink_*` repo-internal, used by rework/cancel — NOT MCP-surfaced); `compute_gating_tier(spawned_from_finding, complexity, unresolved_questions, scope_files) -> GatingTier` (the A.2 rule, single source); `get_story_dossier(story_id) -> StoryDossier` (liveness-filtered composition; epoch returned as metadata, never a filter); `retire_open_question(id)` (sets `retired_at`); extend the readiness query with `verification_commands_set` + `plan_epoch` + `gating_tier`. Each mutation = one txn + one `events` row (preserve the +1 invariant); epoch/link/retire events are **export-inert** (non-`work_item` aggregate via `record_inert_event`) — this REQUIRES registering a NEW inert aggregate-type literal (e.g. `plan_epoch`) in the `record_inert_event` guard list (`events.rs`) + its doc comment + the inert-vocab enumerations in `lumina/CLAUDE.md`, since the guard rejects `work_item` and currently allows only run/sprint/finding/batch/session/worktree/task_files (mirror how migration 0023 added `task_files`).
- **Detail**: Dossier liveness filter, applied PER child-table by its OWN signal (no single uniform predicate): `research_notes`/`risks`/`rejected_alternatives` ⇒ `superseded_by IS NULL`; child tasks ⇒ `status != 'cancelled'`; `open_questions` ⇒ `retired_at IS NULL` AND `status != 'cancelled'` (the pre-existing 0003 lifecycle — a `resolve_open_question`-cancelled question keeps `retired_at` NULL); `task_research_links` ⇒ folded ONLY for links whose note is live (`superseded_by IS NULL`) AND whose task is not `cancelled` (else the brief's Grounding re-introduces R52, so rework must also `unlink_task_research` or the fold must join-filter); `acceptance_criteria` ⇒ NO liveness column (hard-DELETE via `remove_acceptance_criterion`), so a removed AC is simply absent — §o must declare ACs the one hard-delete exception and note the AC `plan_epoch` stamp annotates live rows only (or add `acceptance_criteria.retired_at` for symmetry). `compute_gating_tier` unit tests pin each branch, mirroring `compute_tier`'s tests at `task_graph.rs:649-703`. **Sizing**: this is a large task (6–7 mutators/reads + ~10 branch tests + the dossier composition + a rework-sim test) — implement as two sequential passes if needed (T3a mutators+gating+tests → T3b dossier+readiness+rework test; same dir, distinct functions).
- **Acceptance**: `cargo test -p lumina-core` — per-branch `compute_gating_tier` tests pass; a unit test simulates a rework (supersede a note + retire a question) and asserts `get_story_dossier` excludes both while keeping surviving older-epoch rows.
- **Effort**: L

#### T4: MCP tools + count-invariant (94 → 99)
- **Files**: `lumina/server/src/mcp/mod.rs`, `lumina/server/src/mcp/planning.rs`, `lumina/server/src/mcp/reads.rs`
- **Depends on**: T3
- **Action**: Surface **5** new tools — writes in `planning.rs` (`bump_plan_epoch`, `link_task_research`, `retire_open_question`), reads in `reads.rs` (`get_story_dossier`, `get_gating_tier`). Fold `task_research_links` into `get_work_item`'s task detail (no new tool). Update BOTH count-invariant assertions — `names.len()` (`mod.rs:~593`) AND the `unique.len()` name-uniqueness guard (`mod.rs:~608`) — from **94 to 99**, append the five new tools to the enumerating comment block (`mod.rs:~578-589`), and state the `+5` delta in the test comment.
- **Detail**: `unlink_task_research` is **not** surfaced (repo-internal). Map `AppError::Validation → invalid_params` for misuse: `link_task_research` on a non-task or note from another story; `retire_open_question` on a non-question; `get_story_dossier`/`get_gating_tier` on a non-story. The same-story (and live-note) validation for `link_task_research` MUST live in `repo::link_task_research`, not the MCP param layer, so the T5 HTTP mirror inherits it (single-mutation-path); add a T3 unit test for the cross-story rejection.
- **Acceptance**: `cargo test -p lumina-server` mcp tests green incl. the `== 99` count-invariant; `invalid_params` returned on each misuse path.
- **Effort**: M

#### T5: HTTP mirrors
- **Files**: `lumina/server/src/http/` — add `http/dossier.rs` (the `GET` dossier + `GET` gating-tier + `POST` bump-plan-epoch + `POST` link-task-research routes) with `pub mod dossier;` + `.merge(dossier::router())` registered in `mod.rs:49-80`; extend `open_questions.rs` for `POST` retire-open-question. (Pick the new-module layout — do not leave "extend readiness.rs OR new dossier.rs" as an executor choice.)
- **Depends on**: T4
- **Action**: Mirror the new tools under `/api` delegating to the same `repo::*` mutations: `GET` dossier + `GET` gating-tier (extend `readiness.rs` or new `dossier.rs`); `POST` retire-open-question (extend `open_questions.rs`); `POST` bump-plan-epoch + `POST` link-task-research (new planning/epoch route). Each handler returns `Json<T>` / `Result<_, AppError>` per the round-4 mirror contract.
- **Acceptance**: route tests; an e2e HTTP read of a dossier matches the MCP read byte-for-byte on the shared fields.
- **Effort**: M

#### T6: SPA wire-mirrors (lockstep — see memory: SPA wire-mirror coupling)
- **Files**: `lumina/web/src/api/wire-enums.ts`, `lumina/web/src/api/readiness.ts` (the `StoryReadinessSchema` `z.object` lives HERE, not `execution.ts`), `lumina/web/src/api/work-items.ts` (the `WorkItem` interface + `WorkItemSchema` — `plan_epoch` is a required `i64`, so it MUST be mirrored or `bun run type-check` breaks; plus the `WorkItemDetail` task-detail mirror if `task_research_links` folds as a first-class field rather than into opaque `attributes`), `lumina/web/src/api/execution.ts` (the new `StoryDossier` interface + `z.object`), and the full-literal `WorkItem`/readiness snapshot helpers + fixtures under `lumina/web/src/__tests__/` (e.g. the `makeWorkItem` factory in `floating-chat.test.ts`)
- **Depends on**: T2 (runs parallel to T3; before final SPA verification)
- **Action**: Add `const GATING_TIER_VALUES = ['full','light','autonomous'] as const` + `GatingTierSchema = z.enum(...)` + `type GatingTier` to `wire-enums.ts`. Add `plan_epoch` to `WorkItem`/`WorkItemSchema` in `work-items.ts`; add `plan_epoch`/`gating_tier`/`verification_commands_set` to the readiness `z.object` **in `readiness.ts`** (not `execution.ts`); add the `StoryDossier` interface + `z.object({...}) satisfies z.ZodType<StoryDossier>` in `execution.ts`. Update every full-literal `WorkItem`/readiness fixture + snapshot helper (e.g. `makeWorkItem` in `floating-chat.test.ts`).
- **Detail**: No lens enum exists in the SPA (lens is free-text) — the contrarian lens needs **no** SPA change.
- **Acceptance**: `cd lumina/web && bun run type-check && bun test` green.
- **Effort**: M

### Phase 2: Orchestrator core (after Phase 1)

#### T7: Rewrite `plan-story` as the stage-machine orchestrator (stage machine + decision brief + rework)
- **Files**: `claude/plugins/lumina-story-blocks/skills/plan-story/SKILL.md` (single file)
- **Depends on**: T4
- **Action**: Replace the per-block `Run/Skip/Inspect/Abort` walk with the A.1 stage machine (triage→frame→plan→brief→align→rework), implemented in one pass:
  - **triage**: `get_gating_tier`; surface `gating: <tier> — <rationale>`; honour user override; autonomous degrades grills to durable open-questions (read `get_execution_mode`).
  - **frame** (gate 1): reshaped `problem-statement` + `user-interrogation` framing grill + scope-challenge.
  - **plan**: auto-run Phases 2–5; thread `task_research_links` grounding into decompose; single mid-flow interrogation only on serious ambiguity.
  - **brief**: render the A.3 five-section decision brief from `get_story_dossier`; record brief text + align outcome per epoch.
  - **align** (gate 2): grill on the brief (competing options + impact + grounding shown).
  - **rework**: A.5 — `bump_plan_epoch`, affected-phase diff (scope/problem→frame, approach→Decide, decompose→Decompose), supersede affected blocks + `cancelled` stale tasks + `retire_open_question`/`remove_acceptance_criterion` under confirm, one rework audit activity, re-entry routing.
  Keep `Skill()`-dispatch of blocks (§l.4) and the §l.0 six phases as the planning core. Add per-stage §c provenance (one activity per transition); remove the §l.1 skip-override path; cite new §o.
- **Detail**: Brief + rework live here (not split tasks) to avoid parallel edits to this one file. This is judgement-heavy prose → `implement-deep`.
- **Acceptance**: skill body contains the six stage names (`triage`/`frame`/`plan`/`brief`/`align`/`rework`), the three gating tiers (`full`/`light`/`autonomous`), the brief's five section headings, and the rework contract (all grep-checkable); `bash scripts/verify-plan-story-blocks.sh` passes after T11 — NOTE the script gates only §l.0 coverage / §l.4 citation / a `Skill(` call / no `disable-model-invocation:`, so it does NOT verify the stage machine; this task ALSO requires explicit human review of the rewritten prose in the verification pass.
- **Effort**: L

### Phase 3: Block prose — re-fusion + devil's advocate (after Phase 1; distinct files)

#### T8: Re-fuse decomposition (sizing + parallelism + persisted grounding)
- **Files**: `claude/plugins/lumina-story-blocks/skills/decompose-tasks/SKILL.md`, `skills/set-task-spec/SKILL.md`
- **Depends on**: T4
- **Action**: Per A.4 — `decompose-tasks` sets effort/complexity inline; ≤~3-file / one-session sizing + no-file-overlap rule between would-be-parallel tasks; **writes `task_research_links` on create** (persist grounding); demote vertical-slice to a small-parallel-task grouping label. `set-task-spec` treats inbound effort/complexity as idempotent (no double-prompt) and keeps §k tier derivation.
- **Acceptance**: the `decompose-tasks` SKILL.md includes a worked example that SHOWS ≥3 file-disjoint Lite tasks, each with a `link_task_research` call grounding it to ≥1 note; both edited bodies contain the strings `R52` and `R53` (doc-content assertion — a SKILL.md cannot be "run").
- **Effort**: M

#### T9: Flip `wire-task-deps` to prune-down
- **Files**: `claude/plugins/lumina-story-blocks/skills/wire-task-deps/SKILL.md`
- **Depends on**: T4
- **Action**: Per A.4 — propose a maximally-parallel graph from file-overlap (`get_story_files_footprint`) + foundation-consumption, surface Kahn batches (`compute_task_batches`), and ask to **prune/confirm** (not add from zero); the R27 complexity-high gate defaults toward split.
- **Acceptance**: skill derives candidate edges from file overlap rather than from zero; cites R53.
- **Effort**: M

#### T10: Devil's-advocate prose (tournament / contrarian lens / sharpened review / framing scope-challenge)
- **Files**: `skills/approach/SKILL.md`, `skills/research-explore/SKILL.md`, `skills/story-review/SKILL.md`, `skills/user-interrogation/SKILL.md`, `skills/problem-statement/SKILL.md`
- **Depends on**: T4
- **Action**: Per A.6 — `approach` tournament (≥2 scored approaches → winner as `execution_strategy` + auto-populated `rejected_alternatives`; **relax the `:35-38` zero-accepted-notes hard-fail to a warning**); add the 6th **`contrarian`** lens (always-dispatch) to `research-explore`; `story-review` gains argue-against + scope-conservatism rubric categories; `user-interrogation` gains the scope-challenge axis + a stricter already-covered heuristic; `problem-statement` supports the framing scope-challenge.
- **Detail**: 5 files, each a small localized edit (under the 6-file batch cap). The §k.1 six-lens vocabulary itself is amended in CONVENTIONS (T11); `research-explore` prose must match that vocabulary — keep the six-lens wording byte-consistent between `research-explore/SKILL.md` and §k.1, because the drift gate does NOT check lens-name consistency (a mismatch passes CI silently; verify by hand in T11). The "exactly five values" statement is at `research-explore/SKILL.md:50` and the always-dispatch logic at `:52-54` — amend both.
- **Acceptance**: `approach` writes ≥1 `rejected_alternative` from the tournament and no longer aborts on zero accepted notes; `research-explore` dispatches the contrarian lens; `story-review` rubric includes the two new categories; `user-interrogation` adds the scope-challenge axis.
- **Effort**: M

### Phase 4: Integration, gates, docs (after Phases 2–3)

#### T11: CONVENTIONS §o + §k.1 amendment + drift gate + create-project + next-block
- **Files**: `claude/plugins/lumina-story-blocks/CONVENTIONS.md` (new §o + §k.1 amendment), `scripts/verify-plan-story-blocks.sh`, `skills/create-project/SKILL.md`, `skills/next-block/SKILL.md`
- **Depends on**: T7, T8, T9, T10
- **Action**: Author **§o** (orchestrator stages; gating tiers + `compute_gating_tier` rule; plan epoch + the corrected liveness model — *liveness filters the dossier, epoch is provenance/scoping only*; dossier-first reads; rework contract; devil's-advocate mandate; the export-inert epoch trade-off). Amend **§k.1** from five to **six** lenses (add `contrarian`). Update `verify-plan-story-blocks.sh` for the new orchestrator shape — it must still assert §l.4 citation + a `Skill(` dispatch in `plan-story`/`create-project` + no `disable-model-invocation:`, and §l.0 coverage must still pass (no new phase skill is added since the brief is inline; `research-explore` stays a phase skill). Update `create-project`'s plan-story dispatch note (orchestrator shape; §l.4 + depth-1→2 preserved) and `next-block` to surface the gating tier (`readiness.gating_tier` / `get_gating_tier`). Also amend the §l.0 note that says `verification_commands_set` is unexposed (T2/T3 now expose it — `plan-story` reads the readiness field, not `detail.attributes.verification_commands`), and tombstone/retire the §l.1 skip-override contract in CONVENTIONS (T7 drops that path).
- **Acceptance**: `bash scripts/verify-plan-story-blocks.sh` passes (all four invariants).
- **Effort**: M

#### T12: e2e round-5 thread + project/lumina/plugin docs
- **Files**: `lumina/server/tests/e2e.rs` (new thread), `CLAUDE.md`, `lumina/CLAUDE.md`, `claude/plugins/lumina-story-blocks/README.md`, `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` (the authoritative tool catalogue — add the 5 new tools)
- **Depends on**: T5, T11
- **Action**: Add an in-process thread (4-leg pattern per `e2e.rs:171`): create story → triage tier (`compute_gating_tier` inputs) → frame → plan (decompose writing `task_research_links`) → brief read (`get_story_dossier`) → rework (`bump_plan_epoch` + supersede a research note + `retire_open_question`) → assert the live dossier excludes the superseded/retired rows while keeping surviving older-epoch rows. Update the MCP-surface count (**94 → 99**) across ALL occurrences in `CLAUDE.md` + `lumina/CLAUDE.md` (grep for `94` — it appears many times, not one paragraph) and reword the in-context skill-prose refs (`run-sprint/SKILL.md:97,281` "stays 94" → "adds no tool"); document the 5 new tools in `skills/mcp/SKILL.md`; add a §o pointer in the plugin README. (Re-sync the plugin cache after merge per the plugin-cache-sync note.)
- **Acceptance**: `cargo nextest … --profile ci` green; doc MCP count reads 99 and matches T4's test.
- **Effort**: M

## Dependency Graph
```
T1 ─► T2 ─┬─► T3 ─► T4 ─┬─► T5 ─────────────────────────┐
          │             ├─► T7 ──────────────┐          │
          │             ├─► T8 ──────────────┤          │
          │             ├─► T9 ──────────────┤          │
          │             └─► T10 ─────────────┤          │
          └─► T6                             │          │
                          T7,T8,T9,T10 ─► T11 ───────────┤
                                    T5,T11 ─────────────► T12
```
Parallel batches (no batch chunk > 4 agents; no two parallel tasks share a file):
- **B1**: T1 → T2 (sequential)
- **B2**: {T3, T6} (repo.rs ∥ lumina/web)
- **B3**: {T4}
- **B4**: {T5, T7, T8} then {T9, T10} (split to respect the 4-agent cap; all files distinct)
- **B5**: {T11}
- **B6**: {T12}

## Verification
- [ ] `build` / `test` / `lint` / `audit` (Verification Commands block) all green
- [ ] macro-eradication gate (`rg sqlx::query…!` = 0) + control-plane purity gate (`git2|gix` = 0) report 0
- [ ] `bash scripts/verify-plan-story-blocks.sh` passes (§l.0 coverage, §l.4 citation, `Skill(` dispatch, no `disable-model-invocation:`)
- [ ] `cd lumina/web && bun run type-check && bun test` green
- [ ] MCP count-invariant test asserts `== 99` and is green; `CLAUDE.md` + `lumina/CLAUDE.md` MCP-surface count reads 99
- [ ] e2e round-5 thread asserts the live dossier excludes superseded/retired rows after a rework

## Risks
- **R-risk-1 (high)** — The `plan-story` rewrite (T7) is the load-bearing change and the `verify-plan-story-blocks.sh` gate asserts the §l.0/§l.4 shape. *Mitigation*: keep the six §l.0 phases as the planning core (stages wrap them); brief + rework fold into the same single-file task (no parallel same-file edits); update the gate in the same phase (T11); §l.4 `Skill()`-dispatch preserved.
- **R-risk-2 (medium)** — Plan-epoch stamping on five child tables risks "stale row leaks into the live view". *Mitigation*: the corrected model makes **liveness** the sole dossier filter (`not superseded/rejected/cancelled/retired`); epoch is provenance/scoping only and never filters. T3 unit test + T12 e2e both assert the live dossier excludes superseded/retired rows while keeping surviving older-epoch rows.
- **R-risk-3 (medium)** — `open_questions`/`acceptance_criteria` have no supersede tool — the rework retire path is bespoke. *Mitigation*: add the minimal `open_questions.retired_at` column + `retire_open_question` tool; reuse `remove_acceptance_criterion` (under confirm) for ACs; don't build a general supersede this round; audit every retire in the rework activity.
- **R-risk-4 (medium)** — Gating-tier autonomy could let an under-specified item run zero-gate. *Mitigation*: the A.2 rule grants `autonomous` only when `spawned_from_finding AND complexity!=high AND unresolved_questions==0`; the user can always override to `full`; autonomous *mode* degrades grills to durable open-questions (never silent). Per User Decision 2, a large file footprint relies on the override rather than an automatic ceiling — an accepted trade-off.
- **R-risk-5 (low)** — SPA wire-mirror drift (per memory). *Mitigation*: T6 in lockstep + `bun run type-check`/`bun test` in final verification; the contrarian lens needs no SPA change (lens is free-text).
- **R-risk-6 (low)** — Export-inert `plan_epoch`: a bump records a non-`work_item` event so it does NOT itself trigger a re-export — but the column rides the `work_item` snapshot and SELF-HEALS on the next `work_item`-aggregate event (a rework emits several). So the snapshot reflects the epoch as of the last `work_item` render, not a standing divergence. *Mitigation*: deliberate, documented in §o; mirrors the `task_files`/`worktree` inert-event precedent; epoch is internal planning metadata. **T12's e2e must NOT assert the exported snapshot's `plan_epoch` immediately after `bump_plan_epoch` without an intervening `work_item` event + drain.**

## Future-round notes
- Wire the orchestrator into the focus-1C tokio scheduler/queue (1C.3) so backlog stubs are auto-activated and planned; durable-comms transport (1C.1) carries the autonomous-tier open-questions.
- Promote the decision brief to a first-class `plan_briefs` table if a UI consumer materialises.
- Add a `task_groups` table (§j.1) when a real grouping consumer (e.g. `/lumina:run-batch`) lands.
- Reconcile `plan_epoch` with `0021_resume_epoch` if run-resume and plan-rework generations ever need to join.
- Surface `unlink_task_research` as an MCP tool (would take the count to 100) if a UI or skill needs to detach grounding edges outside the rework path.
