<!-- Generated from execution-record.toml. Do not edit by hand. -->

# lumina-pty-service — Progress Log

---

## Completed Items

| # | Item | Date | Commit | Notes |
|---|------|------|--------|-------|
| E2 | add-dependencies-write-migration-0008 | 2026-05-27 | `6475e78` | portable-pty/vt100/async-trait/bytes + axum ws feature + dev-deps; migration 0008 (3 tables + 4 indexes + 2 triggers). (2 files) |
| E3 | define-pty-wire-types | 2026-05-27 | `6475e78` | Frozen pty/ module tree with 8 sub-module declarations + stubs; protocol.rs defines SessionId/SessionStatus/MessageKind/TypedMessage/InputFrame/InputKind. (10 files) |
| E5 | define-transport-trait-handle-types | 2026-05-27 | `d5468ea` | async_trait Transport trait + SpawnConfig (w/ prompt_pattern), SessionExit, TransportHandle. (2 files) |
| E6 | implement-vt100-parser-end-of-turn-heuristic | 2026-05-27 | `d5468ea` | vt100::Parser feed/check_idle + 5 inline unit tests; matches_prompt is a free function (regex intentionally not a dep). (2 files) |
| E7 | extend-repo-rs-with-pty-crud | 2026-05-27 | `d5468ea` | pub mod pty with 12 CRUD helpers + now_string helper; PtySession/PtyMessage/PtyQueueEntry to domain.rs; .sqlx regenerated --all-targets. (3 files) |
| E9 | implement-ptytransport-via-portable-pty | 2026-05-27 | `6d319ee` | PtyTransport with 6 worker tasks (reader/writer/parser-bridge/input-bridge/child-wait/cancel). clone_killer fallback (kill_child not on portable-pty 0.9). (2 files) |
| E10 | session-sessionregistry-queue | 2026-05-27 | `6d319ee` | Session container + SessionRegistry (Arc<RwLock<HashMap>>) + Queue facade; 5 new unit tests pass. (4 files) |
| E11 | supervisor-task-spawn | 2026-05-27 | `142907a` | 4-branch select (cancel/tick/registration/exit-reap); per-session error swallowing; 3 inline tests; added futures 0.3. (3 files) |
| E12 | http-family-websocket-pty-sessions | 2026-05-27 | `df2fa18` | 10 PTY HTTP routes + WS upgrade; AppState widened; AppState::new constructor; 4 smoke tests pass. (16 files inc. cross-cut migration) |
| E15 | 6-mcp-tools-for-pty | 2026-05-27 | `ffdc403` | 6 PTY MCP tools; tool count 55→61; LuminaTools widened with state: AppState + back-compat new(pool); mcp::service_with_state added. (2 files) |
| E16 | wire-supervisor-registry-into-app-rs | 2026-05-27 | `ffdc403` | Supervisor spawned in app::serve; state.pty_register_tx = Some(supervisor.register_tx()); shared registry across HTTP+MCP. (1 file) |
| E17 | pty-api-client-ws-opener | 2026-05-27 | `ffdc403` | zod schemas + 9 fetch wrappers + openSessionStream WS opener with exponential backoff reconnect. (2 files) |
| E18 | composables-useptysessions-useptysession | 2026-05-27 | `0fcc7c6` | Module-singleton composables; request-id token cancellation; onScopeDispose-guarded cleanup; 13 bun tests. (3 files) |
| E19 | integration-test-pty-e2e | 2026-05-27 | `0fcc7c6` | In-process e2e with PATH-override claude substitution + RAII tempdir guard; deterministic spawn→input→message→cancel; no sleep. (3 files) |
| E22 | ptyconsole-ptymessage-vapor-components | 2026-05-27 | `5366534` | PtyConsole + PtyMessage Vapor SFCs with all 8 message-kind renderers; Cmd/Ctrl+Enter submit; IntersectionObserver auto-scroll; null-project affordance. (2 files) |
| E23 | wire-pty-view-into-app-vue | 2026-05-27 | `5366534` | View union widened to focus\|tree\|pty; CenterToolbar auto-renders new button; PtyConsole mounted in focused + portfolio branches. (3 files) |
| E24 | run-full-verification-docs-refresh | 2026-05-27 | `88a169d` | All 6 gates green (build/nextest/clippy/sqlx-check/bun-test/cargo-audit); lumina/CLAUDE.md updated with PTY HTTP routes + migration-0008 MCP catalogue. (1 file) |

---

## Deviations

| # | Deviation | Date | Commit | Rationale | Supersedes |
|---|-----------|------|--------|-----------|------------|
| E4 | TypedMessage.created_at uses String, not jiff::Timestamp | 2026-05-27 | `6475e78` | jiff = 0.2 has no serde feature enabled, and Cargo.toml ownership was held by T1 in the parallel batch. domain.rs convention is `created_at: String` for every row struct. T8 supervisor mints via jiff::Timestamp::now().to_string() per export.rs pattern. | — |
| E8 | PtySession/PtyMessage/PtyQueueEntry omit Deserialize + sqlx::FromRow + rename-all | 2026-05-27 | `d5468ea` | Established domain.rs convention is Debug+Clone+Serialize only for read-only row structs; query_as! works without FromRow; column names are already snake_case. Dispatch said match conventions over the literal derive list. | — |
| E13 | AppState migration cross-cut to 13 files (16 sites) | 2026-05-27 | `df2fa18` | Widening AppState required converting every AppState{pool: ...} literal to AppState::new(...). Mechanical edits in lumina/src/http/ + lumina/tests/e2e.rs. No semantic change. | — |
| E14 | PATCH /api/pty/sessions/{id} returns 501 Not Implemented for v1 | 2026-05-27 | `df2fa18` | No repo::pty::update_pty_session_meta helper exists; adding one was out of T9 scope. Follow-up PR can add the helper + handler. | — |
| E20 | Production gap: Spawning → Idle transition never written by parser-bridge | 2026-05-27 | `0fcc7c6` | Neither pty_transport parser-bridge nor http spawn handler calls Session::set_status(Idle) on first-prompt detection. Test workaround calls set_status(Idle) directly. Follow-up: wire parser-bridge to push Idle transition on first Prompt-kind TypedMessage. | — |
| E21 | v1 only persists user_input messages; assistant blocks broadcast but not persisted | 2026-05-27 | `0fcc7c6` | pty_transport parser-bridge broadcasts TypedMessage but does not call insert_pty_message. Only supervisor::dispatch_one persists messages (user_input only). Follow-up: extend parser-bridge to persist assistant_text/tool_call/prompt/error blocks. | — |

---

## Deferrals

| # | Item | Deferred From | Date | Reason | Re-evaluate When |
|---|------|--------------|------|--------|-----------------|
| (none) | | | | | |

---

## Session Log

| Date | Changes | Commits |
|------|---------|---------|
| 2026-05-27 | 30 entries: status-transition × 1, task-completion × 17, deviation × 6, verification × 6 | `0fcc7c6`, `142907a`, `5366534`, `6475e78`, `6d319ee`, `88a169d`, `d5468ea`, `df2fa18`, `ffdc403` |
