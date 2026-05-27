# Plan: Lumina PTY follow-ups — UI spawn + status + message persistence

**Plan path**: `docs/plans/lumina-pty-followups.md`
**Created**: 2026-05-27
**Status**: Draft

## Context

The PTY supervisor (flow `lumina-pty-service`, just shipped) is implemented end-to-end but cannot be hands-on tested in the UI. The PTY view shows status `closed` and the chat input is disabled because no UI affordance exists to spawn or select a session. Even if a session were spawned via curl/MCP, the supervisor would never dispatch queued inputs because the production code never writes the `Spawning → Idle` status transition. And even if dispatch worked, assistant output would be broadcast live but never persisted, so a reload would show only the user's prompts.

This plan addresses three coupled blockers documented in the `lumina-pty-service` execution-record:

- **E20 (production gap)** — Parser-bridge never flips `Session.status` from `Spawning` to `Idle` on first-prompt detection. The supervisor's tick loop only dispatches queue entries when `status == Idle`; consequently, queued inputs sit forever.
- **E21 (production gap)** — Parser-bridge broadcasts `TypedMessage` events but never persists them to `pty_messages`. Only the supervisor's user-input dispatch path writes rows. Live WebSocket subscribers see assistant output; nothing survives reload.
- **T15-deferral (UI gap)** — `PtyConsole.vue` consumes `usePtySession()` whose `currentId` is `null` until something calls `select(id)`. Nothing does. The component renders an inert empty-state with all controls disabled.

Outcome: a user can switch to the PTY view, spawn a fresh `claude` session against a cwd, see assistant output stream in, and see the full conversation persist across reload.

## Scope

- **In scope**
  - Session struct gains a per-session message sequence counter; supervisor's user_input persistence migrates onto it to eliminate the deterministic sequence collision (see Approach §Session sequence counter).
  - A new `pty/spawn.rs` module factors the duplicated HTTP + MCP spawn pipeline into one helper that wires both gaps (status transition + message persistence) at the point where the broadcast bridge is constructed. Cwd resolve+validate is extracted to a shared free function in the same module.
  - HTTP `POST /api/pty/sessions` and MCP `spawn_pty_session` refactor to delegate to the helper.
  - `PtyConsole.vue` gains an active empty-state with a session list + "+ new session" form (cwd input, spawn button); on spawn success, auto-`select(id)`.
  - `pty_e2e.rs` removes its `set_status(Idle)` workaround now that production code writes the transition, and adds an assertion that at least one `assistant_text` row lands in `pty_messages`.

- **Out of scope**
  - PATCH endpoint for session label/project_id (E14 still 501; separate follow-up).
  - Resize-frame plumbing to the PTY (parsed but ignored in v1).
  - Sequence allocation race on `POST /input` (documented v1 behaviour; separate follow-up).
  - Left-spine session picker (PTY console renders its own session list inline).

- **Affected areas**
  - `lumina/src/pty/` — new `spawn.rs`, edit `mod.rs`, edit `session.rs`, edit `supervisor.rs`, edit `protocol.rs` (promote `MessageKind::as_wire` to `pub fn`).
  - `lumina/src/http/pty_sessions.rs` — refactor spawn handler.
  - `lumina/src/mcp.rs` — refactor `spawn_pty_session` tool method.
  - `lumina/web/src/components/PtyConsole.vue` — empty-state.
  - `lumina/tests/pty_e2e.rs` — drop workaround + rewrite assertions.

- **Estimated file count**: 9 unique files (8 modified, 1 new). All batches stay under the 6-file cap.

## Approach

### Session sequence counter

Add `pub sequence_counter: AtomicI64` to `Session` (initialised to `AtomicI64::new(1)`) plus method `pub fn next_sequence(&self) -> i64 { self.sequence_counter.fetch_add(1, Ordering::Relaxed) }`. Relaxed ordering is sufficient — `fetch_add` is atomic and produces a monotone sequence regardless of `Ordering`; no other atomic state on `Session` is read together with this counter. Pin this invariant in a doc-comment on the method so future contributors don't "upgrade" to `SeqCst` without a measurable need.

**Critical: bridge counter and supervisor's persistence allocator currently collide on the first prompt.** The supervisor's `dispatch_one` (`pty/supervisor.rs:249-253`) persists user_input rows via `repo::pty::insert_pty_message(..., entry.sequence, ...)` — i.e. the **`pty_queue` row's** sequence, which was allocated at enqueue time as `Queue::list().len() + 1` in the HTTP / MCP / WS-receive code paths. Both `entry.sequence` (first queue row → 1) and the new `Session::next_sequence()` (first call → 1) start at 1, so the very first `pty_messages` INSERT pair deterministically trips `UNIQUE(session_id, sequence)` (migration 0008). The supervisor's INSERT logs `eprintln!` and the user_input audit row is lost — guaranteed to break T6 on the e2e happy path.

**Mitigation (in scope):** migrate the supervisor's user_input persistence to call `session.next_sequence()` for the **message-row** sequence (the `pty_queue` row keeps its own sequence at enqueue time, used only for queue ordering). One-line change in `pty/supervisor.rs::dispatch_one`. Queue-row sequence and message-row sequence become distinct namespaces conceptually — solves the collision without restructuring either the queue table or the supervisor's dispatch path. The supervisor's `Queue::list().len() + 1` race on the *enqueue* side (a separate `pty_queue.UNIQUE` concern) remains out of scope; same documented v1 caveat as before.

### Spawn helper module

Create `lumina/src/pty/spawn.rs` exposing one function:

```rust
pub async fn spawn_pty_session_internal(
    state: &AppState,
    config: SpawnConfig,            // caller-built, `config.cwd` already canonicalised
    label: Option<String>,
    project_id: Option<String>,
    cwd_display: String,            // canonical_cwd.to_string_lossy().into_owned() —
                                    // caller's pre-validated form; helper does not re-canonicalise
) -> Result<PtySession, AppError>
```

The helper performs the 6-step pipeline currently duplicated in `http/pty_sessions.rs:179–296` and `mcp.rs:2546–2651`:

1. `state.pty_transport.spawn(config)` → `TransportHandle`.
2. `repo::pty::create_pty_session(...)` → DB row.
3. Build `Session::new(id, broadcast_tx, input_tx)` and insert into `state.pty_registry` — **clone the `Arc<Session>` before insert so the bridge task can retain its own handle**.
4. Spawn the broadcast-bridge tokio task (described next).
5. If `state.pty_register_tx.is_some()`, send `SessionRegistration { session_id, completed: handle.completed }` to the supervisor; else `eprintln!` and explicitly `drop(handle.completed); drop(handle.shutdown);` — `TransportHandle`'s `Drop` impl does not chain these, and the existing HTTP/MCP handlers do this deliberately to release the child-wait worker and unblock the cancel task.
6. Return the persisted `PtySession` row.

**Pre-`tokio::spawn` setup for the bridge task** (the pseudocode below elides these for brevity but they are mandatory — the task outlives the helper's stack frame):

```rust
let pool = state.pool.clone();
let session = session.clone();              // Arc<Session> clone — keeps next_sequence()/set_status() reachable
let session_id_str = session_id.to_string();
let bridge_tx = broadcast_tx.clone();
let mut transport_rx = handle.outbound;
let mut idle_flipped = false;
```

The broadcast-bridge task replaces the existing per-message forward with three actions per message:

```rust
loop {
    match transport_rx.recv().await {
        Ok(msg) => {
            // Action 1: forward to registry-side broadcast (existing behaviour)
            let _ = bridge_tx.send(msg.clone());

            // Action 2: assign sequence, persist to pty_messages
            let seq = session.next_sequence();
            let msg_id = uuid::Uuid::now_v7().to_string();
            let kind_wire = msg.kind.as_wire();  // &'static str — promote MessageKind::as_wire to `pub fn` in protocol.rs
            let content_json = serde_json::to_string(&msg.content)
                .unwrap_or_else(|_| "{}".to_string());
            let raw_text = msg.raw_text.as_deref();
            if let Err(e) = repo::pty::insert_pty_message(
                &pool, &msg_id, &session_id_str, seq,
                &kind_wire, &content_json, raw_text,
            ).await {
                eprintln!("pty bridge: insert_pty_message failed for {session_id_str}: {e}");
            }

            // Action 3: on first Prompt, flip status Spawning -> Idle
            if !idle_flipped && matches!(msg.kind, MessageKind::Prompt) {
                idle_flipped = true;
                session.set_status(SessionStatus::Idle).await;
                if let Err(e) = repo::pty::update_pty_session_status(
                    &pool, &session_id_str, "idle", None,
                ).await {
                    eprintln!("pty bridge: status -> idle persist failed for {session_id_str}: {e}");
                }
            }
        }
        Err(broadcast::error::RecvError::Lagged(_)) => continue,
        Err(broadcast::error::RecvError::Closed) => break,
    }
}
```

All errors are logged via `eprintln!` rather than failing the task — consistent with the supervisor's per-session error-swallowing policy. The bridge task owns clones of `Arc<SqlitePool>`, the session-id string, and the `Arc<Session>`.

### Spawn-handler refactors

Both `http/pty_sessions.rs::spawn_session` and `mcp.rs::spawn_pty_session` keep their entry-point shell (extractors and error mapping). **Cwd resolve+validate is extracted to a shared free function in `pty/spawn.rs`** — `pub fn resolve_and_validate_cwd(raw: &Path) -> Result<PathBuf, AppError>` — so both shells share one definition of "under worktree root" (today's two copies are byte-equivalent semantically; keeping them in sync as the project grows is a non-zero drift risk). Both handlers then delegate the 6-step pipeline to `spawn_pty_session_internal`. The handlers shrink from ~120 lines each to ~30 lines each, and the bug-fix burden for any future PTY-spawn change moves to one location.

### Frontend empty-state

`PtyConsole.vue` gains a `v-if="currentId === null"` block above the existing header/list/input that renders:

- A `<section>` titled "Select a session" listing each `usePtySessions().sessions` row with a click handler `() => select(s.id)`.
- A `<section>` titled "Spawn new session" with:
  - A single `<input type="text" v-model="newCwd" placeholder="cwd (default: worktree root)">` field.
  - A "Spawn" button calling an inline async handler that:
    1. Calls `usePtySessions().spawn({ cwd: newCwd.value || '.', claude_args: [], agent_json: null, model: null, env_passthrough_otel: false, label: null, project_id: null, settings_json: null, prompt_pattern: null })`.
    2. On success (truthy return), calls `select(result.id)`.
    3. On null return (spawn errored), surfaces `usePtySessions().error` via the existing error display.

**Destructure widening required**: the current `PtyConsole.vue:47-52` destructure is `const { currentId, messages, status: wsStatus, submit } = usePtySession()` — `select` is NOT imported. Widen to `const { currentId, messages, status: wsStatus, submit, select, error: focusError } = usePtySession()` so the empty-state click handlers and the active branch's error rendering both work. `focusError` is the focused-session error ref (WS/history failures); the v-else (active) branch MUST render it so post-spawn errors aren't silently swallowed after the empty-state hides — `usePtySessions().error` only covers catalogue/spawn failures.

The existing header / message list / input box stay in a `v-else` branch — the rest of the component is unchanged. `onMounted(() => void loadSessions())` is already in place (line 183).

### Integration-test update

`pty_e2e.rs` currently calls `session.set_status(SessionStatus::Idle).await` directly to work around E20. With the bridge task wired, the test:

1. Removes the workaround entirely.
2. Continues polling `/messages` after submitting the prompt until at least one row with `kind == "assistant_text"` appears (the stub binary emits `Assistant: echo: <line>` lines that the parser categorises as AssistantText).
3. **Rewrites the existing index-0 user_input assertion** (currently `pty_e2e.rs:313-317`: `let first = &messages[0]; assert_eq!(first["kind"].as_str(), Some("user_input"))`) to `assert!(messages.iter().any(|m| m["kind"].as_str() == Some("user_input")))` — the bridge now persists assistant_text/system/prompt rows that arrive BEFORE the dispatched user_input (the stub binary emits `"Lumina PTY stub ready."` + `> ` before any input), so the user_input row is no longer guaranteed at index 0.
4. Tightens the timeout if necessary (the stub responds within ~100ms).

### Reused patterns

- Error-swallowing in background tasks: matches the supervisor's `eprintln!` policy (`lumina/src/pty/supervisor.rs`).
- `Arc<AtomicI64>` for per-session counters: idiomatic; no existing precedent in lumina but standard tokio practice.
- `uuid::Uuid::now_v7()` for new row IDs: matches `repo::pty::create_pty_session`.

### Important constraints

- **Sequence-counter relaxed ordering** — single-allocator-per-session, so `Relaxed` is safe. Do not promote to `SeqCst` without a measurable need.
- **Persistence errors are logged, not propagated** — the bridge task MUST NOT terminate on `insert_pty_message` failure (one bad row should not silence the whole session). The supervisor's policy applies here.
- **Idle-flip fires exactly once per session** — guard with a local `bool idle_flipped` rather than reading `session.status()` (avoids a race with concurrent set_status calls).

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets --no-deps
```

Additional gates:

```
sqlx:    cargo sqlx prepare --check (run inside lumina/)
web:     cd lumina/web && bun test
manual:  cargo run --manifest-path lumina/Cargo.toml then open http://127.0.0.1:24817, switch to PTY view, spawn a session, send a prompt, see assistant output, reload, confirm history persists
```

Note: `cd lumina/web && npm run build` is expected to fail on the pre-existing `RepoLinksPanel.vue` TypeScript break — not caused by this plan; use `bun test` + `npx vue-tsc --noEmit` for web verification.

## Tasks

### Phase 1: Foundations (parallel — 2 tasks)

#### 1. Add `sequence_counter` + migrate supervisor user_input persistence [S]
- **Files**: `lumina/src/pty/session.rs`, `lumina/src/pty/supervisor.rs`, `lumina/src/pty/protocol.rs`
- **Depends on**: —
- **Action**: (a) Add `pub sequence_counter: AtomicI64` field to `Session` (initialised in `new()` to `AtomicI64::new(1)`). Add method `pub fn next_sequence(&self) -> i64` returning `self.sequence_counter.fetch_add(1, Ordering::Relaxed)`. Add `use std::sync::atomic::{AtomicI64, Ordering};` at the top. Pin the Relaxed-ordering invariant in a doc-comment on the method. (b) In `pty/supervisor.rs::dispatch_one` (around line 249-253), change the `insert_pty_message` call to use `session.next_sequence()` for the message-row sequence instead of `entry.sequence`. The `pty_queue` row's sequence (used for queue ordering) stays as `entry.sequence`. (c) Promote `MessageKind::as_wire` in `pty/protocol.rs:122` from `fn` to `pub fn` so the bridge task (T3) can use it without a per-message `to_string()` allocation.
- **Detail**: Field is on the struct, not behind a Mutex — atomic ops are lock-free. Tests in `pty/registry.rs` and `pty/queue.rs` that construct `Session::new(...)` need no changes (no field addition to the constructor signature; counter is initialised internally). The supervisor migration fixes a deterministic collision between the bridge's `next_sequence()` and `entry.sequence` on `pty_messages.UNIQUE(session_id, sequence)` — see Approach §Session sequence counter.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` passes; `cargo nextest run --manifest-path lumina/Cargo.toml --lib pty::` passes (existing 13 tests still green); `cargo clippy --manifest-path lumina/Cargo.toml --all-targets --no-deps` clean.

#### 2. PtyConsole spawn affordance [M]
- **Files**: `lumina/web/src/components/PtyConsole.vue`
- **Depends on**: —
- **Action**: Widen the existing `usePtySession()` destructure on `PtyConsole.vue:47-52` to add `select` and `error: focusError` (current destructure is `{ currentId, messages, status: wsStatus, submit }` — `select` is missing). Add a `v-if="currentId === null"` empty-state branch above the existing header. Render two sections: "Select a session" (v-for over `usePtySessions().sessions` with click handler → `select(s.id)`; show id, label, status, started_at) and "Spawn new session" (cwd input + Spawn button). The Spawn button calls `usePtySessions().spawn({...})` with default config (claude_args: [], env_passthrough_otel: false, all optionals null), then on success calls `select(result.id)`. Surface `usePtySessions().error` near the form (catches spawn failures). Wrap the existing header + list + input in a `v-else` branch; surface `focusError` (the renamed `usePtySession().error`) in that branch so post-spawn WS/history errors aren't silently swallowed.
- **Detail**: Defaults: cwd = the input value (trimmed) or `'.'` (current working dir of lumina) when empty. Tailwind classes consistent with the rest of the component (`var(--surface)`, `var(--border)`, `var(--accent)` tokens). Session list renders as clickable rows with hover state; no separate "open" button needed.
- **Acceptance**: `npx vue-tsc --noEmit -p tsconfig.app.json | grep PtyConsole | wc -l` returns 0. `bun test` does not regress. Manual: switch to PTY view, see the empty-state. (Real spawn won't work until T4+T5 land — for this task the spawn call merely returns a row; the assistant-output flow requires the helper.)

### Phase 2: Helper module (after T1)

#### 3. Create `pty/spawn.rs` helper with persistence + Idle wiring [M]
- **Files**: `lumina/src/pty/spawn.rs` (new), `lumina/src/pty/mod.rs`
- **Depends on**: T1
- **Action**: Implement `pub async fn spawn_pty_session_internal(state: &AppState, config: SpawnConfig, label: Option<String>, project_id: Option<String>, cwd_display: String) -> Result<PtySession, AppError>` per the Approach section. The function performs the 6-step pipeline and spawns the enhanced broadcast-bridge task that (a) forwards messages to the registry-side broadcast, (b) assigns `session.next_sequence()` and persists each message via `repo::pty::insert_pty_message`, (c) on the first `MessageKind::Prompt` flips `session.set_status(SessionStatus::Idle).await` and persists via `repo::pty::update_pty_session_status`. Add `pub mod spawn;` to `pty/mod.rs` (do not re-export — keep the helper module-qualified to mark it as internal infrastructure).
- **Detail**: Use `eprintln!` for all error-logging in the bridge task — never propagate. The `idle_flipped: bool` local guard ensures the status flip fires exactly once. `kind_wire` comes from `msg.kind.to_string()` (Display impl renders snake_case). `content_json` is `serde_json::to_string(&msg.content).unwrap_or_else(|_| "{}".to_string())` — the unwrap fallback is fine because `msg.content` is itself a `serde_json::Value` and serialising a Value cannot fail except OOM. The task owns clones of `Arc<SqlitePool>`, `Arc<Session>`, and a String session id.
- **Acceptance**: `cargo build` + `cargo clippy --all-targets --no-deps` clean. `cargo nextest run --lib pty::` still passes 13/13 (no inline tests added in this task; the bridge task's behaviour is exercised end-to-end by T6).

### Phase 3: Wire helper into surfaces (parallel — 2 tasks, after T3)

#### 4. Refactor `http/pty_sessions.rs::spawn_session` to call helper [M]
- **Files**: `lumina/src/http/pty_sessions.rs`
- **Depends on**: T3
- **Action**: Replace the 6-step pipeline body (currently lines ~179–296) with a call to `crate::pty::spawn::spawn_pty_session_internal`. The handler retains: extractor parsing, cwd canonicalisation against `LUMINA_WORKTREE_ROOT` / `current_dir()`, `SpawnConfig` construction, and the `(StatusCode::CREATED, Json(row))` response shape. Strip the now-dead imports.
- **Detail**: Keep the existing 422 mapping for cwd validation; only the helper handles AppError → IntoResponse. Handler shrinks from ~120 lines to ~30 lines.
- **Acceptance**: `cargo build` + `cargo clippy --all-targets --no-deps` clean. `cargo nextest run --lib http::pty_sessions::` passes (4 existing smoke tests still green).

#### 5. Refactor `mcp.rs::spawn_pty_session` to call helper [M]
- **Files**: `lumina/src/mcp.rs`
- **Depends on**: T3
- **Action**: Replace the 6-step pipeline body (currently lines ~2546–2651) with a call to `crate::pty::spawn::spawn_pty_session_internal`. The MCP tool retains: param destructuring, cwd validation, error mapping via `app_error_to_mcp`, and the `json_result` envelope wrapping the returned `PtySession`.
- **Detail**: After this refactor, both spawn entry points are byte-equivalent in behaviour. The MCP tool method shrinks from ~110 lines to ~30 lines. Existing imports for `PtySession`/`SpawnConfig`/`SessionRegistration`/`Session`/`TransportHandle` may become unused in mcp.rs — strip them.
- **Acceptance**: `cargo build` + `cargo clippy --all-targets --no-deps` clean. `cargo nextest run --lib mcp::tests::` passes (7 existing tests still green, including the tool-count assertion at 61).

### Phase 4: E2E test (after T4)

#### 6. Drop workaround + assert assistant message persistence + fix user_input assertion [S]
- **Files**: `lumina/tests/pty_e2e.rs`
- **Depends on**: T4
- **Action**: (a) Remove the test-side `session.set_status(SessionStatus::Idle).await` workaround call (search for it; the 12-line comment block at `pty_e2e.rs:219-249` flags it as a deliberate deviation, ending in the call at line 249). (b) Extend the existing `/messages` polling loop to continue until a row with `kind == "assistant_text"` is observed (the stub binary's `Assistant: echo: <line>` output parses as AssistantText). Bound by the existing 10s timeout. (c) Rewrite the existing index-0 user_input assertion (`pty_e2e.rs:313-317`: `let first = &messages[0]; assert_eq!(first["kind"].as_str(), Some("user_input"))`) to `assert!(messages.iter().any(|m| m["kind"].as_str() == Some("user_input")))` — the bridge now persists assistant_text/system/prompt rows that arrive BEFORE the dispatched user_input.
- **Detail**: The new assistant_text assertion shape: `assert!(messages.iter().any(|m| m["kind"].as_str() == Some("assistant_text")))`. If the assistant_text row never arrives within the timeout, the test fails with a clear message naming what was missing. The user_input rewrite is required, not additive — the positional `messages[0]` no longer holds (the stub's `> ` prompt or banner line gets persisted first).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml --test pty_e2e --retries 0` passes 3 times in a row. The grep guard (`grep -E 'tokio::time::sleep|std::thread::sleep' lumina/tests/pty_e2e.rs` returns empty) still holds.

## Dependency Graph

```
Batch 1 (parallel): T1, T2
Batch 2 (after T1): T3
Batch 3 (parallel after T3): T4, T5
Batch 4 (after T4): T6
```

T2 (frontend) is technically independent of T1/T3/T4 — it ships the UI scaffold; users will see "no sessions yet" until backend lands. **However, all six tasks MUST land in a single PR** — merging T2 in isolation ships a spawn UI that produces sessions with no assistant output and broken reload history, which is worse than the current inert empty-state. Do not merge any task to main until the full set passes T6's acceptance. T1 → T3 because `Session::next_sequence` is called inside the helper. T3 → T4, T5 because both refactors call the helper. T4 → T6 because the e2e test exercises the HTTP path with the fixed bridge.

## Verification

End-to-end test plan (`/implement` Phase 3):

- **Build**: `cargo build --manifest-path lumina/Cargo.toml`
- **Backend tests**: `cargo nextest run --manifest-path lumina/Cargo.toml` (146 + new assertion in pty_e2e)
- **Lint**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets --no-deps`
- **SQLx**: `cd lumina && cargo sqlx prepare --check -- --all-targets`
- **Web tests**: `cd lumina/web && bun test` (188 still pass)
- **Web typecheck**: `cd lumina/web && npx vue-tsc --noEmit -p tsconfig.app.json` (errors only from pre-existing RepoLinksPanel.vue, which is not in scope)

Manual smoke (the goal of this plan):

- Start lumina (`cargo run --manifest-path lumina/Cargo.toml`).
- Open the SPA, switch to the PTY view — see the empty-state with the spawn form.
- Enter a cwd (or leave blank for `.`) and click Spawn — a row appears in the session list with status `spawning` → `idle`.
- A WebSocket auto-connects; status pill flips to `open`.
- Type a prompt, press Cmd/Ctrl+Enter — see the user prompt appear, then the assistant echo response.
- Reload the page — switch back to PTY view, select the session — message history is preserved including the assistant response.

## Risks

- **Broadcast lag drops persistence AND can wedge the Idle flip (MEDIUM)** — the bridge task's `Lagged(_) => continue` silently skips persistence of dropped messages (broadcast capacity 1024). If the first `MessageKind::Prompt` falls inside a lagged batch, `idle_flipped` never sets, the session sits in `Spawning` indefinitely, and the supervisor never dispatches queued input. Mitigation deferred to a future revision: either widen broadcast capacity, or add a periodic "no Idle after N ms in Spawning" watchdog in the supervisor that flips Idle defensively. At echo-binary throughput this is improbable; at real `claude` throughput with a slow disk it's plausible.
- **Status race on rapid output (LOW)** — if the parser emits a non-Prompt message before the first Prompt (e.g. stub binary's `"Lumina PTY stub ready."` initial line), the bridge task persists those messages with `status` still `Spawning`. The supervisor only dispatches on `Idle`, so any user input enqueued before the first Prompt sits in the queue until the flip. Acceptable: the e2e test exercises exactly this path and the assertion holds.
- **insert_pty_message failure cascade (LOW)** — if SQLite locks up during a high-throughput session, the bridge task could log thousands of `eprintln!` lines. Acceptable for v1; a real fix is the future tracing-subscriber introduction.
- **Sequence-counter race on enqueue side (LOW; in-scope collision fixed)** — T1 migrates the supervisor's user_input *persistence* onto `Session::next_sequence()`, eliminating the `pty_messages.UNIQUE` collision documented in Approach §Session sequence counter. The supervisor's `Queue::list().len() + 1` allocation on the *enqueue* side (assigning `pty_queue.sequence`) still races under concurrent `POST /input` — but that's a `pty_queue.UNIQUE` concern, not a `pty_messages` one. The two table-level unique constraints are now decoupled. Full enqueue-side unification is a separate follow-up.
- **PtyConsole spawn-form UX is minimal (LOW)** — single text field for cwd, default `'.'`. Real users will want claude_args, model selection, etc. v1 ships the minimum to test the loop; richer config UI is future work.
- **Lock contention on `Session::set_status`** — the bridge task takes the status Mutex during the Idle flip. If a WS handler or supervisor tick is reading status simultaneously, brief contention; not a correctness concern.
