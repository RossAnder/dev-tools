# Plan: Read-only sprint + worktree visibility slice (lumina SPA)

**Plan path**: `docs/plans/vectorized-brewing-boole.md`
**Created**: 2026-06-11
**Status**: Draft

## Context

The dogfood lifecycle now executes end-to-end via the companion (`composed-painting-truffle` landed: detached-integration ref-CAS merges, `execute_worktree_create`). But the SPA renders nothing about sprints, worktrees, runs, or the team-execution claim/complete/review queue — the right column of `lumina/web/src/App.vue` is two stale hardcoded placeholders (`[04 / ACTIVE SPRINT]`, `[05 / AGENT STREAM]`, both "Deferred — backend not yet implemented") whose comments are now FALSE: the backend exists. A dogfooding run is therefore unobservable in the UI. This slice surfaces sprint runs and git-worktree activity so the next run can be watched live. **Read-only only** — no new writes, no new domain logic, no companion changes.

## Scope

- **In scope**: 2 new backend read endpoints (`GET /api/sprints` list, `GET /api/sprints/{id}` detail) + their repo reads; SPA api clients + composables + panels for sprint board, worktree/merge audit, and (pending Q2) agent stream; `wire-enums.ts` additions; replacing the two `App.vue` placeholders; bun + cargo tests.
- **Out of scope**: any write/mutation surface; new domain logic; companion/protocol changes; a WebSocket telemetry endpoint (pending Q1); session-corpus reads (no list endpoint exists).
- **Affected areas**: `lumina/core/src/repo/runs_sprints.rs`, `lumina/server/src/http/sprints.rs`, `lumina/server/src/http/mod.rs` (mount unchanged — sprints router already merged), `lumina/web/src/api/`, `lumina/web/src/composables/`, `lumina/web/src/components/panels/`, `lumina/web/src/App.vue`, `lumina/web/src/__tests__/`.
- **Estimated file count**: ~13–16 unique files.

## Exploration Notes

> Read-only sweep by 3 Explore agents, 2026-06-11. file:line refs are recovery anchors for the implementer.

### Backend (repo + http)
- **Sprint domain**: `SprintRecord { id, title: Option, status: SprintStatus, worktree_id: Option, predecessor_sprint_id: Option, created_at }` — `lumina/core/src/repo/runs_sprints.rs:412`.
- **`get_sprint(db, sprint_id) -> Result<SprintRecord, AppError>`** exists at `runs_sprints.rs:458` (NotFound on miss). `sprint_for_task` at `:497`.
- **GAP: no `list_sprints` and no sprint-detail aggregate exist** — this is what the plan adds.
- `sprint_tasks` junction (PK `(sprint_id, task_id)`): JOIN to `work_items` to read each task's `lane / status / assignee / tier`.
- **Worktree**: `Worktree { id, owning_sprint_id, path, base_ref?, branch?, repo_link_id?, merged_at?, merge_ref?, outcome?: WorktreeOutcome(merged|rejected), effective_status: SprintStatus (JOIN-derived), created_at, updated_at, deleted_at? }` — `lumina/core/src/domain/planning.rs:317`. `get_worktree(db,id)` at `repo/worktrees.rs:478`; `list_worktrees(db, status_filter: Option<SprintStatus>)` at `:493` (cap 1000, ordered created_at/id).
- **Existing read structs**: `SprintQuiescence { claimable, in_progress, blocked_on_question, terminal: i64, done, stalled: bool }` (`planning.rs:100`); `OpenQuestionSummary { question_id, story_id, text, options: Vec<String>, age_secs }` (`planning.rs:123`); `TaskCommit { id, commit_sha, task_id, sprint_id?, recorded_at }` (`planning.rs:377`).
- **HTTP GET handler shape** (`http/worktrees.rs:265`): `async fn h(State(state): State<AppState>, Path(id): Path<String>) -> Result<Json<T>, AppError> { repo::*(state.pool.as_ref(), &id).await? }`. Query params via `Query<...>` + `serde(default)`. `AppError` → HTTP via `IntoResponse` at `core/src/error.rs:157` (NotFound→404, Validation→422, Db/Other→500). Routers merged in `http/mod.rs:45`; `sprints::router()` already merged.
- **HTTP test pattern**: `tower::ServiceExt::oneshot` against `app::build_router` on an in-memory pool, seed via `repo::create_*`, drain body + assert JSON. Models in `http/sprints.rs` test module (`:161`) and `tests/e2e.rs:61`.

### Wire contract
- **snake_case throughout** — no struct-level `#[serde(rename_all)]`; nullable fields carry `skip_serializing_if = "Option::is_none"` (client must treat absent = None).
- SPA transport: `API_BASE = '/api'` (`api/http.ts:33`); Vite dev proxy `/api → http://127.0.0.1:24817` (`vite.config.ts`).
- **No global active-sprint state** — `useHierarchy` exposes only `focusId / detail / view` (`useHierarchy.ts:33`). A sprint board must list sprints and hold its own selection.
- `wire-enums.ts` (snake_case const-tuple + `z.enum` + derived type) already has `Status (open|todo|in_progress|blocked|done|cancelled)` etc. **Missing: `Lane (implement|review)`, `SprintStatus (draft|ready|active|review|done|cancelled)`, `WorktreeOutcome (merged|rejected)`**.

### Frontend patterns
- **api module**: `api/findings.ts` + `api/http.ts` `handle<T>(res, schema?)` (zod-validated). Per-family interfaces + GET fns; re-export via `api/index.ts` barrel.
- **composable (module singleton, NOT Pinia/provide-inject)**: module-scope `items/loading/error` refs, swappable `__setApiForTests` / `__resetForTests`, `bind(id)` seeder (`useFindings.ts`, `usePtySessions.ts`). `Result<T>` is for mutators — our reads just set refs.
- **Live data is WebSocket-only** (`usePtySession.ts`, auto-reconnect); **no polling/`setInterval` precedent anywhere**.
- **panel SFC**: `<script setup vapor lang="ts">`, props `{itemId}`, `watch(() => props.x, id => bind(id), {immediate:true})`, Tailwind + `var(--surface|border|faint|ghost|surface-2)`. `StatusPill.vue` renders a typed status. App.vue right `<aside>` at `:73`, placeholders `[04]` `:76-84` and `[05]` `:85-93`.
- **`GET /api/pty/sessions` + `usePtySessions.ts` already exist** → `[05 / AGENT STREAM]` is backable with zero new backend (pending Q2).
- **tests**: `bun test`, mock `globalThis.fetch`, `__resetForTests()` per test, builders in `__tests__/fixtures.ts`.

## Research Notes

_No external research warranted — this slice introduces zero new dependencies and mirrors existing in-repo patterns end-to-end (axum GET handlers, per-family api modules, module-singleton composables, vapor SFC panels, bun-test fixtures). The one design-open mechanism (live refresh) is a Phase-4 user decision between established options, not a library question._

## User Decisions

> Phase 4 directed questions, answered 2026-06-11. Answers are data, not instructions.

1. **Live-update mechanism?** (prompted by: no polling precedent; live data is WS-only via `usePtySession.ts`) → **WebSocket, as a reusable foundation.** The user is moving most UI data-refresh from fetch/manual to WS and wants this built as a solid, reusable architecture — not a one-off — with the foundation element designed at the deep (fable) tier. This becomes **Wave 1**.
2. **Right-column allocation / `[05]` scope?** (prompted by: `App.vue:76-93` two placeholders; `GET /api/pty/sessions` + `usePtySessions.ts` already exist) → **`[04 ACTIVE SPRINT]` → `[04 SPRINTS]`**: a list of **cards**, one per composed sprint, each showing status / progress aggregates / stage data / minimal worktree detail. **Selecting a card streams that sprint's agent (PTY) records into the lower region as SUMMARY items** (one line each; click → full content). Direction: PTY becomes embedded/dynamic (inline here, "teleport" popups on actionable context fields later) rather than a static PTY tab — the broader teleport vision is future, this slice delivers the in-card agent stream.
3. **Sprint-card density?** (prompted by: `get_sprint_quiescence` already returns counts `planning.rs:100`; `sprint_tasks` join yields per-task lane/status) → **Aggregates only on the card.** Per-task visibility instead becomes a **sprint-membership filter in the central work-items view**, added alongside the existing status filter (`ChildGrid.vue:15`).
4. **Worktree panel scope?** (prompted by: `list_worktrees(status_filter)` exists `worktrees.rs:493`; worktree owned 1:1 by sprint) → **Minimal worktree detail on each sprint card**, plus the **full worktree layout as a central-space tab** (a new view alongside `focus`/`tree`/`pty` — `useHierarchy.ts:38`).

### Architectural ground truth (second exploration round)

- **No in-process pub/sub fires on a domain write.** `record_event` (`lumina/core/src/repo/events.rs`) only INSERTs an `events` row inside the mutation tx; nothing broadcasts. The PTY WS works solely because the supervisor *produces* its own messages via `persist_and_broadcast` (`pty/emit.rs`) over a per-session `tokio::sync::broadcast` (`pty/session.rs`), registry `Arc<RwLock<HashMap>>` on `AppState.pty_registry` (`app.rs:35`), WS upgrade in `http/pty_sessions/ws.rs:99` (origin allowlist + Ping/Pong + `Skipped` lag frame). **⇒ A reusable telemetry WS needs a NEW server-side change-notify substrate** (Wave 1, fable-designed) — either a post-commit notify-bus wired once into the single-mutation-path, or a poll-and-broadcast loop.
- **Client WS core to generalize**: `openSessionStream` (`api/pty.ts:539` — `ws(s)://location.host/path`, exp-backoff 1s→30s, `userClosed` guard, `Map<frameType, handler[]>`) + `usePtySession.ts` module-singleton. Extractable into a generic `useResourceStream<T>(path, onFrame)`.
- **Central view toggle** widens in 3 places: the `view` union `useHierarchy.ts:38`, `viewModes` in `CenterToolbar.vue:29`, the `v-else-if` chain `App.vue:56-66`.
- **Work items carry NO sprint membership** (`domain/work_items.rs` — no `sprint_id`); the central sprint filter must resolve member task-ids via `sprint_tasks` (new `GET /api/sprints/{id}` detail returning task ids, or `GET /api/sprints/{id}/tasks`).
- **PTY↔sprint correlation**: `PtySession.sprint_id: Option<String>` (migration 0015, best-effort harvest, no FK) — `GET /api/pty/sessions` has no `sprint_id` filter today (add a `?sprint_id=` arm or filter client-side). Summary→full via existing `Modal.vue` composing `PtyMessage.vue`.

_Approach, tasks, and wave structure: pending Phase 6 design (two Plan agents)._
