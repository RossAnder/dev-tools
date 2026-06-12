# Plan: Read-only sprint + worktree visibility slice (lumina SPA)

**Plan path**: `docs/plans/vectorized-brewing-boole.md`
**Created**: 2026-06-11
**Status**: Draft

## Context

The dogfood lifecycle now executes end-to-end via the companion (`composed-painting-truffle` landed: detached-integration ref-CAS merges, `execute_worktree_create`). But the SPA renders nothing about sprints, worktrees, runs, or the team-execution claim/complete/review queue — the right column of `lumina/web/src/App.vue` is two stale hardcoded placeholders (`[04 / ACTIVE SPRINT]`, `[05 / AGENT STREAM]`, both "Deferred — backend not yet implemented") whose comments are now FALSE: the backend exists. A dogfooding run is therefore unobservable in the UI. This slice surfaces sprint runs and git-worktree activity so the next run can be watched live. **Read-only only** — no new writes, no new domain logic, no companion changes.

## Scope

- **In scope**: 2 new backend read endpoints (`GET /api/sprints` list, `GET /api/sprints/{id}` detail) + their repo reads; SPA api clients + composables + panels for sprint board, worktree/merge audit, and (pending Q2) agent stream; `wire-enums.ts` additions; replacing the two `App.vue` placeholders; bun + cargo tests.
- **Out of scope**: any write/mutation surface; new domain logic; companion/protocol changes; a WebSocket telemetry endpoint (pending Q1); session-corpus reads (no list endpoint exists).
- **Affected areas**: `lumina/core/src/` (`notify.rs` new, `db/`, `repo/{events,runs_sprints,pty}.rs`), `lumina/server/src/` (`stream/` new, `http/{stream,sprints,ws_common,pty_sessions}.rs`, `app.rs`), `lumina/web/src/{api,composables,components,__tests__}/`, `lumina/web/src/App.vue`, `lumina/CLAUDE.md`.
- **Estimated file count**: **~22 source files + ~10 test files across 5 waves** — well over the ~15-file single-plan guideline. This is DELIBERATE and STAGED: the WS foundation (Wave 1) is the reusable-architecture investment the user asked for, and each wave is an independently implementable + reviewable `/implement` boundary. **Recommended execution: one `/implement` run per wave** (Wave 1 → 2a → 2b → 3 → 4), not a single pass. Waves 1+2 are the MVP that makes a dogfood run observable; 3+4 layer on the agent stream and worktrees tab.

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

## Approach

Two Plan agents designed this (the WS foundation on the deep/fable tier per User Decision 1). The slice is one reusable-foundation wave plus four consumer waves.

**Wave 1 — reusable WS data-refresh foundation (the architecture investment).**
- **Change signal: a post-commit notify-bus (chosen over poll-and-broadcast).** Every `repo::*` mutator already funnels through one `record_event(tx, …)` (`core/src/repo/events.rs` — the single place that knows `aggregate_type`/`aggregate_id`) and one shared `DbTx::commit` impl (`core/src/db/erased.rs`). So: `record_event` calls a new **no-op-default** `DbTx::note_change(n)` that BUFFERS a `ChangeNotification` on the tx; the shared `commit()` runs `inner.commit().await?` FIRST and only then publishes the buffer to a process-wide `tokio::sync::broadcast` (`core/src/notify.rs`, `OnceLock<NotifyBus>`, cap 1024). Best-effort, non-awaiting, errors ignored; a rollback/drop discards the buffer ⇒ never a phantom signal; **zero mutator edits, automatic for all ~100 existing + every future write.** Publishing from inside `record_event` would push pre-commit state (WAL isolation) — the buffer-until-commit is load-bearing, not stylistic.
  - *Rejected: poll-and-broadcast* (per-resource recompute registration + a permanent poll loop + 1–2 s latency floor — the opposite of "adopt with minimal code" for a foundation meant to carry most UI data). *Rejected: events-table watermark tail* (survives a future 2nd writer process but adds poll wakeups; can be slotted behind `NotifyBus` later with no consumer change).
- **Server endpoint: ONE multiplexed `GET /api/stream`** (not per-resource sockets — a new resource then costs zero routes). The client sends `{type:"subscribe",topic:"sprint-quiescence:<id>"}`; the server replies `init` (full snapshot) on subscribe, then `data` (full snapshot, deduped-on-equal) on change, plus `skipped` on bus lag, `error`, `pong`. **Snapshots, never deltas** ⇒ a missed frame self-heals on the next push and reconnect is race-free (init-on-subscribe + resubscribe-all-on-reconnect). A new resource implements a small `TopicResolver { prefix, interested(may over-approximate), resolve(read-only) }` registered in a `TopicRegistry`; a 150 ms coalesce window + cheap recompute + push-only-if-changed absorbs the over-approximation. Reuses the PTY ws machinery (origin allowlist — extracted to `http/ws_common.rs` — `CancellationToken`, two-task split, `Skipped`-on-lag). `AppState` gains defaulted `notify` + `stream_topics` fields. The connection state machine (`stream/watcher.rs::ConnState`) is socket-free so it unit-tests without a WS.
- **Client: a generic `useResourceStream<T>(topic)`** over `api/ws-core.ts` (carved from `openSessionStream`: `ws(s)://location.host` url, exp-backoff 1s→30s, `userClosed` guard, handler map) + `api/stream.ts` (one refcounted socket per tab, topic multiplexing). PTY's `usePtySession` is **left byte-identical this wave** (it carries the battle-tested AUQ path; rebasing it on the generic core is optional future cleanup). The whole "new resource" recipe is demonstrated by `useSprintTelemetry(sprintId)` (~10 lines) + the `SprintQuiescence` wire type, both owned here in `api/execution.ts` (Wave-2 imports them — no duplication).

**Waves 2–4 — consumers (read-only, no new domain/writes).**
- **Sprint reads (flat composition, no new SQL aggregate)**: `repo::list_sprints(status?)` + `repo::list_sprint_member_task_ids(sprint_id)` (the only genuinely new reads; `get_sprint`/`get_worktree` already exist). `GET /api/sprints` (list, `?status=`) and `GET /api/sprints/{id}` → `SprintDetailResponse { sprint, worktree: Option<Worktree>, member_task_ids, predecessor_sprint_id }`. Live counts come from the Wave-1 stream, NOT this endpoint (it stays a static snapshot). **Add `Serialize` to `SprintRecord`** (currently `Debug, Clone` only).
- **Sprint cards `[04 SPRINTS]`**: `useSprints` (list + `selectedSprintId` + `selectedDetail` — the cross-wave seam) feeds `SprintsPanel`/`SprintCard`; each card shows status + live aggregates (`useSprintTelemetry`) + a minimal worktree chip.
- **Agent stream `[05]` (Fork B — reuse the existing per-session PTY WS, no new socket)**: add a `?sprint_id=` arm to `list_pty_sessions` (server-side filter beats over-fetching all sessions; the migration-0015 correlation is best-effort — a UX caveat, not a reason to filter client-side); `useSprintAgentStream` follows the selected sprint's sessions, renders one-line summary items, subscribes `openSessionStream(latest)` for live frames, and click→`Modal`+`PtyMessage` for full content.
- **Worktrees central tab + sprint filter**: widen the `view` toggle (`'focus'|'tree'|'pty'` + `'worktrees'`, 3 edit sites) → `WorktreesView` (full list + status chips + merge-audit columns); add a sprint-membership filter to `ChildGrid` driven by `selectedDetail.member_task_ids` (work items carry no `sprint_id`, so it's an id-set cross-filter).

**Key implementation notes**: the `Worktree`/`SprintDetail` nullable fields use `skip_serializing_if = "Option::is_none"` (key OMITTED, not `null`) ⇒ the SPA zod schemas must use `.nullish()` (optional + nullable), not `.nullable()`. The `sprint-quiescence:<id>` topic key is the canonical form (Wave-1 owns it).

## Verification Commands

```
build: cargo build --workspace --manifest-path lumina/Cargo.toml && (cd lumina/web && bun run build)
test:  cargo nextest run --manifest-path lumina/Cargo.toml --profile ci && (cd lumina/web && bun test)
lint:  cargo clippy --workspace --manifest-path lumina/Cargo.toml --all-targets && (cd lumina/web && bun run type-check)
smoke: cargo tree --manifest-path lumina/Cargo.toml -p lumina-server -e normal | rg -i '\b(git2|gix)'   # control-plane purity unchanged (must be empty)
       rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src                            # macro-eradication gate (must be 0)
```

> Build discipline: sub-agents run `cargo clippy --manifest-path lumina/Cargo.toml` / `bun run type-check` + their task's narrow `cargo nextest … --profile quick -E '<filter>'` / `bun test <file>` only; the full build/test/lint/smoke pass belongs to the orchestrator's per-wave checkpoint + final verification tiers.

## Tasks

> 24 tasks across 5 waves. Within a wave, listed tasks touch disjoint files (parallel-safe); cross-wave `App.vue` edits (T16/T20/T21) are sequenced by wave order. Effort: S <30 min/1–2 files, M 30–120 min/2–5 files, L >120 min.

### Wave 1 — Reusable WS data-refresh foundation (fable-designed)

#### 1. Create the process-wide notify bus [S]
- **Files**: `lumina/core/src/notify.rs` (new), `lumina/core/src/lib.rs` (mod decl)
- **Depends on**: —
- **Action**: `ChangeNotification { aggregate_type, aggregate_id, event_type }`; `NotifyBus { tx: broadcast::Sender<…> }` with `subscribe()` + best-effort `publish()` (ignore `SendError`); `bus() -> &'static NotifyBus` via `OnceLock` (cap 1024).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml -p lumina-core --profile quick -E 'test(notify)'` — publish-with-no-receivers is Ok; subscribe→publish→recv round-trips.

#### 2. Buffer notifications on the tx; flush post-commit [M]
- **Files**: `lumina/core/src/db/client.rs` (provided `note_change` default no-op), `lumina/core/src/db/erased.rs` (`NotifyingTx` wrapping both `begin` arms), `lumina/core/src/repo/events.rs` (one `note_change` call after the INSERT), `lumina/core/tests/notify_bus.rs` (new)
- **Depends on**: 1
- **Action**: `note_change` pushes onto a `Vec` on the tx wrapper; `commit()` runs `inner.commit().await?` then publishes each buffered notification (sync, non-awaiting). `record_inert_event` delegates to `record_event` ⇒ sprint/run/worktree events covered free. Object-safety preserved.
- **Detail**: NEVER publish inside the tx (WAL isolation ⇒ pre-commit state). Raw `db::begin_write(&SqlitePool)` paths stay silent (correct — PTY has its own broadcast). Under plain `cargo test` (not nextest) tests must filter received notifications by their own aggregate ids — document in the test.
- **Acceptance**: new `notify_bus.rs`: subscribe → `repo::create_sprint(&pool,…)` → `timeout(1s, rx.recv())` yields `{aggregate_type:"sprint"}` AFTER commit; negative: begin tx + `record_event` + drop-without-commit → `try_recv()` empty. Plus full `cargo nextest run --manifest-path lumina/Cargo.toml -p lumina-core --profile ci` green (proves zero tx-semantics drift).

#### 3. Extract the shared Origin allowlist [S]
- **Files**: `lumina/server/src/http/ws_common.rs` (new — move `is_origin_allowed` + its unit test), `lumina/server/src/http/pty_sessions/ws.rs` (import; delete local fn), `lumina/server/src/http/mod.rs` (mod decl)
- **Depends on**: —
- **Action**: Lift the PTY origin-allowlist helper into a shared module both ws handlers import; behaviour byte-identical.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml -p lumina-server --profile quick -E 'test(origin_allowlist)'`; `cargo clippy --workspace --manifest-path lumina/Cargo.toml -- -D warnings`.

#### 4. Build the topic seam + connection state machine [M]
- **Files**: `lumina/server/src/stream/mod.rs` (new — `TopicResolver`, `TopicRegistry`, frame enums), `lumina/server/src/stream/watcher.rs` (new — `ConnState`), `lumina/server/src/lib.rs` (mod decl)
- **Depends on**: 1
- **Action**: `TopicResolver { prefix(); interested(param, n) -> bool /*may over-approximate*/; async resolve(pool, param) -> Result<Value, AppError> }`; `TopicRegistry { with_default_topics(); register(); parse(topic) -> Option<(resolver, param)> }`. `ConnState { subs, dirty }` with `handle_subscribe`/`handle_unsubscribe`/`note`/`mark_all_dirty`/`drain` (recompute dirty, dedupe-on-equal). Socket-free so unit-testable.
- **Acceptance**: in-module unit tests over an in-memory `AnyPool`: topic parse (`"x:y"`→resolver+param, unknown/colonless→None); subscribe→`init`; `note`→`drain` pushes once; unchanged recompute ⇒ NO frame; `mark_all_dirty` recomputes every sub. `cargo nextest … -p lumina-server --profile quick -E 'test(stream)'`.

#### 5. Mount `GET /api/stream` + wire AppState [M]
- **Files**: `lumina/server/src/http/stream.rs` (new — upgrade handler, two tasks, `CancellationToken`, `Skipped`-on-lag), `lumina/server/src/http/mod.rs` (mod + `.merge(stream::router())`), `lumina/server/src/app.rs` (`notify` + `stream_topics` AppState fields, defaulted)
- **Depends on**: 2, 3, 4
- **Action**: WS handler mirrors `pty_sessions/ws.rs`: origin allowlist (via `ws_common`) before upgrade, split socket, `select!` over {ws frames, `state.notify.subscribe()`, 150 ms coalesce sleep}; on `Lagged(n)` → `mark_all_dirty()` + emit `skipped`.
- **Acceptance**: `oneshot` test: disallowed/absent Origin → same `AppError::Validation` envelope as PTY ws (pre-upgrade); `app.rs` health test still green (defaults didn't break construction); clippy clean.

#### 6. First consumer: sprint-quiescence resolver + e2e proof [M]
- **Files**: `lumina/server/src/stream/topics/mod.rs` (new — registration point), `lumina/server/src/stream/topics/sprint_quiescence.rs` (new), `lumina/server/src/stream/mod.rs` (`with_default_topics` registers it), `lumina/server/tests/stream_e2e.rs` (new)
- **Depends on**: 5
- **Action**: `SprintQuiescenceTopic` (prefix `sprint-quiescence`; `interested` true for any `work_item|sprint|batch|worktree` event; `resolve` calls `repo::get_sprint_quiescence`).
- **Acceptance**: integration test (tokio-tungstenite, already a dev-dep per `companion_e2e`): bind ephemeral listener over `build_router`, connect with `Origin: http://127.0.0.1`, subscribe `sprint-quiescence:{id}` → `init` (zeros) → drive `create_sprint`+`create_work_item`+`add_tasks_to_sprint`+`set_sprint_status(active)` → `data` frame with `claimable==1` within timeout. `cargo nextest … -p lumina-server --profile quick -E 'test(stream_e2e)'`.

#### 7. Client ws-core + multiplexed stream opener [M]
- **Files**: `lumina/web/src/api/ws-core.ts` (new), `lumina/web/src/api/stream.ts` (new), `lumina/web/src/__tests__/stream-api.test.ts` (new)
- **Depends on**: — (parallel with 1–6)
- **Action**: `openReconnectingSocket({path, onFrame, onOpen, onDown})` (exp backoff 1s→30s, reset-on-open, `userClosed` guard); `openResourceStream()` → `{ subscribe(topic, onFrame) -> unsubscribe, onStatus, close }` (one socket, `Map<topic, handler[]>`; first handler ⇒ send subscribe, last gone ⇒ unsubscribe; `onOpen` ⇒ resubscribe every live topic; zod-validate frames).
- **Acceptance**: `bun test src/__tests__/stream-api.test.ts` (mock `globalThis.WebSocket`): subscribe sends frame; reconnect re-sends every live subscription; `close()` suppresses retry; bad frames dropped by zod. `bun run type-check`.

#### 8. `useResourceStream<T>` composable [S]
- **Files**: `lumina/web/src/composables/useResourceStream.ts` (new), `lumina/web/src/__tests__/resource-stream.test.ts` (new)
- **Depends on**: 7
- **Action**: `useResourceStream<T>(topic: MaybeRefOrGetter<string|null>) -> { data, status, error, connect, disconnect }`, module-singleton convention (one refcounted shared stream), swappable `__setApiForTests`/`__resetForTests`, auto-disconnect on scope dispose.
- **Acceptance**: `bun test src/__tests__/resource-stream.test.ts`: `init`/`data` update `data`; topic-ref change unsub/resub; two consumers share one server subscription; `__resetForTests` clears.

#### 9. `useSprintTelemetry` wrapper + wire type [S]
- **Files**: `lumina/web/src/api/execution.ts` (new — `SprintQuiescence` type + schema, `sprintQuiescenceTopic(id) -> "sprint-quiescence:"+id`), `lumina/web/src/composables/useSprintTelemetry.ts` (new), `lumina/web/src/api/index.ts` (barrel: `export * from './execution'`), `lumina/web/src/__tests__/sprint-telemetry.test.ts` (new)
- **Depends on**: 8
- **Action**: `useSprintTelemetry(sprintId) -> { quiescence, status, error, connect, disconnect }` delegating to `useResourceStream<SprintQuiescence>(sprintQuiescenceTopic(id))`. **Owns the `SprintQuiescence` wire type for the whole SPA** (Wave 2 imports it).
- **Acceptance**: `bun test src/__tests__/sprint-telemetry.test.ts`: mocked stream pushes a snapshot → `quiescence.value.claimable` updates. `bun run type-check`.

#### 10. Document the stream surface [S]
- **Files**: `lumina/CLAUDE.md`
- **Depends on**: 6
- **Action**: Add `/api/stream` to the HTTP-routes section; add a Transactions-section note on the post-commit notify-bus (buffer-on-tx, flush-after-commit, best-effort) and the `TopicResolver` seam.
- **Acceptance**: doc-only; orchestrator review confirms the route + the notify-bus invariant are recorded.

### Wave 2a — Sprint reads + typed client (after Wave 1)

#### 11. Add `list_sprints` + `list_sprint_member_task_ids` + `Serialize` on `SprintRecord` [S]
- **Files**: `lumina/core/src/repo/runs_sprints.rs`
- **Depends on**: — (Wave 1 not required for the Rust reads, but ships in Wave 2a)
- **Action**: `list_sprints(db, status_filter: Option<SprintStatus>) -> Result<Vec<SprintRecord>, AppError>` (mirror `list_worktrees`' optional-status arm; `ORDER BY created_at DESC, id`); `list_sprint_member_task_ids(db, sprint_id) -> Result<Vec<String>, AppError>` (`SELECT task_id FROM sprint_tasks WHERE sprint_id=$1`). Add `Serialize` to `SprintRecord`'s derive. Read-only, no tx.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml -p lumina-core --profile quick -E 'test(list_sprints) + test(member_task_ids)'`; `cargo clippy -p lumina-core …`.

#### 12. Add `GET /api/sprints` + `GET /api/sprints/{id}` handlers [M]
- **Files**: `lumina/server/src/http/sprints.rs`
- **Depends on**: 11
- **Action**: `list_sprints_handler` (`Query<{status: Option<SprintStatus>}>` → `Json<Vec<SprintRecord>>`); `get_sprint_detail_handler` (`Path<String>` → compose `get_sprint` + conditional `get_worktree(worktree_id)` + `list_sprint_member_task_ids` → `SprintDetailResponse { sprint, worktree: Option<Worktree>, member_task_ids, predecessor_sprint_id }`). Register both routes. `worktree` emits `null` (do NOT `skip_serializing_if`) so the SPA's `.nullish()` schema round-trips.
- **Acceptance**: `oneshot` tests (seed via `create_sprint`+`add_tasks_to_sprint`+`create_worktree`): list returns seeded sprints + `?status=` filters; detail returns `{sprint, worktree:null, member_task_ids:[t]}` and populated `worktree` when owned. `cargo nextest … -p lumina-server --profile quick -E 'test(sprints_http)'`; clippy.

#### 13. Wire-enum additions: `Lane`, `SprintStatus`, `WorktreeOutcome` [S]
- **Files**: `lumina/web/src/api/wire-enums.ts`
- **Depends on**: —
- **Action**: Append three const-tuple + `z.enum` + derived-type blocks: `Lane (implement|review)`, `SprintStatus (draft|ready|active|review|done|cancelled)`, `WorktreeOutcome (merged|rejected)`, each doc-commented as mirroring its Rust enum.
- **Acceptance**: `bun run type-check`; schemas exercised by task 14's test.

#### 14. `api/sprints.ts` + `api/worktrees.ts` (+ barrel) [M]
- **Files**: `lumina/web/src/api/sprints.ts` (new), `lumina/web/src/api/worktrees.ts` (new), `lumina/web/src/api/index.ts`, `lumina/web/src/__tests__/sprints.test.ts` (new)
- **Depends on**: 13 (enums); shapes from 12
- **Action**: `WorktreeSchema` (mirror the `Worktree` struct; nullable fields `.nullish()` since they're skip-serialized; `effective_status: SprintStatusSchema`, `outcome: WorktreeOutcomeSchema.nullish()`); `SprintRecordSchema`; `SprintDetailSchema { sprint, worktree: WorktreeSchema.nullable(), member_task_ids: z.array(z.string()), predecessor_sprint_id: z.string().nullable() }`; `listSprints({status?})`, `getSprintDetail(id)`, `listWorktrees({status?})`, `getWorktree(id)` via `handle()`. Barrel adds both.
- **Acceptance**: `bun test src/__tests__/sprints.test.ts` (mock `fetch`, round-trip both wrappers + the three enums); `bun run type-check`.

### Wave 2b — Sprint cards (after Wave 2a + Wave 1)

#### 15. `useSprints.ts` (list + `selectedSprintId` + `selectedDetail`) [M]
- **Files**: `lumina/web/src/composables/useSprints.ts` (new), `lumina/web/src/__tests__/use-sprints.test.ts` (new)
- **Depends on**: 14
- **Action**: Module-singleton (mirror `usePtySessions.ts`): refs `sprints`, `selectedSprintId`, `selectedDetail`, `status`, `error`; `loadSprints`, `selectSprint(id)` (sets id + fetches `getSprintDetail`); swappable `__setApiForTests`/`__resetForTests`. **`selectedSprintId` + `selectedDetail.member_task_ids` are the Wave-3/4 seams.**
- **Acceptance**: `bun test src/__tests__/use-sprints.test.ts`: `loadSprints` populates, `selectSprint` sets id+detail, reset clears.

#### 16. `SprintsPanel.vue` + `SprintCard.vue`; swap `[04]` [M]
- **Files**: `lumina/web/src/components/SprintsPanel.vue` (new), `lumina/web/src/components/SprintCard.vue` (new), `lumina/web/src/App.vue`
- **Depends on**: 15, 9 (`useSprintTelemetry`)
- **Action**: `SprintCard` (props `{sprint, worktree, selected}`): title, `<StatusPill :status="sprint.status">`, live aggregates from `useSprintTelemetry(sprint.id)` (claimable/in_progress/blocked/done/stalled badges), minimal worktree chip (branch + `effective_status` pill + outcome chip). `SprintsPanel`: `useSprints()`, `onMounted(loadSprints)`, scrollable `v-for` of cards wiring `@select="selectSprint"` + `:selected`. `App.vue`: replace the `[04]` `<section>` body (`:76-84`) with `<SprintsPanel/>` under a `[04 / SPRINTS]` header.
- **Acceptance**: `bun run type-check`; `bun run build` (orchestrator pass). SFC render is out of bun-test scope per `web/CLAUDE.md`; composable logic covered by task 15.

### Wave 3 — Selected-sprint agent stream (after Wave 2)

#### 17. `?sprint_id=` arm on `list_pty_sessions` + `GET /api/pty/sessions` [M]
- **Files**: `lumina/core/src/repo/pty.rs`, `lumina/server/src/http/pty_sessions/mod.rs`
- **Depends on**: —
- **Action**: Extend `list_pty_sessions(…, sprint_id: Option<&str>)` with `AND ($n IS NULL OR sprint_id = $n)` mirroring the existing `status`/`project_id` guards; thread `sprint_id` through the handler's query struct. Update existing call sites/tests for the new arg.
- **Detail**: Server-side filter (not client-side) avoids over-fetching every session; the migration-0015 `sprint_id` correlation is best-effort (a session whose harvest missed the sprint won't appear — UX caveat, documented).
- **Acceptance**: `cargo nextest … --profile quick -E 'test(list_pty_sessions) + test(pty_sessions)'`; `cargo clippy --workspace …`.

#### 18. `api/pty.ts` `listSessions` gains `sprint_id` [S]
- **Files**: `lumina/web/src/api/pty.ts`
- **Depends on**: 17
- **Action**: Add `sprint_id?: string` to `listSessions` params + `qs.set('sprint_id', …)`. Otherwise `pty.ts` frozen.
- **Acceptance**: `bun run type-check`; `bun test src/__tests__/pty-session.test.ts` green + a new assertion the `sprint_id` param appears in the URL.

#### 19. `useSprintAgentStream.ts` composable [M]
- **Files**: `lumina/web/src/composables/useSprintAgentStream.ts` (new), `lumina/web/src/__tests__/sprint-agent-stream.test.ts` (new)
- **Depends on**: 18, 15 (`selectedSprintId`)
- **Action**: Module-singleton: `sessions`, derived `summaryItems`, `liveMessages`, `status`, `error`. `bind(sprintId)`/`loadForSprint` → `listSessions({sprint_id})`; subscribe `openSessionStream(latestSessionId)` folding `message` frames into one-line summaries (kind badge + truncated content + ts); `openTranscript(sessionId)` → `getMessages` for the modal. Swappable `Api`, `__set/__resetForTests`. Reuses `api/pty.ts` verbatim.
- **Acceptance**: `bun test src/__tests__/sprint-agent-stream.test.ts` (mock `openSessionStream` per the existing `SessionStream` fake): `loadForSprint` populates, summary truncation, transcript fetch.

#### 20. `SprintAgentStream.vue` + `PtySessionSummary.vue`; swap `[05]` [M]
- **Files**: `lumina/web/src/components/SprintAgentStream.vue` (new), `lumina/web/src/components/PtySessionSummary.vue` (new), `lumina/web/src/App.vue`
- **Depends on**: 19, 16 (App.vue sequencing)
- **Action**: `PtySessionSummary` (props `{item}`): one-line summary, emits `open`. `SprintAgentStream`: reads `selectedSprintId` from `useSprints()`, `watch(immediate)` → `bind(id)`, renders the lower-region summary list; click → `<Modal v-model:open>` with `#title` + `PtyMessage` rows from `openTranscript`. Reuse `ui/Modal.vue` + `PtyMessage.vue`. `App.vue`: replace the `[05]` `<section>` (`:85-93`) with `<SprintAgentStream/>`.
- **Acceptance**: `bun run type-check`; `bun run build`.

### Wave 4 — Worktrees central tab + central sprint filter (after Wave 2; T21 after T20)

#### 21. Widen the central view toggle to add `'worktrees'` [S]
- **Files**: `lumina/web/src/composables/useHierarchy.ts`, `lumina/web/src/components/CenterToolbar.vue`, `lumina/web/src/App.vue`
- **Depends on**: 20 (App.vue sequencing)
- **Action**: Widen the `view` union to `…|'worktrees'` (`useHierarchy.ts:38`); add `'worktrees'` to `viewModes` + widen `setView` (`CenterToolbar.vue:25,29`); add `<WorktreesView v-else-if="view === 'worktrees'"/>` to the central chain (`App.vue:60-66`).
- **Acceptance**: `bun run type-check`; `bun test src/__tests__/tab-state.test.ts` (extend to assert `'worktrees'` is a member).

#### 22. `useWorktrees.ts` composable [S]
- **Files**: `lumina/web/src/composables/useWorktrees.ts` (new), `lumina/web/src/__tests__/use-worktrees.test.ts` (new)
- **Depends on**: 14 (`api/worktrees.ts`)
- **Action**: Module-singleton: `worktrees`, `statusFilter: Ref<SprintStatus|'ALL'>`, `filtered` computed, `status`, `error`, `loadWorktrees({status?})`; swappable `__set/__resetForTests`.
- **Acceptance**: `bun test src/__tests__/use-worktrees.test.ts`; `bun run type-check`.

#### 23. `WorktreesView.vue` (full list + status chips + merge-audit columns) [M]
- **Files**: `lumina/web/src/components/WorktreesView.vue` (new)
- **Depends on**: 22, 21
- **Action**: `useWorktrees()`, `onMounted(loadWorktrees)`; merge-audit table: branch / `effective_status` (StatusPill) / outcome chip / `merged_at` / `merge_ref` / path; status-filter chips (reuse the `ChildGrid` chip pattern over `SprintStatus` + `ALL`).
- **Acceptance**: `bun run type-check`; `bun run build`.

#### 24. Central sprint-membership filter on `ChildGrid.vue` [M]
- **Files**: `lumina/web/src/components/ChildGrid.vue`, `lumina/web/src/__tests__/child-grid-filter.test.ts` (new)
- **Depends on**: 15 (`useSprints` `member_task_ids`)
- **Action**: Add a sprint filter beside the existing status filter: read `selectedSprintId` + `selectedDetail.member_task_ids` from `useSprints()`; local `sprintFilterOn` + a "SPRINT" chip (enabled only when a sprint is selected); extend the `filtered` computed (`:17-20`) to AND `member_task_ids.includes(c.id)` when on; "N hidden by sprint filter" affordance. Extract the cross-filter to a pure exported fn to keep it bun-testable.
- **Acceptance**: `bun test src/__tests__/child-grid-filter.test.ts` (pure cross-filter logic); `bun run type-check`.

## Dependency Graph

```
Wave 1 (foundation):
  Rust lane:  1 → 2 → 5 → 6 → 10        3 ─┘ (3 feeds 5)        4 → 5
  TS lane:    7 → 8 → 9                  (7..9 parallel to 1..6)
Wave 2a:  11 → 12        13 → 14         (Rust lane ∥ TS lane)
Wave 2b:  15 → 16        (16 also needs 9)
Wave 3:   17 → 18 → 19 → 20              (20 needs 16 for App.vue order)
Wave 4:   22 → 23 ;  21 (after 20) → 23 ;  24 (needs 15)
```

Wave boundaries are checkpoint commits. `App.vue` is edited only in 16 (Wave 2b), 20 (Wave 3), 21 (Wave 4) — never twice in one wave.

## Verification

Orchestrator-owned, two tiers (sub-agents run only their narrow clippy/type-check + scoped test):
1. **Per-wave checkpoint** (before each wave's commit): `cargo build --workspace --manifest-path lumina/Cargo.toml` + `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` for waves touching Rust; `cd lumina/web && bun run type-check && bun test` for waves touching the SPA — every commit non-broken and bisectable.
2. **Final full pass**: the `## Verification Commands` build + test + lint + the two smoke gates (control-plane purity `git2|gix` empty; macro-eradication count 0) + `cargo audit --file lumina/Cargo.lock` (no new Rust deps expected; `tokio-tungstenite` is already a dev-dep).
3. **Manual smoke**: `cargo run --manifest-path lumina/Cargo.toml -p lumina-server --bin lumina -- --with-companion`, open the SPA, compose a sprint, and confirm the card's aggregates update live over `/api/stream` as `claim`/`complete` fire (this is the dogfood-observability acceptance the whole slice exists for).

## Risks

- **Core-path touch (Wave 1, T2)**: `NotifyingTx` wraps every `DbClient::begin`. Contained — publish is sync, non-awaiting, post-commit-only, errors ignored; rollback drops the buffer; raw `begin_write` paths untouched. Any regression fails ~the whole existing core suite loudly (T2 acceptance includes the full `-p lumina-core` run).
- **Recompute chattiness**: `interested()` over-approximates, so any `work_item|sprint|batch|worktree` write dirties a quiescence topic. Mitigated by the 150 ms coalesce window + a cheap aggregate query + push-only-if-changed. Deferred scaling lever (no consumer change): a shared per-topic hub recomputing once and fanning out to N connections.
- **Reconnect/initial-state race**: structurally solved — init-on-subscribe + resubscribe-all-on-reconnect + full-snapshot (never delta) frames ⇒ no missed-delta bug class.
- **PTY↔sprint correlation is best-effort** (migration 0015, no FK): a session whose `sprint_id` harvest missed won't appear in its sprint's agent stream. Acceptable UX caveat; documented in T17.
- **Scope ~22 source files / 5 waves** exceeds the ~15 guideline — deliberate (User Decision 1's reusable-architecture investment). Contained by strict wave boundaries, disjoint files within a wave, and the per-wave `/implement` recommendation. Waves 1+2 are the MVP; 3+4 are independently deferrable.
- **Global `OnceLock<NotifyBus>` under plain `cargo test`**: cross-test notification chatter possible (not under nextest, the canonical runner); tests assert on their own aggregate ids (documented in T2).
- **Auth posture**: `/api/stream` is loopback-only + origin-allowlisted (browser-CSRF defence), strictly read-only (subscribe-only inbound) — a smaller surface than the existing PTY ws. Unchanged by `HOST=0.0.0.0` exposure caveats that already apply to all of `/api`.
