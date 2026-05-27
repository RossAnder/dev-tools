# Plan: lumina story-planning round-4 — HTTP API + frontend wire surface
**Plan path**: docs/plans/lumina-story-planning-round-4.md
**Created**: 2026-05-27
**Status**: draft

## Context

Rounds 1–3 of the lumina story-planning workflow landed the full backend domain — migrations 0001–0007, ~16 typed enums (Kind/Status/Relevance/Effort/Complexity/Origin/ClosureGate/Severity/RiskSeverity/Tier/Phase/ResearchState/QuestionStatus/ActivityType/Disposition/Confidence/TaskKind), ~18 read structs (WorkItem, WorkItemDetail, AcceptanceCriterion, ResearchNote, OpenQuestion, QuestionOption, Risk, RejectedAlternative, TaskDependency, RepoLink, ContextBlock, Finding, WorkItemActivity, BatchEntry, StoryReadiness, …), ~57 `repo::*` mutation/read functions, and ~55 MCP `#[tool]` methods spanning every planning operation. The git-export drain and SQLite-canonical write path are all proven end-to-end by `lumina/tests/e2e.rs`.

The HTTP API (`lumina/src/http.rs`) and the frontend wire layer (`lumina/web/src/api.ts`) lag the MCP surface by a wide margin:

- **HTTP**: 8 routes covering only `GET /health`, work-item CRUD, and repo-links. Every planning write (acceptance criteria, research notes, open questions, risks, rejected alternatives, task dependencies, scalars, story plan, task spec, task kind, tier, findings beyond what's already implicit, activity log, context blocks) is MCP-only. No HTTP equivalent of `get_story_readiness` or `get_task_dispatch_plan` exists. The detail-fold read (`GET /work-items/{id}`) already surfaces every aggregate (risks, rejected_alternatives, task_dependencies are folded into `WorkItemDetail`), so reads are mostly complete — the gap is overwhelmingly writes.
- **Frontend wire** (`lumina/web/src/api.ts`): `WorkItemSchema` is missing `task_kind` and `tier`; `WorkItemDetailWireSchema` is missing `risks`, `rejected_alternatives`, `task_dependencies`; wire enums missing `TaskKind`, `Tier`, `RiskSeverity` (deliberately distinct from `Severity`); no wire schemas exist for `Risk`, `RejectedAlternative`, `TaskDependency`, `BatchEntry`, `StoryReadiness`, `NextAction`. Fetch wrappers cover only `fetchTree`/`fetchDetail`/`createWorkItem`/`updateWorkItem`/`updateStatus` + the three repo-link mutations.

Without this round, the web UI cannot consume rounds 1–3 work — it would have to read everything via MCP from inside the browser (not how the SPA is wired) or use untyped JSON. Round-4 closes that gap so UI work can start.

## Scope

**In scope**:
- Expand `lumina/src/http.rs` (refactored to `lumina/src/http/` directory) with one PATCH/POST/DELETE/GET handler per MCP write tool + two new read endpoints (`/readiness`, `/dispatch-plan`).
- Mirror the MCP write surface fully: scalars (relevance/effort/complexity/closure-gate/task-kind/tier), story plan, task spec, acceptance criteria (add/check/uncheck/remove), research notes (add/update/supersede), open questions + branches (add/add-option/block/set-enabling/resolve), risks (add/update/supersede/remove), rejected alternatives (add/update/supersede/remove), task dependencies (block/unblock + list/compute-batches reads), findings (add/update/resolve/supersede), activity log (record), context blocks (create/link).
- Update `WorkItemDetail` HTTP serialization is **already correct** (the Rust struct already folds risks/rejected_alternatives/task_dependencies — no backend read changes needed apart from confirming via test).
- Expand `lumina/web/src/api.ts` (refactored to `lumina/web/src/api/` directory) with wire schemas for every missing type and fetch wrappers for every new endpoint.
- Add per-family composables under `lumina/web/src/composables/` mirroring the existing `useHierarchy` / `useRepoLinks` pattern.
- Per-family HTTP smoke tests in `lumina/tests/e2e.rs`.
- Per-family bun tests in `lumina/web/src/__tests__/`.

**Out of scope**:
- Any UI rendering (Vue components/views consuming the new composables). UI work is the next round.
- New MCP tools or new repo functions. The Rust layer is complete; this round is HTTP + wire only.
- Schema migrations. No SQL changes.
- Authentication, authorisation, rate-limiting. Existing in-process posture (no auth) is preserved.
- Splitting the existing `severity` vocab unification — `Severity::{Critical,Major,Minor,Suggestion}` (findings) and `RiskSeverity::{Low,Medium,High,Critical}` (risks) remain deliberately distinct per `lumina/CLAUDE.md`.

**Affected areas**:
- `lumina/src/` (http module refactor + new per-family route files)
- `lumina/tests/e2e.rs`
- `lumina/web/src/` (api split + new composables + tests)
- `lumina/CLAUDE.md` (route-surface documentation)

**Estimated file count**: ~28 files (1 split of `http.rs` into ~10 module files + ~10 new per-family backend route files + 1 e2e test file + 1 split of `api.ts` into ~10 module files + ~10 new per-family composable files + ~3 new test files + 2 CLAUDE.md updates).

## Research Notes

_No external research required — the work is mechanical mirroring of an already-typed, in-codebase MCP surface against an already-typed, in-codebase HTTP/zod surface. Axum 0.8 and zod 4 are pinned; idioms are established in the existing `http.rs` (Router + State + IntoResponse + tower::oneshot tests) and `api.ts` (zod schema + handle<T> + module-singleton composables). No deprecations or version migrations affect this round._

## User Decisions

| Question | Choice | Prompting finding |
|---|---|---|
| HTTP shape | **Sub-resource under work-items** — `POST /work-items/{id}/acceptance-criteria`, `PATCH /work-items/{id}/relevance`, `GET /work-items/{story_id}/readiness`. Mirrors the existing `/work-items/{project_id}/repo-links` pattern. | Existing http.rs uses sub-resource style for repo-links (`/work-items/{project_id}/repo-links/{id}`). |
| Write surface scope | **Full mirror — every MCP write tool** gets an HTTP endpoint. | MCP exposes ~45 write tools; user wants UI freedom across the whole planning surface, not just human-driven subset. |
| Frontend organisation | **One composable per domain family** — `useAcceptanceCriteria`, `useResearchNotes`, `useRisks`, `useRejectedAlternatives`, `useTaskDependencies`, `useOpenQuestions`, `useReadiness`, `useDispatchPlan`, `useFindings`, `useScalars`, `useStoryPlan`, `useTaskSpec`, `useContextBlocks`, `useActivity`. | Existing `useHierarchy.ts` / `useRepoLinks.ts` split establishes the per-family module-singleton pattern (memory: `feedback_lumina_web_state_management.md`). |
| Test depth | **Per-family smoke test** — one happy-path e2e per family covering POST + PATCH + read; trust repo-layer tests for exhaustive cases. | Repo layer already has comprehensive tests; HTTP layer is a thin wrapper. |

## Approach

### Backend: split + mirror

1. **Refactor** `lumina/src/http.rs` into a directory module `lumina/src/http/` so per-family work can run in parallel without single-file contention. Module layout:
   - `mod.rs` — `pub fn router() -> Router<AppState>`, composes all sub-routers via `.merge(...)`; re-exports `AppState`.
   - `work_items.rs` — existing `list_work_items`/`get_work_item`/`create_work_item`/`update_work_item` handlers + `ListQuery`/`TreeNode`.
   - `repo_links.rs` — existing repo-link handlers + bodies.
   - Per-family files added in subsequent tasks.

2. **Per-family route files** under `lumina/src/http/`. Each file:
   - Defines `pub fn router() -> Router<AppState>` returning its own `Router::new().route(...)` chain.
   - Defines HTTP request body structs (`#[derive(Deserialize)]`) where the MCP `*Params` struct isn't directly reusable; reuses MCP Params where it is.
   - Handlers delegate to `repo::*` (the established single-mutation-path invariant) — no raw SQL.
   - Returns `StatusCode::OK` + `Json<Detail>` for state-changing operations so the frontend's `handle<T>` (which unconditionally `.json()`s) doesn't break; `204 No Content` only where a return body is genuinely meaningless and the FE explicitly handles 204 (e.g. removes).
   - Maps `AppError` to the existing `IntoResponse` envelope — no new error variants.

3. **Per-family smoke tests** in `lumina/tests/e2e.rs`. One in-process thread test per family, driven by `tower::ServiceExt::oneshot` against `build_router(state)` (existing pattern). Each test exercises one mutation + one detail re-read to confirm git-export + DB + HTTP align.

### Frontend: split + mirror

1. **Refactor** `lumina/web/src/api.ts` into `lumina/web/src/api/` so per-family schemas + wrappers parallelise. Module layout:
   - `index.ts` — re-exports for backcompat (existing `import { fetchTree } from '@/api'` continues to work).
   - `http.ts` — shared `handle<T>` helper, base URL.
   - `wire-enums.ts` — every wire-enum schema (Kind/Status/Relevance/Effort/Complexity/Origin/ClosureGate/Severity/Disposition/ActivityType/ResearchState/QuestionStatus/Confidence + **new**: TaskKind, Tier, RiskSeverity).
   - `work-items.ts` — `WorkItemSchema` (**now including `task_kind` and `tier`**), `WorkItemNodeSchema`, `WorkItemDetailWireSchema` (**now including `risks`, `rejected_alternatives`, `task_dependencies`**), `fetchTree`/`fetchDetail`/`createWorkItem`/`updateWorkItem`/`updateStatus`.
   - `repo-links.ts` — `RepoLinkSchema` + existing wrappers.
   - Per-family files added in subsequent tasks.

2. **Per-family wire modules** under `lumina/web/src/api/`. Each file:
   - Defines its zod schemas for inputs (request bodies) and outputs (returned detail rows).
   - Defines fetch wrappers; uses `handle<T>` from `./http.ts`.
   - Exports `Result<T, E>` types matching the existing composable contract.

3. **Per-family composables** under `lumina/web/src/composables/`. Each composable follows the established module-singleton pattern: module-scope reactive refs (`ref()`/`computed()`), an init/bind function, mutation functions returning `Result<T, E>`, and `__setApiForTests` / `__resetForTests` seams.

4. **Per-family bun tests** under `lumina/web/src/__tests__/`. Mirror existing `showcase.test.ts` patterns: schema-fixture validation, fetch wrapper happy + error path, composable mutation flow with mocked api.

## Verification Commands

```text
build: cargo build --manifest-path lumina/Cargo.toml
test: cargo nextest run --manifest-path lumina/Cargo.toml && cd lumina/web && bun test
lint: cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings
sqlx: cd lumina && cargo sqlx prepare --check
smoke: manual — see Verification section step 8 (start lumina, POST a risk, GET the detail)
```

## Tasks

### Phase 1: Backend scaffolding (sequential)

#### T1: Split `lumina/src/http.rs` into `lumina/src/http/` module directory + pre-declare router merges + add DELETE work-item
- **Files**: `lumina/src/http.rs` (delete), `lumina/src/http/mod.rs` (new), `lumina/src/http/work_items.rs` (new), `lumina/src/http/repo_links.rs` (new), `lumina/src/main.rs` (only if it imports http internals — usually no change).
- **Action**:
  1. Move every existing handler, request body struct, and helper from `http.rs` into a per-family file under `http/`.
  2. **Pre-declare every Phase-2 family in `mod.rs` up front.** Declare empty modules `pub mod structured_patches; pub mod acceptance_criteria; pub mod research_notes; pub mod risks; pub mod rejected_alternatives; pub mod task_dependencies; pub mod open_questions; pub mod findings; pub mod activity; pub mod context_blocks; pub mod readiness;` and write a stub `pub fn router() -> Router<AppState> { Router::new() }` in each empty file. Compose `pub fn router() -> Router<AppState>` in `mod.rs` via `.merge(work_items::router()).merge(repo_links::router()).merge(structured_patches::router())…` for ALL future families. Each Phase-2 task then only fills in its owned file body — `mod.rs` is touched ONCE here and NOT in subsequent tasks.
  3. **Add `DELETE /work-items/{id}` handler in `work_items.rs`** delegating to `repo::delete_work_item` (soft-delete; `repo.rs:1912`). Returns 204 No Content; 404 on unknown id. Closes the "Full mirror" gap.
  4. Move existing in-file `#[cfg(test)]` tests with their handlers.
- **Detail**: Effort M. Refactor + 1 new endpoint (DELETE) + future-router stub scaffolding. `AppState` lives in `crate::app::AppState`; grep shows zero `crate::http::AppState` consumers. The only public symbol `mod.rs` must continue to expose is `pub fn router() -> Router<AppState>` (consumed at `app.rs:159`). `app.rs` should not need changes beyond the import path.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds (stubs compile); `cargo test --manifest-path lumina/Cargo.toml` succeeds (every existing http-layer test passes; one new test asserts `DELETE /work-items/{id}` returns 204 and the row is `deleted_at`-stamped on re-read); `cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings` clean.
- **Blocked-by**: none.

### Phase 2: Backend per-family routes (parallel — 4 agents, each owns its own files)

#### T2: Structured patches — scalars + story plan + task spec + task kind + tier
- **Files**: `lumina/src/http/structured_patches.rs` (own — fill the empty stub T1 pre-declared), `lumina/tests/e2e.rs` (extend with one new `#[tokio::test]` for this family).
- **Action**: Add 10 PATCH handlers — `PATCH /work-items/{id}/relevance`, `…/effort`, `…/complexity`, `…/closure-gate`, `…/task-kind`, `…/tier`, `…/story-plan`, `…/task-spec`. Each handler delegates to its `repo::*` setter. Define `Patch<EnumName>Body { value: Option<EnumName> }` for scalars (so a `null` value clears to NULL, mirroring `repo::set_task_tier(pool, id, Option<Tier>)` at `repo.rs:4218`). Define `SetStoryPlanBody` / `SetTaskSpecBody` mirroring the MCP Params for the structured ones. **Write the family smoke test `scalars_round_trip_http`** in `e2e.rs` exercising one mutation per route + a `GET /work-items/{id}` re-read.
- **Detail**: Effort M. Returns 200 + `Json<WorkItem>` (re-fetched) on success — matches the existing PATCH /work-items/{id} return convention. 404 on unknown id; 422 on invalid enum or hierarchy violation. **Body limit**: `story-plan` / `task-spec` payloads inherit axum's 2 MiB `DefaultBodyLimit` for `Json<T>` — acceptable for current planning prose; raise via `.layer(DefaultBodyLimit::max(8 * 1024 * 1024))` on this router if a future plan body exceeds it.
- **Acceptance**: `scalars_round_trip_http` in `e2e.rs` passes (covers POST/PATCH per route + GET re-read); `cargo test` + `cargo clippy` clean; `cd lumina && cargo sqlx prepare --check` exits 0.
- **Blocked-by**: T1.

#### T3: Acceptance criteria + research notes
- **Files**: `lumina/src/http/acceptance_criteria.rs` (own — fill stub), `lumina/src/http/research_notes.rs` (own — fill stub), `lumina/tests/e2e.rs` (extend with two `#[tokio::test]` functions for these families).
- **Action**: 7 routes total. AC: `POST /work-items/{id}/acceptance-criteria` (create), `POST /acceptance-criteria/{id}/check` (with `{by}` body), `POST /acceptance-criteria/{id}/uncheck`, `DELETE /acceptance-criteria/{id}`. Research notes: `POST /work-items/{id}/research-notes`, `PATCH /research-notes/{id}`, `POST /research-notes/{old_id}/supersede/{new_id}`. **Write `acceptance_criteria_round_trip_http` and `research_notes_round_trip_http` smoke tests** in `e2e.rs`.
- **Detail**: Effort M. AC check appends a verification activity (handled inside `repo::check_acceptance_criterion`). Supersession is one txn / one event (handled inside `repo::supersede_research_note`). Returns 200 + re-fetched detail row for state-changing ops; 204 only for delete.
- **Acceptance**: Both smoke tests pass; `cargo test` + `cargo clippy` clean; `cd lumina && cargo sqlx prepare --check` exits 0.
- **Blocked-by**: T1.

#### T4: Risks + rejected alternatives + task dependencies
- **Files**: `lumina/src/http/risks.rs` (own — fill stub), `lumina/src/http/rejected_alternatives.rs` (own — fill stub), `lumina/src/http/task_dependencies.rs` (own — fill stub), `lumina/tests/e2e.rs` (extend with three `#[tokio::test]` functions).
- **Action**: 10 routes total. Risks: `POST /work-items/{id}/risks`, `PATCH /risks/{id}`, `POST /risks/{old_id}/supersede`, `DELETE /risks/{id}`. Rejected alternatives: same 4-shape under `/work-items/{id}/rejected-alternatives` and `/rejected-alternatives/{id}`. Task deps: `POST /work-items/{task_id}/depends-on/{depends_on_id}` (body: `{kind: "data"}`), `DELETE /work-items/{task_id}/depends-on/{depends_on_id}`. Reads: `GET /work-items/{story_id}/task-dependencies` (list), `GET /work-items/{story_id}/task-batches` (compute_task_batches). **Write `risks_round_trip_http`, `rejected_alternatives_round_trip_http`, `task_dependencies_round_trip_http` smoke tests** — the last includes a cycle-422-with-edges assertion.
- **Detail**: Effort M. `compute_task_batches` returns `Vec<Vec<String>>` directly as JSON. Cycle error propagates as 422 with the `edges` field (existing `AppError::Cycle` mapping in `error.rs`).
- **Acceptance**: All three smoke tests pass (incl. cycle 422 + edges body); `cargo test` + `cargo clippy` clean; `cd lumina && cargo sqlx prepare --check` exits 0.
- **Blocked-by**: T1.

#### T5: Open questions + findings + activity + context blocks + readiness/dispatch
- **Files**: `lumina/src/http/open_questions.rs` (own — fill stub), `lumina/src/http/findings.rs` (own — fill stub), `lumina/src/http/activity.rs` (own — fill stub), `lumina/src/http/context_blocks.rs` (own — fill stub), `lumina/src/http/readiness.rs` (own — fill stub), `lumina/tests/e2e.rs` (extend with five `#[tokio::test]` functions covering this family group).
- **Action**: 13 routes total. Open questions: `POST /work-items/{story_id}/open-questions`, `POST /open-questions/{id}/options`, `POST /work-items/{task_id}/block-on-question/{question_id}`, `PATCH /work-items/{task_id}/enabling-option/{option_id}`, `POST /open-questions/{id}/resolve` (body: `{chosen_option_id, by}`). Findings: `POST /work-items/{id}/findings`, `PATCH /findings/{id}`, `POST /findings/{id}/resolve` (body: `{disposition, resolution, rationale}`), `POST /findings/{old_id}/supersede/{new_id}`. Activity: `POST /work-items/{id}/activity`. Context blocks: `POST /context-blocks`, `POST /work-items/{id}/context-blocks/{cb_id}` (link), `DELETE /work-items/{id}/context-blocks/{cb_id}` (unlink). Readiness/dispatch: `GET /work-items/{story_id}/readiness`, `GET /work-items/{story_id}/dispatch-plan`. **Write `open_questions_round_trip_http`, `findings_round_trip_http`, `activity_log_round_trip_http`, `context_blocks_round_trip_http`, `readiness_and_dispatch_http` smoke tests**.
- **Detail**: Effort M. Five sub-files = at the 6-file cap; does not split further. **Activity handler delegates fully to `repo::append_activity`** which accepts all 9 `ActivityType` variants (verified via `repo::validate_entry_kind` at `repo.rs:72-83`) — no HTTP-layer allowlist; human-vs-orchestrator UX distinctions belong in the FE composable / form layer where the UI for human-driven activity lives. **`GET /dispatch-plan` is N+1 by design** (Kahn's batch composition + per-task spec reads ≈ N SELECTs per call) — acceptable for current single-user posture; revisit caching/ETag if a future sprint-composer UI polls aggressively.
- **Acceptance**: All five smoke tests pass; `cargo test` + `cargo clippy` clean; `cd lumina && cargo sqlx prepare --check` exits 0.
- **Blocked-by**: T1.

### Phase 3: Backend verification sweep (sequential after Phase 2)

#### T6: Verification sweep + e2e shape check
- **Files**: `lumina/tests/e2e.rs` (audit; optionally add a single insta snapshot of `WorkItemDetail` shape).
- **Action**: The 11 per-family smoke tests are written inside their owning tasks (T2–T5) — Phase 3 is a SWEEP, not a test-authoring task. Run the full verification chain: `cargo nextest run`, `cargo clippy --all-targets -- -D warnings`, `cargo sqlx prepare --check`. Audit `e2e.rs` for any cross-family coverage gaps (e.g. a `WorkItemDetail` JSON-shape locked snapshot via `insta::assert_json_snapshot!` would catch silent wire drift). **Testing stack opt-outs documented**: per-family handwritten tests are kept (clearer to read than rstest tables given setup divergence per family); insta snapshot is OPTIONAL (cargo-insta review adds CI ceremony) — apply only if a `WorkItemDetail` field is forgotten in T2–T5. Proptest is intentionally NOT used for the cycle case (the deterministic round-trip test in T4 is sufficient evidence).
- **Detail**: Effort S. Verification-only; no new code unless an audit gap surfaces.
- **Acceptance**: Full verification chain green: `cargo nextest run --manifest-path lumina/Cargo.toml`, `cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings`, `cd lumina && cargo sqlx prepare --check` (benign `unused queries` warning expected — do NOT regenerate without `--all-targets`).
- **Blocked-by**: T2, T3, T4, T5.

### Phase 4: Frontend scaffolding (sequential, can start after T1 but depends on T1 only logically — really after Phase 3 for confidence)

#### T7: Split `lumina/web/src/api.ts` + extend schemas + pre-declare per-family modules
- **Files**: `lumina/web/src/api.ts` (delete after split), `lumina/web/src/api/index.ts` (new), `lumina/web/src/api/http.ts` (new), `lumina/web/src/api/wire-enums.ts` (new), `lumina/web/src/api/work-items.ts` (new), `lumina/web/src/api/repo-links.ts` (new). Also `lumina/web/src/composables/useHierarchy.ts` and `useRepoLinks.ts` (update imports only).
- **Action**:
  1. Move existing schemas/wrappers into the per-family files.
  2. Add wire enums for `TaskKind` (`foundation|main|polish`, kebab-case per migration 0007), `Tier` (`lite|deep`), `RiskSeverity` (`low|medium|high|critical` — deliberately distinct from `Severity`).
  3. **Nullable invariant**: extend `WorkItemSchema` with `task_kind: TaskKindSchema.nullable()` and `tier: TierSchema.nullable()`. ALL future WorkItem field additions MUST be `.nullable()` to preserve compile-compatibility for the eleven downstream consumers that import `WorkItem` from `@/api` (`ChildCard.vue`, `ChildGrid.vue`, `FocusLens.vue`, `HierarchySpine.vue`, `SpineNode.vue`, `RepoLinksPanel.vue`, `treeUtils.ts`, `useHierarchy.ts`, `useRepoLinks.ts`, `repoTag.ts`).
  4. **Zod nullable verification**: zod 4 `.nullable()` accepts `null` but rejects missing/undefined. `lumina/src/domain.rs` has zero `skip_serializing_if` attributes, so serde emits `None` as JSON `null` and `.nullable()` is correct. VERIFY during implementation with `curl /api/work-items/<id>` against a work-item that has `task_kind` unset; if the field is absent rather than `null`, switch to `.nullish()` for affected fields.
  5. Extend `WorkItemDetailWireSchema` with `risks: z.array(RiskSchema).default([])`, `rejected_alternatives: z.array(RejectedAlternativeSchema).default([])`, `task_dependencies: z.array(TaskDependencySchema).default([])` — declared inline in `work-items.ts`; T9/T10 move them to per-family files.
  6. **Pre-declare every Phase-5 family in `index.ts` up front.** Write empty per-family files for every future family (scalars, structured-patches, readiness, acceptance-criteria, research-notes, risks, rejected-alternatives, task-deps, open-questions, findings, activity, context-blocks) and add their `export * from './<family>'` lines to `index.ts`. Each Phase-5 task then only fills in its owned file body — `index.ts` is touched ONCE here and NOT in subsequent tasks.
- **Detail**: Effort S. `index.ts` re-exports everything so `import { fetchTree } from '@/api'` continues working.
- **Acceptance**: `cd lumina/web && bun test` passes (existing `showcase.test.ts`/`smoke.test.ts`/`repoTag.test.ts` exercise the schemas); `cd lumina/web && npm run build` succeeds; **explicit verification: all existing `from '@/api'` imports continue resolving under both `bun test` and `vite build`** (the `@/` alias is set in `vite.config.ts` and `tsconfig.json`; bun-test's module resolver MUST honour the same path-mapping or every consumer breaks).
- **Blocked-by**: T1 (logically; new endpoints in T2–T5 aren't strictly required for T7's scope but T7 is easier to land after backend is built).

### Phase 5: Frontend per-family wire + composables (parallel — 4 agents)

#### T8: Scalars + story plan + task spec + task kind/tier + readiness/dispatch
- **Files**: `lumina/web/src/api/scalars.ts` (new), `lumina/web/src/api/structured-patches.ts` (new — story-plan/task-spec wrappers), `lumina/web/src/api/readiness.ts` (new), `lumina/web/src/composables/useScalars.ts` (new), `lumina/web/src/composables/useStoryPlan.ts` (new), `lumina/web/src/composables/useTaskSpec.ts` (new), `lumina/web/src/composables/useReadiness.ts` (new), `lumina/web/src/composables/useDispatchPlan.ts` (new). Total 8 files = above the 6-file cap → SPLIT into T8a + T8b.
- **Action (T8a)**: Scalars + task kind + tier. `api/scalars.ts` exports `setRelevance/setEffort/setComplexity/setClosureGate/setTaskKind/setTier` wrappers + composables `useScalars.ts`. Files: 3.
- **Action (T8b)**: Story plan + task spec + readiness + dispatch. `api/structured-patches.ts` + `api/readiness.ts` + 4 composables. Files: 6.
- **Detail**: Effort M each. Wrappers `handle<T>(WorkItemSchema)` for scalar PATCHes (since they return the re-fetched item); use new `StoryReadinessSchema` and `BatchEntrySchema` for the two reads.
- **Acceptance**: New bun tests cover happy + error paths; `bun test` and `npm run build` clean.
- **Blocked-by**: T7.

#### T9: Acceptance criteria + research notes
- **Files**: `lumina/web/src/api/acceptance-criteria.ts` (own — fill stub), `lumina/web/src/api/research-notes.ts` (own — fill stub), `lumina/web/src/composables/useAcceptanceCriteria.ts` (new), `lumina/web/src/composables/useResearchNotes.ts` (new). 4 files. **Note**: `index.ts` already re-exports these per T7's pre-declaration; no edit to `index.ts` here.
- **Action**: Schemas mirror Rust types (already in domain.rs); wrappers mirror MCP tool params; composables follow the module-singleton pattern. Move the `AcceptanceCriterion`/`ResearchNote` schemas from T7's `work-items.ts` (declared inline there) into these per-family files; update `work-items.ts` to import them.
- **Acceptance**: Bun tests + `npm run build` clean.
- **Blocked-by**: T7.

#### T10: Risks + rejected alternatives + task deps
- **Files**: `lumina/web/src/api/risks.ts` (own — fill stub), `lumina/web/src/api/rejected-alternatives.ts` (own — fill stub), `lumina/web/src/api/task-deps.ts` (own — fill stub), `lumina/web/src/composables/useRisks.ts` (new), `lumina/web/src/composables/useRejectedAlternatives.ts` (new), `lumina/web/src/composables/useTaskDependencies.ts` (new). 6 files (at the cap). **Note**: `index.ts` already re-exports these per T7's pre-declaration.
- **Action**: Define `RiskSchema`, `RejectedAlternativeSchema`, `TaskDependencySchema`, `BatchEntrySchema` (the latter for compute_task_batches result rows where used). Move them out of inline `work-items.ts` declarations into these files. Wrappers cover full CRUD + supersession + cycle-error parse (when 422 returned with `error.edges`, surface to composable as `{ ok: false, error: { kind: 'cycle', edges } }`).
- **Acceptance**: Bun tests including a cycle-422 parse test; `npm run build` clean.
- **Blocked-by**: T7.

#### T11: Open questions + findings + activity + context blocks
- **Files**: `lumina/web/src/api/open-questions.ts` (new), `lumina/web/src/api/findings.ts` (new), `lumina/web/src/api/activity.ts` (new), `lumina/web/src/api/context-blocks.ts` (new), `lumina/web/src/composables/useOpenQuestions.ts` (new), `lumina/web/src/composables/useFindings.ts` (new), `lumina/web/src/composables/useActivity.ts` (new), `lumina/web/src/composables/useContextBlocks.ts` (new). 8 files → SPLIT into T11a + T11b.
- **Action (T11a)**: Open questions + findings. 4 files.
- **Action (T11b)**: Activity + context blocks. 4 files.
- **Acceptance**: Bun tests + `npm run build` clean.
- **Blocked-by**: T7.

### Phase 6: Frontend tests (parallel after Phase 5 — 2 agents)

#### T12a: Bun tests — scalars / AC / research / open-questions / risks / rejected
- **Files**: `lumina/web/src/__tests__/scalars.test.ts` (new), `acceptance-criteria.test.ts` (new), `research-notes.test.ts` (new), `open-questions.test.ts` (new), `risks.test.ts` (new), `rejected-alternatives.test.ts` (new). 6 files (at the cap).
- **Action**: Per-family schema fixture validation, fetch wrapper happy + error path, composable mutation flow with mocked api. Mirror existing `showcase.test.ts` and `smoke.test.ts` patterns. Use bun's `mock` to stub `fetch`.
- **Detail**: Effort M.
- **Acceptance**: `cd lumina/web && bun test` runs all suites green.
- **Blocked-by**: T8a, T8b, T9, T10.

#### T12b: Bun tests — task-deps / findings / activity / context-blocks / readiness
- **Files**: `lumina/web/src/__tests__/task-deps.test.ts` (new), `findings.test.ts` (new), `activity.test.ts` (new), `context-blocks.test.ts` (new), `readiness.test.ts` (new). 5 files.
- **Action**: Same patterns as T12a. Task-deps includes a cycle-422 parse test.
- **Detail**: Effort M.
- **Acceptance**: `cd lumina/web && bun test` runs all suites green; coverage report (if collected) does not regress.
- **Blocked-by**: T10, T11a, T11b.

### Phase 7: Documentation (sequential after T12)

#### T13: Update CLAUDE.md route catalogue + plugin SKILL.md backlink
- **Files**: `lumina/CLAUDE.md` (add `## HTTP routes` section), `lumina/web/CLAUDE.md` (note the `api/` directory layout if it deserves one), `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` (add one-line backlink `HTTP equivalents: lumina/CLAUDE.md#http-routes`), `CLAUDE.md` (repo-root — append a one-line note to the MCP tool families paragraph: "HTTP route mirror landed in round-4; see `lumina/CLAUDE.md`").
- **Action**: Add a `## HTTP routes` section in `lumina/CLAUDE.md` enumerating the new route families with the established `<method> <path> → repo::<fn>` form. Note that every HTTP write delegates to a single `repo::*` mutation (preserving the single-mutation-path invariant). Add a one-line backlink in the plugin's MCP `SKILL.md` (the authoritative MCP catalogue) so agents discovering MCP tools also see the HTTP equivalents. Touch the repo-root `CLAUDE.md` MCP paragraph likewise.
- **Detail**: Effort S. Documentation only — no code changes.
- **Acceptance**: A reader scanning `lumina/CLAUDE.md` can locate any new route family within ~30s; the plugin's MCP `SKILL.md` and repo-root `CLAUDE.md` carry the backlink.
- **Blocked-by**: T12a, T12b.

## Dependency Graph

```
T1
├── T2, T3, T4, T5     (parallel — each writes its own family smoke test in e2e.rs)
│   └── T6              (verification sweep; no test authoring)
│       └── T7          (FE scaffold; pre-declares api/index.ts re-exports for all Phase-5 families)
│           ├── T8a, T8b, T9, T10, T11a, T11b   (parallel — 6 agents, batched into 2 waves of 3)
│           │   ├── T12a   (parallel after Phase 5)
│           │   └── T12b   (parallel after Phase 5)
│           │       └── T13
```

Notes:
- T1 and T7 are the only tasks that touch the shared composition files (`http/mod.rs` and `api/index.ts`); all subsequent parallel tasks fill in their own pre-declared stubs. This eliminates the Phase-2 / Phase-5 shared-file contention.
- Per-family smoke tests live inside their owning task (T2–T5), not a separate test-authoring task. T6 is a verification sweep only.
- Phase 5 has 6 parallel-eligible tasks. /plan-new caps per-batch agent count at 3–4. Recommend /implement batches as: wave 1 = {T8a, T9, T10}; wave 2 = {T8b, T11a, T11b}.
- T12a and T12b are file-disjoint and parallel-safe.

## Verification

End-to-end smoke after T13:

1. `cargo build --manifest-path lumina/Cargo.toml` → green.
2. `cargo nextest run --manifest-path lumina/Cargo.toml` → all e2e tests (existing + per-family smoke tests written by T2-T5) pass.
3. `cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings` → clean.
4. `cd lumina && cargo sqlx prepare --check` → exits 0 (benign `unused queries` warning expected, do NOT regenerate).
5. `cd lumina/web && bun test` → all suites green.
6. `cd lumina/web && npm run build` → bundle builds.
7. Manual: start lumina (`cargo run --manifest-path lumina/Cargo.toml`), open SPA in browser, confirm the existing tree view still loads (no UI regression — UI doesn't yet consume the new endpoints, but `WorkItemDetail` now includes new aggregates which existing FE schemas must tolerate via the `.default([])` zod fallback added in T7).
8. Manual: `curl -X POST http://127.0.0.1:24817/api/work-items/<story-id>/risks -H 'content-type: application/json' -d '{"summary":"smoke","severity":"medium"}'` then `curl http://127.0.0.1:24817/api/work-items/<story-id>` and confirm the risk appears in the detail's `risks` array.

## Risks

1. **Scope size (~28 files, 13 tasks)** — beyond /plan-new's ~15-file flag. Mitigation: T1 pre-declares one `.merge(<family>::router())` line per Phase-2 family in `http/mod.rs`, and T7 pre-declares one `export * from './<family>'` line per Phase-5 family in `api/index.ts`. Each parallel agent then edits only its owned file body; the router-composition and re-export files are touched once (by T1/T7), not by every parallel task. /implement can land Phase 1–3 (backend) as a logical "round-4-backend" sub-milestone and Phase 4–7 (frontend) as "round-4-frontend" if the user wants to checkpoint. Currently bundled into one plan per the user's explicit request.
2. **Cross-task schema declarations (T7 ↔ T9/T10)** — `WorkItemDetailWireSchema` references `RiskSchema`/`RejectedAlternativeSchema`/`TaskDependencySchema` which logically belong in T9/T10's per-family files. Mitigation: T7 declares them inline in `work-items.ts`; T9/T10 move them to per-family files and re-export. The move is a search-and-replace within owned files, no contention.
3. **Cycle error round-trip** — task-deps cycle is the only HTTP route that surfaces a structured error field (`edges`). Easy to forget to test the FE parse side. Mitigation: T10 explicitly mandates a cycle-422 parse test.
4. **`severity` vocab confusion** — `Severity` (findings) and `RiskSeverity` (risks) share the word "severity" but have different vocabularies. Mitigation: separate schema names (`SeveritySchema` vs `RiskSeveritySchema`), both exported from `wire-enums.ts` with explicit doc comments; T10 bun tests assert both vocabularies decode in their respective contexts.
5. **axum default body limit** — `Json<T>` enforces a 2 MiB `DefaultBodyLimit` (axum 0.8). `set_story_plan` / `set_task_spec` payloads are well under this for normal planning prose, but `task-spec.files_touched` could in principle grow. Mitigation: documented in T2's Detail; raise via `.layer(DefaultBodyLimit::max(8 * 1024 * 1024))` on the structured-patch router if a future plan body exceeds the cap.
6. **`GET /dispatch-plan` is N+1 by design** — Kahn's batch composition + per-task spec reads ≈ N SELECTs per call. Acceptable for current single-user posture; revisit caching/ETag if a future sprint-composer UI polls aggressively. Mitigation: documented in T5's Detail as a known characteristic.
7. **Zod `.nullable()` vs `.nullish()` divergence** — backend likely emits `null` for unset Option fields (zero `skip_serializing_if` in `domain.rs`), but verify with `curl` during T7 implementation. Mitigation: T7 Action step 4 mandates the verification before locking in `.nullable()`.
