# Plan: Lumina PTY Supervisor Service for Interactive Claude Code Sessions

**Plan path**: `docs/plans/lumina-pty-service.md`
**Created**: 2026-05-27
**Status**: Draft

## Context

Lumina is gaining a PTY-backed supervisor service so the Vue web UI can drive
**interactive `claude`** (Claude Code REPL) sessions. Goals:

- Spawn and supervise PTY-attached `claude` subprocesses; one PTY per
  session/conversation.
- Queue individual and batched user prompts against a dedicated session in
  strict FIFO order; the supervisor sends one prompt, waits for an end-of-turn
  marker, then sends the next.
- Stream input and output transparently between the UI and the PTY; the UI
  renders the **structured message content only** (no terminal emulation, no
  xterm.js, no raw ANSI rendering).
- Expose the supervisor via a dedicated HTTP+WebSocket endpoint family
  (`/api/pty/...`) and a parallel MCP tool surface so agents can drive
  sessions too.
- Structure the module so a future split (remote lumina core ⟷ local-agent
  process owning the PTYs) can plug in without rewriting callers — out of
  scope for this plan, but the transport abstraction is designed for it.
- Provide a pluggable transport seam so a future ACP backend (for
  non-interactive structured sessions) can be added alongside PTY without
  reshaping HTTP / DB / UI layers.
- Scaffold OTEL env-var passthrough to the spawned `claude` process
  (gated on a config flag) so users can plug in their own OTel Collector
  sidecar later; no in-process OTLP receiver is built.

The codebase has zero PTY / subprocess / streaming code today. Tokio "full"
features and axum 0.8 give us all the runtime primitives; `portable-pty` 0.9
gives us cross-platform PTY (Linux/macOS Unix98 + Windows ConPTY). The
supervisor sits alongside the domain model but is operationally distinct —
PTY sessions and messages are runtime state, NOT domain-traced entities, so
the `+1 work_items / +1 events` invariant does not apply. New PTY tables
write directly through repo helpers without `record_event`.

## Exploration Notes

### Backend (lumina/src/)

- **Composition root**: `lumina/src/app.rs:154–171` (`build_router`) — single function that nests
  `/api` (HTTP), `/mcp` (rmcp), and SPA fallback. New PTY routes nest under `/api`.
- **HTTP families**: each domain area gets `lumina/src/http/<family>.rs` exposing
  `pub fn router() -> Router<AppState>`, merged in `lumina/src/http/mod.rs`. This is the
  pattern for a new `/api/pty/*` family.
- **MCP tools**: `lumina/src/mcp.rs:1084+` — `#[tool_router] impl LuminaTools { ... }`,
  each `#[tool]` method delegating to `repo::*`. PTY tools fit alongside existing 39 tools.
- **Mutation path**: `lumina/src/repo.rs` — every domain write goes through
  `db::begin_write` + one row + `record_event` + commit (transactional outbox).
- **Background tasks**: `lumina/src/export.rs:347–370` — `spawn(pool) -> ExportHandle`
  with `CancellationToken` + `tokio::select!`. The canonical pattern for the PTY
  supervisor task.
- **Graceful shutdown**: `lumina/src/app.rs:120–149` (`shutdown_signal`) — Ctrl+C on
  all platforms, SIGTERM on Unix.
- **No subprocess spawning today** outside `build.rs` (build-time `bun`). No tokio
  `process` usage. No `portable-pty` / `expectrl` / `pty-process` in either crate.
- **No streaming today**: no SSE, no WebSocket, no `broadcast::channel`. axum 0.8
  provides `axum::extract::ws::WebSocket` and `Sse` out of the box.
- **No tracing / OTEL**: `tracing` is not a dependency.
- **Single-mutation-path invariant**: applies to **domain writes only**. Runtime
  PTY session/message state is NOT a domain entity in the lumina sense — it can
  live in its own tables without `events` outbox rows (or with them if audit is
  desired — but the constraint isn't binding).
- **Cross-platform**: only `#[cfg(unix)]` SIGTERM is gated today. PTY support
  on Windows differs significantly from Unix — `portable-pty` (or similar) is
  the only realistic cross-platform path.

### Frontend (lumina/web/)

- **Vapor mode**: all SFCs use `<script setup vapor>`. `createVaporApp` mounts the
  root. Options API is disabled in the Vite plugin.
- **State**: module-singleton composables (NOT Pinia, NOT provide/inject) — see
  `composables/useHierarchy.ts:42–46`. Five module-level refs, every caller
  sees the same instance.
- **No streaming today**: no `EventSource`, `WebSocket`, or `ReadableStream` usage.
- **HTTP client**: plain `fetch`, base `/api`. `api/http.ts:33–80` is the central
  `handle()` wrapper. JSON-only today — must be extended to support streamed
  responses (or kept JSON and the streaming endpoint added separately).
- **No terminal / xterm.js**: no monospace-output component beyond raw `font-mono`
  via Tailwind theme tokens.
- **Build**: Vite 8, Vue 3.6.0-beta.12, Bun test runner, Tailwind 4. `zod` for
  wire schemas. No vue-router (single-view SPA, focus state in `useHierarchy`).
- **Dev proxy**: `/api/*` proxied to `127.0.0.1:24817` (backend `DEFAULT_PORT`).

### Verification commands

- `cargo build --manifest-path lumina/Cargo.toml`
- `cargo nextest run --manifest-path lumina/Cargo.toml`
- `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- `cd lumina && cargo sqlx prepare --check`
- `cd lumina/web && npm run build` (runs type-check + vite build)
- `cd lumina/web && npm test` (bun test)

### Scope estimate (early)

~12–16 unique files: 1 migration, ~3 new backend modules (pty supervisor +
http family + MCP tool block), `repo.rs` extensions, `app.rs` wire, `Cargo.toml`
deps, ~2 new frontend modules (api family + composable), 1 new component,
`App.vue` slot, 1–2 integration tests, optionally a tracing scaffold.
At the upper edge of the recommended single-plan ceiling but manageable —
no agent batch should touch more than 6 files.

### User clarification (mid-Phase 3)

> "We will not be rendering the terminals directly, just the input and output
> content."

Implications:
- No xterm.js, no client-side ANSI parser, no scrollback buffer of raw bytes.
- Backend parses Claude Code's `--output-format stream-json` NDJSON event stream
  and exposes **semantically-typed messages** (user prompt, assistant text,
  tool-use call, tool result, system events) to the SPA.
- WebSocket framing becomes structured JSON envelopes, not raw `Uint8Array`
  chunks. The composable can use ordinary `ref()`/`shallowRef()` arrays of
  typed messages — no reactivity-hot-path concerns.
- PTY may still be needed for `claude`'s session semantics (interactive prompts,
  permission requests, OAuth flows), but the data path is structured-event,
  not raw bytes. **Phase 4 should explicitly probe whether PTY is required
  at all vs piped stdio for `claude -p --output-format stream-json`.**

## Research Notes

Cross-cutting verification: vet pass on both agents — see `[[vet_events]]`
inline below.

### PTY library choice (HIGH confidence)

- **`portable-pty` 0.9.0** is the realistic cross-platform choice.
  Linux/macOS Unix98 PTY + Windows ConPTY (Win10 1809+). Exposes
  **synchronous** `Read`/`Write` master handles — bridge to tokio via
  two `tokio::task::spawn_blocking` workers shuttling bytes through
  `tokio::sync::broadcast` (output) and `tokio::sync::mpsc` (input).
  This is the canonical wezterm/zellij/tab-rs pattern.
  Source: https://docs.rs/portable-pty, Context7 `/websites/rs_portable-pty`.
- **Rejected**: `pty-process` (Unix-only), `tokio-pty-process` (abandoned
  2019), `pseudoterminal`/`rust-pty` (unproven Windows edge cases),
  plain `tokio::process::Command + Stdio::piped` (`claude` checks `isatty`
  and switches modes — UNLESS we use `-p --output-format stream-json`,
  where TTY may not matter; **probe in Phase 4**).
- **Impact on plan**: depend on `portable-pty = "0.9"`; supervisor task
  pattern is `spawn_blocking` × 2 + broadcast/mpsc per session.

### axum 0.8 transport (HIGH confidence)

- **WebSocket via `axum::extract::ws::WebSocketUpgrade`** for the
  session stream. `socket.split()` → spawn two tasks (send driven by
  `broadcast::Receiver`, receive pushing into the per-session input
  `mpsc::Sender`). Multiple browser tabs can subscribe to one session
  via `broadcast`; handle `RecvError::Lagged(n)` by sending a UI marker
  rather than disconnecting. Enforce Origin header in handler
  (browsers don't apply same-origin to WS upgrades).
  Source: https://docs.rs/axum/0.8 ws example.
- **Rejected**: SSE+POST (half-duplex, CSRF + Last-Event-Id complexity);
  full-duplex fetch (Chromium-only); WebTransport (requires HTTP/3,
  not on hyper/axum 0.8).
- **Impact on plan**: single `/api/pty/sessions/:id/ws` route; session
  registry holds `(broadcast::Sender<Message>, mpsc::Sender<Input>)`
  where `Message`/`Input` are **typed** structures (per user
  clarification, not raw bytes).

### Claude Code CLI invocation (HIGH confidence)

- **`claude agents` is a dashboard, NOT a dispatch API.** It opens an
  interactive Agent View screen requiring a TTY; `--json` flag emits live
  sessions as a JSON array for monitoring. There is NO subcommand for
  named-agent persona dispatch.
  Source: https://code.claude.com/docs/en/cli-reference.
  **This contradicts the user's brief** which says "prefer claude agents
  over typical claude sessions". `claude agents` is for *observation*,
  not for driving a session. Surface this in Phase 4.
- **The "efficient content interchange" mode is**
  `claude -p "<prompt>" --output-format stream-json --verbose
   --include-partial-messages`
  — NDJSON, one typed JSON event per line. Event shape includes `type`
  field (`stream_event`, `system`, etc.) with subtypes (`init`,
  `api_retry`, `plugin_install`). Session continuity: `--continue`
  (latest) / `--resume <session_id>`.
  Source: https://code.claude.com/docs/en/headless.
- **`--bare` flag** skips auto-discovery (hooks, skills, plugins, MCP,
  CLAUDE.md). Recommended for scripted invocation; will become the
  `-p` default in a future release. `--agents <json>` in bare mode
  injects custom agent definitions inline.
  Source: https://code.claude.com/docs/en/headless.
- **Impact on plan**: the supervisor invokes
  `claude -p --output-format stream-json --include-partial-messages
   [--continue|--resume <id>] [--bare] [--agents <json>] [--mcp-config <json>]
   [--settings <json>]`,
  parses NDJSON line-by-line, fans typed events out via broadcast.
  Each session keeps the `session_id` it receives in the first
  `type=system, subtype=init` event so subsequent requests can `--resume`.

### Agent Client Protocol — ACP (HIGH for protocol shape, MEDIUM for crate)

- **ACP is JSON-RPC 2.0 over stdio** (same wire as LSP). Official Rust
  SDK: `agent-client-protocol` (Agent + Client traits + transport),
  `agent-client-protocol-schema` (serde + JSON Schema), and
  `agent-client-protocol-conductor` (proxy chains).
  Source: https://crates.io/crates/agent-client-protocol,
  https://github.com/agentclientprotocol/agent-client-protocol.
- **Claude has no native ACP server**; ACP is reached via an adapter
  (`claude-agent-acp` JS or `claude-code-acp-rs` community Rust). The
  Rust crate is community-maintained — confidence MEDIUM; inspect
  before adoption.
- **ACP vs PTY split**: ACP gives structured tool-call/permission
  lifecycle; PTY gives raw TTY fidelity. The user wants both as
  complementary surfaces in the same supervisor.
  Source: https://www.morphllm.com/agent-client-protocol.
- **Impact on plan**: ship the PTY+stream-json path first; ACP is a
  second adjacent endpoint family (`/api/acp/...`) that the supervisor
  module exposes as a separate transport against the same session
  registry. Design the session registry abstraction so PTY and ACP are
  pluggable backends.

### OTEL ingest (HIGH for emitter side, LOW for in-process receiver)

- **Claude Code emits OTLP metrics + logs natively** (≥ v2.1.x).
  Required env vars: `CLAUDE_CODE_ENABLE_TELEMETRY=1`,
  `OTEL_METRICS_EXPORTER=otlp`, `OTEL_LOGS_EXPORTER=otlp`,
  `OTEL_EXPORTER_OTLP_PROTOCOL=grpc`,
  `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317`. Distributed
  traces beta: add `CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1` and
  `OTEL_TRACES_EXPORTER=otlp`. Subprocess TRACEPARENT propagation is
  automatic. Default intervals: 60s metrics, 5s logs.
  Source: https://code.claude.com/docs/en/monitoring-usage.
- **In-process Rust OTLP receiver: not feasible cheaply.**
  `opentelemetry-otlp` is an emitter library, not a receiver. There is
  no out-of-the-box "OTLP receiver" Rust crate. Embedding one would
  require implementing the `opentelemetry.proto.collector.*` tonic gRPC
  service stubs by hand. Sidecar (OTel Collector binary) is dramatically
  lower-cost.
- **Impact on plan**: OTEL is an **optional** layer. If included now:
  set the env vars on the spawned `claude` subprocess and document that
  the user must run an OTel Collector sidecar. If deferred: leave the
  env-var passthrough scaffolded but document the receiver as "BYO
  collector" / future scope.

### Vapor / SPA notes (HIGH)

- Browser `WebSocket` wrapped in a module-singleton composable
  (matches the project pattern; no Pinia). Register
  `onScopeDispose(() => ws.close(1000))` for cleanup. Per user
  clarification, frames are typed JSON messages → ordinary
  `ref<Message[]>` / `shallowRef<Message[]>` is fine; no `Uint8Array`
  ring buffer or terminal emulator needed.
- **Impact on plan**: `composables/usePtySession.ts` exposes
  `{ messages: Ref<Message[]>, status: Ref<'connecting'|'open'|'closed'|'error'>,
     submit: (text: string) => Promise<void>, close: () => void }`.

### Directed research additions (Phase 5)

User asked: does interactive `claude` (no `-p`) support structured output to
avoid server-side ANSI parsing? Findings (Sonnet, ≤6, all HIGH unless noted):

- **`--output-format` is `-p`-mode-only.** Docs explicitly state the flag
  applies to print mode; no equivalent exists for the bare interactive REPL.
  Source: https://code.claude.com/docs/en/cli-reference.
- **No `--no-tui` / `--headless` / `--structured` / `--serve` / `--rpc` /
  `--daemon` flag exists** for interactive mode. The only "serve" surface is
  `claude remote-control`, which is an outbound HTTPS relay to claude.ai —
  not a local programmable endpoint. Discard.
- **`claude -p --input-format stream-json --output-format stream-json
  --verbose --replay-user-messages` is a long-running multi-turn structured
  I/O process** — NDJSON in on stdin, NDJSON typed events out on stdout,
  one persistent process. This is the closest non-interactive substitute,
  but it is still `-p` mode (no interactive prompts, no OAuth, no permission
  callbacks). Source: https://code.claude.com/docs/en/cli-reference.
- **No Rust binding for the Agent SDK** — only Python and TypeScript
  packages. Rust wraps the `-p` CLI directly. Source:
  https://code.claude.com/docs/en/agent-sdk/overview.
- **Per-turn chaining via `claude -p --resume <id>` works** and gives
  structured output per call, but costs process-startup latency per turn
  and loses in-flight async state across processes.

**Phase 5 outcome / design decision**: User has chosen PTY + interactive
`claude` (twice — agent-dispatch and PTY-vs-piped answers). The Phase 5
finding does NOT change this — `-p --input-format stream-json` is the
non-interactive path the user reserved for ACP. The PTY supervisor needs
**server-side ANSI handling** because no structured-output flag exists for
interactive mode. Plan: use the `vt100` crate (or `strip-ansi-escapes`)
on the supervisor side to extract plain text content from PTY output, then
heuristically segment Claude's TUI regions (assistant turn, tool-call
display, prompt) into typed messages emitted to the UI. This is genuinely
the harder path — call it out as a known design risk in `## Risks`.

The `--input-format stream-json + --replay-user-messages` mode is recorded
here as a **future alternative**: if Claude Code ever exposes a structured
output flag for the interactive REPL, or if the user later decides that
the non-interactive structured path is sufficient, the supervisor can swap
backends through the transport abstraction without rewriting the UI / DB
layer.

[[vet_events]]:
- agent: flow-research-deep (Opus, Lens 1-3); sampled: portable-pty 0.9
  cross-platform + sync API, axum WebSocket pattern, pty-process Unix-only;
  dropped: 0; downgraded: 0; note: ESCALATE-TO-DEEP items resolved moot
  by user "no terminal rendering" clarification.
- agent: flow-research (Sonnet, claude/ACP/OTEL); sampled: claude `-p`
  stream-json mode, ACP protocol+SDK shape, Claude OTEL env vars;
  dropped: 0; downgraded: 1 (`claude-code-acp-rs` crate provenance —
  community-only, must inspect before adoption); note: `claude agents`
  finding flagged as user-brief contradiction for Phase 4.
- agent: flow-research Phase 5 (Sonnet, interactive structured output);
  sampled: `--output-format` print-mode-only, `--input-format stream-json
  --replay-user-messages` long-running process, no Rust SDK; dropped: 0;
  downgraded: 0; note: key finding recorded as future-alternative; current
  plan still requires server-side ANSI parsing per user's interactive +
  PTY choice.

## User Decisions

| # | Question | Answer | Prompting finding |
|---|----------|--------|-------------------|
| Q1 | Agent dispatch mechanism | `-p` will be non-interactive (reserved for future ACP layer). The PTY service specifically drives the **interactive `claude` REPL**. | Agent-2 Topic 1 — `claude agents` is a TTY-only dashboard; user clarified the division of responsibility between PTY (interactive) and future ACP (non-interactive). |
| Q2 | PTY vs piped stdio | **PTY via `portable-pty` 0.9** for full TTY fidelity. | Agent-1 Lens 1 — `claude` checks `isatty`; PTY preserves interactive prompts, OAuth, permission callbacks. |
| Q3 | Session persistence | **Persist sessions + messages in SQLite**; recreate `claude --resume <sid>` on demand when a cold session is reopened. | Q for batching durability — user wants queued requests to survive restarts. |
| Q4 | ACP scope in this plan | **Defer ACP**; design the supervisor with a pluggable transport abstraction so an ACP backend slots in later without rework. | Agent-2 Topic 2 — ACP requires a separate adapter (`claude-code-acp-rs` MEDIUM-LOW confidence or Node `claude-agent-acp` sidecar); keeping it out of this plan keeps scope bounded. |
| Q5 | Output parsing strategy | **Phase 5 investigation requested** — directed research confirmed no structured-interactive output flag exists. Plan therefore uses server-side ANSI handling (`vt100` or `strip-ansi-escapes`) to extract typed content blocks from the PTY byte stream. | Phase 5 outcome above. |
| Q6 | Batching semantics | **Strict FIFO queue per session**; supervisor waits for an assistant end-of-turn marker (idle PTY output + prompt heuristic) before sending the next request. | User confirmed UI should be able to enqueue individual or batched requests against one conversation. |
| Q7 | Auth / security model | **Bind localhost only, no auth.** Remote-split is future scope. | Matches lumina's current binding model; pre-emptive auth is over-build. |
| Q8 | OTEL scope | **Defer.** Scaffold only env-var passthrough on the spawned subprocess (gated on a config flag); document BYO OTel Collector sidecar for users who want telemetry. | Agent-2 Topic 3 — in-process OTLP receiver is non-trivial (LOW confidence); env-var passthrough is cheap and sufficient. |

## Scope

- **In scope**
  - Cross-platform PTY supervisor (Linux/macOS Unix98 + Windows ConPTY) for
    interactive `claude` REPL sessions.
  - `Transport` trait abstraction with a `PtyTransport` implementation now;
    `AcpTransport` and `RemoteRpcTransport` are placeholder slots in the
    module layout but unimplemented.
  - SQLite persistence for sessions, structured messages, and the per-session
    input queue (new migration `0008_pty_sessions.sql`).
  - Server-side ANSI handling via the `vt100` crate to extract typed message
    blocks (assistant text, prompt, error) from PTY output.
  - Per-session FIFO input queue with end-of-turn detection heuristic
    (idle output + prompt pattern match).
  - HTTP REST family `/api/pty/sessions/...` + a WebSocket endpoint
    `/api/pty/sessions/:id/ws` for live bidirectional streaming.
  - MCP tools for spawn / list / get / send-input / cancel / delete.
  - Vue 3 Vapor SPA: `api/pty.ts` family, `composables/usePtySessions.ts`
    + `composables/usePtySession.ts` module-singletons, `components/PtyConsole.vue`
    + `components/PtyMessage.vue`, layout slot in `App.vue`.
  - Background supervisor task wired into `app::serve` via the `export.rs`
    `ExportHandle` pattern (CancellationToken + tokio::select!).
  - OTEL env-var passthrough (`CLAUDE_CODE_ENABLE_TELEMETRY` + `OTEL_*`)
    when spawning the child, gated on a config flag in `SpawnConfig`.
  - In-process integration test using a stub binary fixture (no real
    `claude` required); unit tests for parser + queue + composables.

- **Out of scope**
  - Remote split of lumina core ⟷ local PTY agent (transport seam designed
    for this, but no remote implementation in this plan).
  - ACP transport implementation (slot exists; deferred).
  - In-process OTLP receiver (BYO sidecar collector).
  - Terminal emulation / xterm.js / scrollback rendering — UI shows typed
    messages only.
  - Auth — supervisor binds localhost; assume same-trust model as the rest
    of lumina's HTTP/MCP surface today.
  - OAuth / interactive permission prompt handling beyond what PTY input
    forwarding naturally provides.
  - Per-session resource limits / cgroups / sandboxing.

- **Affected areas**
  - `lumina/src/` — new `pty/` module tree; new `http/pty_sessions.rs`;
    extensions to `mcp.rs`, `repo.rs`, `domain.rs`, `app.rs`, `error.rs`.
  - `lumina/Cargo.toml` — add `portable-pty`, `vt100`, `async-trait`,
    `bytes` (latest only if not already transitive), `uuid` already present.
  - `lumina/migrations/` — `0008_pty_sessions.sql`.
  - `lumina/web/src/` — new `api/pty.ts`, two composables, two components,
    `App.vue` slot.
  - `lumina/.sqlx/` — offline cache regeneration after migration.
  - `lumina/tests/` — new `pty_e2e.rs` + stub-binary helper.

- **Estimated file count**: 17 unique files (5 modified, 12 new).
  Within the recommended single-plan ceiling; the largest agent batch
  touches 4 files (T9 — REST + WS + extension to `http/mod.rs`).

## Approach

### Module layout (backend)

New tree under `lumina/src/pty/`:

```
pty/
  mod.rs            — public re-exports; ties the subsystem together
  protocol.rs       — wire types: TypedMessage, InputFrame, MessageKind, SessionStatus
  transport.rs      — Transport trait + TransportHandle + SpawnConfig
  pty_transport.rs  — portable-pty implementation (2× spawn_blocking workers)
  parser.rs         — vt100-based ANSI handler + end-of-turn heuristic
  session.rs        — Session struct (id, status FSM, broadcast::Sender,
                      mpsc::Sender, queue head)
  registry.rs       — SessionRegistry: Arc<RwLock<HashMap<SessionId, Arc<Session>>>>
  supervisor.rs     — spawn() / shutdown() — same shape as export::spawn
  queue.rs          — per-session FIFO with persistence sync
```

The `Transport` trait keeps `pty_transport` swappable for future ACP /
remote implementations. `TransportHandle` returns
`(broadcast::Receiver<TypedMessage>, mpsc::Sender<InputFrame>, oneshot for exit,
CancellationToken)` — the same shape every transport must satisfy.

### Process lifecycle

1. **Spawn** (POST `/api/pty/sessions`):
   - Insert `pty_sessions` row (status=`spawning`).
   - Build `SpawnConfig { cwd, env_passthrough_otel, agent_json, model, claude_args }`.
   - `PtyTransport::spawn` opens a PTY via `portable_pty::native_pty_system()`,
     spawns `claude` (no `-p` flag — interactive REPL).
   - Two `tokio::task::spawn_blocking` workers: reader pulls from
     `master.try_clone_reader()` into a `mpsc::Sender<Bytes>`; writer pulls
     from a `mpsc::Receiver<Bytes>` and writes to `master.take_writer()`.
   - A tokio task fronts the reader channel: feeds bytes into the `vt100`
     parser; emits `TypedMessage` events to a per-session
     `broadcast::Sender<TypedMessage>`; persists each as a `pty_messages` row.
   - Status transitions to `active` → `idle` when the parser detects the
     first prompt.

2. **Send input** (WS frame or POST `/api/pty/sessions/:id/input`):
   - Enqueue an `InputFrame` row in `pty_queue` (status=`pending`).
   - When session status is `idle`, dispatcher pops next pending input,
     writes to the writer channel (newline-terminated), persists a
     `pty_messages` row of kind `user_input`, sets status to `awaiting`,
     and stamps `dispatched_at` on the queue row.
   - When end-of-turn is detected (idle output > 750ms AND last non-empty
     stripped line matches the prompt pattern, e.g. `> ` or
     `Human:`-style), the dispatcher stamps `completed_at` on the queue
     row and transitions the session back to `idle`. Pops next pending if any.

3. **Cancel** (WS frame or DELETE `/api/pty/sessions/:id`):
   - Cancellation token fires; writer task sends a control sequence
     (`ETX`/`Ctrl-C`); supervisor waits up to N seconds; if process is
     still alive, `master.kill_child()`. Status → `cancelled` / `completed`.

4. **Shutdown** (lumina graceful shutdown):
   - Supervisor cancels all per-session tokens, awaits their `oneshot`
     completion signals, then returns. Sessions persist in SQLite with
     final status; child processes terminated cleanly.

5. **Cold reattach** (UI opens a session whose lumina-side state was lost,
   e.g. after a restart):
   - The supervisor does NOT auto-respawn at boot. The UI sees the
     persisted session in `completed`/`cancelled` status; clicking
     "reopen" issues a new POST `/api/pty/sessions` with the prior
     session's metadata (cwd, agent, model) and the prior session id is
     recorded as `previous_session_id` for cross-reference. We do not
     attempt to `claude --resume` because (a) the claude session id is
     not reliably emitted by the interactive REPL and (b) the user's
     local `.claude/projects/` state already provides Claude's own
     conversation continuity if applicable.

### ANSI handling — parser strategy (the highest-risk piece)

The `vt100` crate maintains a virtual screen. We don't render it — instead:

- Feed every PTY chunk into `vt100::Parser::process`.
- After each chunk, read the new contents of the alternate screen / scrollback
  via `parser.screen().rows_formatted()` or equivalent.
- Maintain a "logical content cursor": track where we last emitted content
  to the UI, emit only new finalised text.
- Detect message boundaries heuristically:
  - `user_input` is emitted at write time (we know what we sent).
  - `assistant_text` accrues until end-of-turn.
  - `tool_call` is heuristic: lines starting with a tool-call sigil
    (e.g. `⏺ ToolName(...)`) start a new block; the next blank line
    or a different sigil ends it.
  - `prompt` is the line matching the prompt pattern at the bottom.
  - `error` is anything emitted on stderr or matching known error prefixes.

This is fragile — if Claude Code changes its TUI rendering, the parser
breaks silently. Mitigate by:
- Keeping the raw bytes (or vt100-stripped text) on the message row as
  `raw_text` alongside the parsed structured fields, so the UI can fall
  back to plain text.
- A `parse_strategy_version` column on `pty_sessions` so we can roll out a
  new parser version without breaking old rows.
- Logging unrecognised regions to a `parser_unknown` message kind rather
  than dropping them.

### Database schema (migration `0008_pty_sessions.sql`)

```sql
CREATE TABLE pty_sessions (
    id TEXT PRIMARY KEY,                  -- uuid v7
    label TEXT,                           -- user-set, nullable
    project_id TEXT REFERENCES work_items(id) ON DELETE SET NULL,
    cwd TEXT NOT NULL,
    config_json TEXT NOT NULL,            -- SpawnConfig snapshot
    parse_strategy_version INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL,                 -- spawning|active|idle|awaiting|completed|failed|cancelled
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    ended_at TEXT,                        -- nullable
    exit_code INTEGER,
    last_error TEXT,
    previous_session_id TEXT REFERENCES pty_sessions(id) ON DELETE SET NULL
);

CREATE INDEX idx_pty_sessions_project ON pty_sessions(project_id) WHERE project_id IS NOT NULL;
CREATE INDEX idx_pty_sessions_status ON pty_sessions(status);

-- Enforce that project_id, when set, references a row where kind='project'.
-- Mirrors the BEFORE INSERT trigger pattern from migrations 0004/0005 — a
-- column CHECK cannot subquery sibling rows in SQLite, so the guard runs as
-- a trigger. Same shape applies to UPDATE statements that mutate project_id.
CREATE TRIGGER pty_sessions_project_kind_check_insert
BEFORE INSERT ON pty_sessions
FOR EACH ROW WHEN NEW.project_id IS NOT NULL
BEGIN
  SELECT CASE
    WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) != 'project'
    THEN RAISE(ABORT, 'pty_sessions.project_id must reference a work_items row with kind=project')
  END;
END;

CREATE TRIGGER pty_sessions_project_kind_check_update
BEFORE UPDATE OF project_id ON pty_sessions
FOR EACH ROW WHEN NEW.project_id IS NOT NULL
BEGIN
  SELECT CASE
    WHEN (SELECT kind FROM work_items WHERE id = NEW.project_id) != 'project'
    THEN RAISE(ABORT, 'pty_sessions.project_id must reference a work_items row with kind=project')
  END;
END;

CREATE TABLE pty_messages (
    id TEXT PRIMARY KEY,                  -- uuid v7
    session_id TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,            -- per-session monotonic
    created_at TEXT NOT NULL,
    kind TEXT NOT NULL,                   -- user_input|assistant_text|tool_call|tool_result|prompt|system|error|parser_unknown
    content_json TEXT NOT NULL,           -- typed payload
    raw_text TEXT,                        -- ansi-stripped fallback, nullable
    UNIQUE(session_id, sequence)
);

CREATE INDEX idx_pty_messages_session ON pty_messages(session_id, sequence);

CREATE TABLE pty_queue (
    id TEXT PRIMARY KEY,                  -- uuid v7
    session_id TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    input_kind TEXT NOT NULL,             -- prompt|cancel|control
    payload TEXT NOT NULL,
    enqueued_at TEXT NOT NULL,
    dispatched_at TEXT,
    completed_at TEXT,
    status TEXT NOT NULL,                 -- pending|dispatched|completed|failed|cancelled
    error TEXT,
    UNIQUE(session_id, sequence)
);

CREATE INDEX idx_pty_queue_pending ON pty_queue(session_id, sequence) WHERE status = 'pending';
```

These tables do **not** participate in the `events` outbox. Writes go
through `db::begin_write` for atomicity (insert + status update happen
together) but no `record_event` call follows.

### HTTP family `lumina/src/http/pty_sessions.rs`

- `GET    /api/pty/sessions` — list (filter `?status=`, `?project_id=`)
- `POST   /api/pty/sessions` — spawn (returns session id + initial status)
- `GET    /api/pty/sessions/:id` — detail (session row only)
- `GET    /api/pty/sessions/:id/messages?since=<seq>&limit=<n>` — paginated history
- `GET    /api/pty/sessions/:id/queue` — pending inputs (queue inspection)
- `POST   /api/pty/sessions/:id/input` — enqueue one input (REST fallback)
- `POST   /api/pty/sessions/:id/inputs/batch` — enqueue N inputs atomically
- `PATCH  /api/pty/sessions/:id` — update label / project_id
- `DELETE /api/pty/sessions/:id` — cancel (terminate process + status update)
- `GET    /api/pty/sessions/:id/ws` — WebSocket upgrade for live stream

WebSocket frames (JSON):
- Client→server: `{type:"input",kind:"prompt",text}`,
  `{type:"input",kind:"cancel"}`, `{type:"input",kind:"control",signal:"CTRL_C"}`,
  `{type:"resize",cols,rows}`, `{type:"ping"}`.
- Server→client: `{type:"message",sequence,kind,content,raw_text,created_at}`,
  `{type:"status",status,at}`, `{type:"skipped",bytes,reason:"broadcast-lag"}`,
  `{type:"error",code,message}`, `{type:"pong"}`.

WS handler subscribes to the per-session `broadcast::Receiver<TypedMessage>`
(handles `Lagged(n)` with a `skipped` frame, never disconnects). Client
input frames go through the same queue path as POST `/input`.

### MCP tools (extend `lumina/src/mcp.rs`)

Six new tools added to the existing `LuminaTools` `#[tool_router]` impl:
`list_pty_sessions`, `get_pty_session`, `spawn_pty_session`,
`send_pty_input`, `cancel_pty_session`, `delete_pty_session`. Each maps
1:1 to a `pty::repo` helper. `spawn_pty_session` validates the cwd is
under the worktree root.

### Frontend (lumina/web/src/)

- `api/pty.ts` — zod schemas (`PtySessionSchema`, `PtyMessageSchema`,
  `InputFrameSchema`), fetch wrappers (`listSessions`, `spawnSession`,
  `getSession`, `getMessages`, `sendInput`, `cancelSession`,
  `deleteSession`), WebSocket opener (`openSessionStream(id) → { send, on, close }`).
- `composables/usePtySessions.ts` — module-singleton: `sessions` ref,
  `loadSessions`, `spawn`, `select` (sets focus session id).
- `composables/usePtySession.ts` — module-singleton bound to focus
  session id: `messages` ref (append-only), `status` ref, `submit`,
  `cancel`, `connect`/`disconnect` (manages the WebSocket). Uses
  `onScopeDispose` for cleanup. Loads history from REST on connect
  before subscribing to the WS stream.
- `components/PtyConsole.vue` — Vapor SFC: messages list (uses
  `PtyMessage.vue` per row), input box + send button, status pill,
  cancel/delete actions. NO terminal emulation — just a typed message
  feed with prompt input.
- `components/PtyMessage.vue` — per-kind rendering: assistant_text as
  prose block, tool_call as a collapsed-by-default summary, error
  prominent, parser_unknown falls back to raw_text.
- `App.vue` — add a new layout view (third toggle alongside the existing
  `focus`/`tree`) for PTY console; mounts `PtyConsole` when active.

### Background supervisor wire-up (lumina/src/app.rs)

Follow the `export::spawn(pool.clone())` pattern at `app.rs:82-107`. Add:
```rust
let pty_handle = pty::supervisor::spawn(pool.clone(), registry.clone());
```
before `axum::serve`; await `pty_handle.shutdown().await` in the graceful
shutdown path. `AppState` gains a `pty_registry: Arc<pty::registry::SessionRegistry>`
field passed via `.with_state` so HTTP and MCP handlers reach the same registry.

### Reused patterns

- Background task: `lumina/src/export.rs:347-370` (`spawn` returning
  `ExportHandle` with `CancellationToken` + `tokio::select!`).
- Graceful shutdown signal: `lumina/src/app.rs:120-149` (`shutdown_signal`).
- HTTP family pattern: any of `lumina/src/http/*.rs` (e.g. `work_items.rs`
  exposing `pub fn router() -> Router<AppState>` then merged in `http/mod.rs`).
- MCP tool pattern: `lumina/src/mcp.rs` `#[tool_router] impl LuminaTools { #[tool] async fn ... }`.
- `db::begin_write` + atomic insert pattern from `lumina/src/repo.rs`
  (note: PTY writes skip `record_event`).
- AppError variants from `lumina/src/error.rs` (`Validation`, `NotFound`).
- Vue module-singleton composable pattern from
  `lumina/web/src/composables/useHierarchy.ts:42-46` and `useScalars.ts`.

### Rejected alternatives

- **Piped stdio (`tokio::process::Command` + `Stdio::piped`)** —
  rejected because `claude` checks `isatty()` and the interactive REPL
  is the explicit user choice (PTY for full TTY fidelity).
- **`-p --input-format stream-json --output-format stream-json
  --replay-user-messages`** (long-running structured `-p` process) —
  rejected for this plan because the user reserved `-p` (non-interactive)
  for the future ACP layer. Recorded in Research Notes as a future
  alternative if interactive parsing proves too fragile.
- **xterm.js client-side rendering** — rejected per user clarification:
  no terminal rendering, structured content only.
- **SSE + paired POST** — rejected in favour of WebSocket for true
  bidirectional framing and multi-subscriber fanout via `broadcast`.

### Important constraints

- **Error visibility discipline (no tracing subscriber today)** — lumina
  has no `tracing` subscriber installed, so PTY errors that aren't
  persisted are silently lost. EVERY error path in `pty_transport.rs`,
  `parser.rs`, `session.rs`, and `supervisor.rs` MUST round-trip to
  `pty_sessions.last_error` via `repo::pty::update_pty_session_status(id,
  Failed, Some(error_message))` (or the equivalent atomic transition).
  Tracing is out of scope until a global subscriber lands; that is a
  separate follow-up.
- **Single-mutation atomicity for status + last_error** — when an error
  fires, `status='failed'` and `last_error` MUST be set in the same
  `db::begin_write` transaction so a partial-write crash never leaves a
  session in `active`/`awaiting` with no error explanation.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets --no-deps
```

Additional gates (run before each phase commit):

```
sqlx:  cargo sqlx prepare --manifest-path lumina/Cargo.toml --check
web:   cd lumina/web && npm run build
webtest: cd lumina/web && npm test
fmt:   cargo fmt --manifest-path lumina/Cargo.toml --check
```

## Tasks

### Phase 1: Foundations (parallel — 3 tasks)

#### 1. Add dependencies + write migration 0008 [S]
- **Files**: `lumina/Cargo.toml`, `lumina/migrations/0008_pty_sessions.sql`
- **Depends on**: —
- **Action**: Add `portable-pty = "0.9"`, `vt100 = "0.16.2"`, `async-trait = "0.1"`,
  and `bytes = "1"` to `[dependencies]`. Update the existing `axum = "0.8"` line to
  `axum = { version = "0.8", features = ["ws"] }` (`ws` is NOT a default feature — verified
  docs.rs/crate/axum/0.8.7/features). Also add `assert_cmd = "2"` and `tokio-tungstenite =
  "0.24"` to `[dev-dependencies]` for the Task 16 e2e test. Write the migration SQL
  exactly as specified in the Approach section's "Database schema" subsection.
- **Detail**: Confirm `portable-pty` 0.9.x latest on crates.io; pin if a
  newer minor exists. `vt100 = "0.16"` per latest doc check; verify via
  `cargo search vt100` before pinning. `async-trait` only needed if the
  `Transport` trait uses async methods (likely yes given spawn returns
  futures). Migration must include all three tables, all indexes,
  and the FK constraints exactly as in the schema block.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds
  (no usages yet, so only resolves deps); `cd lumina && cargo sqlx prepare
  --check` is NOT run yet (no `query!` macros added in this task).

#### 2. Define PTY wire types in `pty/protocol.rs` + pre-declare full module tree [S]
- **Files**: `lumina/src/pty/mod.rs`, `lumina/src/pty/protocol.rs`,
  `lumina/src/lib.rs`
- **Note**: Task 2 writes the COMPLETE pty/mod.rs upfront with all eight
  `pub mod ...;` declarations (protocol, transport, pty_transport, parser,
  session, registry, queue, supervisor) plus planned re-exports. Tasks 3, 4,
  5, 6, 8 do NOT touch mod.rs — they only add content to their owned files.
- **Depends on**: —
- **Action**: Create the `pty/` module tree. Define `SessionId(Uuid)`,
  `SessionStatus` enum (Spawning|Active|Idle|Awaiting|Completed|Failed|Cancelled),
  `MessageKind` enum (UserInput|AssistantText|ToolCall|ToolResult|Prompt|System|Error|ParserUnknown),
  `TypedMessage { sequence, kind, content, raw_text, created_at }`,
  `InputFrame { kind: InputKind, payload }`, and `InputKind` enum
  (Prompt|Cancel|Control). Derive `Serialize`, `Deserialize`, `Clone`,
  `Debug`. `pty/mod.rs` exports a public re-list; add `pub mod pty;`
  declaration to `lib.rs` (matching the existing frozen-module convention).
- **Detail**: `content` field is `serde_json::Value` to allow per-kind
  payload shapes without an enum proliferation. Avoid `Cow<'static, str>`
  — all string fields are owned `String`. Match the snake_case wire
  convention used elsewhere in `domain.rs` via `#[serde(rename_all = "snake_case")]`.
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds;
  `cargo clippy --manifest-path lumina/Cargo.toml --all-targets --no-deps`
  passes without warnings on the new module.

#### 3. Define `Transport` trait + handle types in `pty/transport.rs` [S]
- **Files**: `lumina/src/pty/transport.rs`, `lumina/src/pty/mod.rs`
- **Depends on**: 2
- **Action**: Define `#[async_trait] trait Transport: Send + Sync` with
  `async fn spawn(&self, config: SpawnConfig) -> Result<TransportHandle, AppError>`.
  Define `SpawnConfig { cwd: PathBuf, claude_args: Vec<String>,
  agent_json: Option<String>, model: Option<String>,
  env_passthrough_otel: bool, settings_json: Option<String> }` and
  `TransportHandle { session_id: SessionId, outbound:
  broadcast::Receiver<TypedMessage>, inbound: mpsc::Sender<InputFrame>,
  shutdown: CancellationToken, completed:
  oneshot::Receiver<ExitStatus> }` (or wrap `ExitStatus` in a local
  newtype that's `Serialize`).
- **Detail**: `ExitStatus` is `std::process::ExitStatus` — not
  serializable directly, so define a local `pub struct SessionExit
  { code: Option<i32>, signal: Option<i32>, success: bool }`. Trait
  methods are async; uses `async_trait`. Re-export from `pty/mod.rs`.
- **Acceptance**: `cargo build` succeeds. Trait compiles even without
  any implementation.

### Phase 2: Backend implementation (parallel — 4 tasks, after Phase 1)

#### 4. Implement `PtyTransport` via `portable-pty` in `pty/pty_transport.rs` [M]
- **Files**: `lumina/src/pty/pty_transport.rs`, `lumina/src/pty/mod.rs`
- **Depends on**: 3
- **Action**: Implement `impl Transport for PtyTransport`. In `spawn`:
  open a PTY via `native_pty_system().openpty(PtySize { rows: 24, cols:
  80, pixel_width: 0, pixel_height: 0 })`. Build a `CommandBuilder::new("claude")`
  with `claude_args`. If `env_passthrough_otel`, set `CLAUDE_CODE_ENABLE_TELEMETRY=1`
  + the four `OTEL_*` env vars (passing through from the parent process env
  if present; otherwise default `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317`).
  Set `cwd(config.cwd)`. Call `pair.slave.spawn_command(cmd)`.
- **Detail**: Two `tokio::task::spawn_blocking` workers — one wraps the
  master's reader (`pair.master.try_clone_reader()`) reading in chunks
  (e.g. 4096 bytes) and pushing `Bytes` into an `mpsc::Sender<Bytes>`
  passed in; the other consumes from an `mpsc::Receiver<Bytes>` and
  writes to the master writer (`pair.master.take_writer()`). Use
  `tokio_util::sync::PollSender` or simple `try_send` for
  cross-runtime hand-off. Wire a tokio task that bridges reader-side
  bytes into the parser pipeline (parser lives in `parser.rs`, called
  via a channel here for separation of concerns). Cancellation token
  triggers `pair.master.kill_child()` then drops the master to unblock
  the blocking reader; the blocking writer exits on closed channel.
  Capture child exit via `child.try_wait` in a poll loop on a third
  short-running task (or use `tokio::task::spawn_blocking { child.wait()
  }`) — emit through the `oneshot` sender on the handle.
- **Acceptance**: `cargo build` + `cargo clippy` clean. A minimal manual
  invocation (`echo` stub binary used in tests) demonstrates spawn →
  bytes-out → cancel → child-exit signalling.

#### 5. Implement `vt100` parser + end-of-turn heuristic in `pty/parser.rs` [M]
- **Files**: `lumina/src/pty/parser.rs`, `lumina/src/pty/mod.rs`
- **Depends on**: 2
- **Action**: Define `pub struct Parser { vt: vt100::Parser, last_emitted: ScreenCursor,
  idle_since: Option<Instant>, prompt_re: Regex }`. Method
  `pub fn feed(&mut self, bytes: &[u8]) -> Vec<TypedMessage>` advances
  the vt100 parser, compares the resulting screen state to `last_emitted`,
  segments new content into `TypedMessage` blocks (see Approach §parser),
  and returns them in order.
- **Detail**: End-of-turn detection: track `idle_since` — set to `Some(now)`
  on first feed call that produced no new content (i.e. parser state didn't
  change); cleared on any new content. A separate method
  `pub fn check_idle(&mut self, now: Instant, threshold: Duration) -> bool`
  returns true if the prompt pattern matches the bottom line AND
  `idle_since` is older than threshold. Prompt regex: default is the compiled-once `Regex::new(r"^\s*[>›❯]\s*$|^Human:\s*$").unwrap()`, stored as a `Parser` field. Override via optional `prompt_pattern: Option<String>` on SpawnConfig (default None → use the built-in). The implementer does NOT verify against captured-real-claude fixtures in this plan; risk is accepted that the pattern may need post-launch tuning. The manual 'advance queue' UI button in Task 14 is the front-line backstop. Parser version field included in returned messages so the
  caller can stamp `parse_strategy_version`. ANY content the parser
  cannot categorise is emitted as `MessageKind::ParserUnknown` with
  the raw text in `raw_text`, never dropped.
- **Acceptance**: Unit tests in the same file (`#[cfg(test)] mod tests`)
  feed canned byte sequences (e.g. captured from a real claude session
  if available, or synthetic ANSI strings) and assert the parsed
  `TypedMessage` shape. `cargo nextest run -p lumina parser::` passes.

#### 6. Implement `Session` + `SessionRegistry` in `pty/session.rs`, `pty/registry.rs`, `pty/queue.rs` [M]
- **Files**: `lumina/src/pty/session.rs`, `lumina/src/pty/registry.rs`,
  `lumina/src/pty/queue.rs`, `lumina/src/pty/mod.rs`
- **Depends on**: 3
- **Action**: `Session { id, status: Mutex<SessionStatus>, broadcast_tx,
  input_tx, parser: Mutex<Parser>, exit_rx }`. `SessionRegistry {
  inner: Arc<RwLock<HashMap<SessionId, Arc<Session>>>> }` with methods
  `insert / get / remove / list / contains`. `Queue` thin wrapper
  around the `pty_queue` table: `enqueue`, `pop_next_pending`,
  `mark_dispatched`, `mark_completed`, `mark_failed`.
- **Detail**: `Session` is the per-process state container; the
  supervisor task (Task 8) drives status transitions. Broadcast channel
  capacity: 1024 frames (tunable). Input mpsc capacity: 64 frames.
  Queue methods take a `&SqlitePool` so they're trivially testable.
  Status `Mutex` is fine — contention is rare (status mutates only on
  lifecycle events, not per-byte). NO `record_event` calls.
- **Acceptance**: `cargo build` + `cargo clippy` clean. Smoke unit test:
  insert/get/remove against `SessionRegistry`; enqueue→pop_next_pending
  round-trip against an in-memory SQLite from `db::connect_in_memory()`.

#### 7. Extend `repo.rs` with PTY CRUD helpers [M]
- **Files**: `lumina/src/repo.rs`, `lumina/src/domain.rs`
- **Depends on**: 1, 2
- **Action**: Add `pub mod pty { ... }` (or inline `pty_*` prefixed functions)
  with: `create_pty_session`, `update_pty_session_status`,
  `update_pty_session_ended`, `list_pty_sessions` (with optional
  `status` + `project_id` filters), `get_pty_session`, `delete_pty_session`
  (soft via status='cancelled' AND `ended_at` stamp), `insert_pty_message`,
  `list_pty_messages` (paginated by `since` + `limit`). All use
  `db::begin_write` for write paths; reads use the pool directly. NO
  `record_event` invocations. Add corresponding `PtySession`,
  `PtyMessage` typed structs to `domain.rs` for serialisation.
- **Detail**: After this task, `cargo sqlx prepare --check` will go
  stale — run `cargo sqlx prepare --manifest-path lumina/Cargo.toml`
  (no `--check`) to refresh `.sqlx/` and commit the cache. Note: PR
  cycle should include the `.sqlx/` update; document this in commit
  body. Domain structs must derive `Serialize` (response) and use
  `#[serde(rename_all = "snake_case")]`.
- **Acceptance**: `cargo build` + `cd lumina && cargo sqlx prepare --check`
  both clean (after committing the refreshed cache). `cargo nextest run`
  passes including any new repo-level smoke tests.

#### 8. Implement supervisor task in `pty/supervisor.rs` [S]
- **Files**: `lumina/src/pty/supervisor.rs`, `lumina/src/pty/mod.rs`
- **Depends on**: 4, 5, 6, 7
- **Action**: Define `pub struct SupervisorHandle { token: CancellationToken,
  join: JoinHandle<()> }` and `pub fn spawn(pool: Arc<SqlitePool>, registry:
  Arc<SessionRegistry>) -> SupervisorHandle`. The task body iterates over
  registry sessions; for each `idle` session with pending queue items,
  pop next, write to input_tx, mark dispatched. Listen on each session's
  `exit_rx` (via `tokio::select!` over a `FuturesUnordered<oneshot>`) to
  reap completed sessions. Shutdown cascades the cancellation token to
  every session.
- **Detail**: Use `tokio::select!` with: `token.cancelled()`,
  `interval.tick()` (250ms), and a `FuturesUnordered` of session exit
  signals. On interval tick, poll for pending queue items per idle
  session. On exit signal, update DB status + reap from registry. On
  cancellation, drop the registry and await all spawn-children's exits
  (best-effort within a timeout).
- **Acceptance**: `cargo build` + `cargo clippy` clean. End-to-end
  test (Task 16) demonstrates queued input dispatch + lifecycle.

### Phase 3: Surface integration (parallel — 3 tasks, after Phase 2)

#### 9. Implement HTTP family + WebSocket in `http/pty_sessions.rs` [M]
- **Files**: `lumina/src/http/pty_sessions.rs`, `lumina/src/http/mod.rs`,
  `lumina/src/error.rs`
- **Depends on**: 6, 7
- **Action**: Build the REST router as listed in Approach §HTTP family.
  WebSocket handler at `/api/pty/sessions/:id/ws`: validate session id,
  obtain the session's `broadcast::Receiver` + `mpsc::Sender` clones
  from the registry, `socket.split()`, spawn a sender task that forwards
  `TypedMessage` frames as JSON (handles `Lagged(n)` by sending a
  `skipped` JSON frame), spawn a receiver task that parses inbound JSON
  frames and either enqueues via the queue API (for inputs) or processes
  control frames (`ping`/`pong`, `resize`). Merge the new family router
  in `http/mod.rs`. Add `AppError::SessionNotRunning(String)` if needed
  (or reuse `NotFound`).
- **Detail**: WebSocket Origin check: read `Origin` header inside the WS upgrade handler (do NOT add tower-http's `cors` feature); reject if not in the allowlist `{http,https}://{localhost,127.0.0.1,[::1]}:<port>` plus dev-server origin if present in env. Note: Origin is browser-CSRF defence only — any local process can forge it. Trust model is the same localhost surface as the rest of lumina. Use the `axum::extract::ws::WebSocketUpgrade`
  + `on_upgrade(handler)` pattern. The two split tasks share a
  CancellationToken so either side closing tears down both. WS write
  task uses `Message::Text(serde_json::to_string(&frame)?)`.
- **Acceptance**: `cargo build` clean. `cargo nextest run` passes
  including any handler-level smoke tests using `tower::ServiceExt::oneshot`
  (REST routes) and `axum::body::Body` request bodies for input enqueue.

#### 10. Add MCP tools for PTY in `mcp.rs` [M]
- **Files**: `lumina/src/mcp.rs`
- **Depends on**: 6, 7
- **Action**: Add six `#[tool]` methods to the existing `#[tool_router]
  impl LuminaTools`: `list_pty_sessions`, `get_pty_session`,
  `spawn_pty_session`, `send_pty_input`, `cancel_pty_session`,
  `delete_pty_session`. Each takes a typed `Params` struct
  (`schemars::JsonSchema` + `serde::Deserialize`), validates inputs,
  and delegates to the corresponding `repo::pty::*` function (or
  routes spawn/cancel through the `SessionRegistry`).
- **Detail**: `spawn_pty_session` Params: `{ cwd: PathBuf, label:
  Option<String>, project_id: Option<String>, claude_args: Vec<String>,
  agent_json: Option<String>, model: Option<String>,
  env_passthrough_otel: bool }`. Validate cwd is under the worktree
  root (use `std::path::Path::strip_prefix`). `send_pty_input` Params:
  `{ session_id, kind, payload }`. Mark `list_pty_sessions` and
  `get_pty_session` with `read_only_hint = true`. Reuse only existing
  `AppError::{Validation, NotFound}` — do NOT add new error variants in
  this task; `error.rs` is Task 9's exclusive surface to avoid
  parallel-batch conflicts.
- **Acceptance**: `cargo build` + `cargo nextest run` clean. Includes extending the
  `create_tool_writes_rows_and_lists_domain_tools` enumeration at mcp.rs:2472-2538 with the 6
  new PTY tool names AND bumping the total-count assertion at mcp.rs:2549 from 55 to 61.
  `list_pty_sessions` and `get_pty_session` must appear in the read_only_hint loop at
  mcp.rs:2647-2659.

#### 11. Wire supervisor + registry into `app.rs` [S]
- **Files**: `lumina/src/app.rs`
- **Depends on**: 8, 9
- **Action**: Construct `Arc<SessionRegistry>` in `AppState`; spawn
  the supervisor via `pty::supervisor::spawn(pool.clone(),
  registry.clone())` before `axum::serve`; retain the `SupervisorHandle`
  in the same shape as `export_handle`. Add `pty_registry` field to
  `AppState`. Update graceful shutdown to await the supervisor handle's
  shutdown after the export handle's.
- **Detail**: `PtyTransport` is the default transport binding —
  attach it to the registry at construction (the registry holds a
  `Box<dyn Transport>`). Keep the transport pluggable for future
  ACP/remote implementations. Order in shutdown: HTTP server stop →
  PTY supervisor stop (cancels all children) → export drain stop.
- **Acceptance**: `cargo build` + `cargo nextest run` clean. Manual:
  start lumina, observe supervisor task in startup logs (if any),
  verify Ctrl+C cleanly exits with no orphaned child process.

### Phase 4: Frontend (sequential chain — 4 tasks 12→13→14→15, after Phase 3)

#### 12. Implement PTY API client + WS opener in `api/pty.ts` [S]
- **Files**: `lumina/web/src/api/pty.ts`, `lumina/web/src/api/index.ts`
- **Depends on**: 9
- **Action**: Define zod schemas (`PtySessionSchema`, `PtyMessageSchema`,
  `InputFrameSchema`, `WsFrameSchema` discriminated union for inbound
  WS frames). Export fetch wrappers using the existing `handle()` from
  `api/http.ts` (`listSessions`, `spawnSession`, `getSession`,
  `getMessages`, `sendInput`, `sendInputsBatch`, `cancelSession`,
  `deleteSession`, `updateSession`). Export `openSessionStream(id) →
  { send(frame), on(event, handler), close() }` — wraps a `WebSocket`,
  parses incoming JSON against `WsFrameSchema`, emits typed events.
  Re-export from `api/index.ts` barrel.
- **Detail**: WS URL: `${location.protocol === 'https:' ? 'wss:' : 'ws:'}//${location.host}/api/pty/sessions/${id}/ws`.
  Auto-reconnect on close (with backoff) if status was not user-initiated.
- **Acceptance**: `cd lumina/web && npm run build` (runs type-check + vite)
  passes. `npm test` passes if any unit test was added for schema parse.

#### 13. Implement composables for PTY sessions [M]
- **Files**: `lumina/web/src/composables/usePtySessions.ts`,
  `lumina/web/src/composables/usePtySession.ts`
- **Depends on**: 12
- **Action**: `usePtySessions()` returns module-singleton refs `sessions:
  Ref<PtySession[]>`, `status: Ref<'idle'|'loading'|'error'>`, `error:
  Ref<string|null>`; methods `loadSessions()`, `spawn(config) →
  Result<PtySession>`, `cancel(id)`, `delete(id)`. `usePtySession()`
  returns refs `messages: Ref<PtyMessage[]>`, `status: Ref<SessionStatus>`,
  `error: Ref<string|null>`, `currentId: Ref<SessionId|null>`; methods
  `select(id)`, `submit(text) → Promise<void>`, `submitBatch(texts)`,
  `cancel()`, `disconnect()`. Internally manages the WebSocket
  lifecycle via `openSessionStream`; on `select(id)` it disconnects
  the current stream, fetches history via REST, then opens the new
  stream and appends incoming `message` frames to `messages`. Cleanup
  via `onScopeDispose`.
- **Detail**: Use the request-id token pattern from
  `useHierarchy.ts:89-115` for in-flight history-load cancellation
  when the user switches sessions. Mirror the test seam pattern
  (`__setApiForTests`, `__resetForTests`) used elsewhere.
- **Acceptance**: `npm run build` + `npm test` clean. Add a bun unit test
  at `lumina/web/src/__tests__/pty-session.test.ts` (mirror the structure
  of `__tests__/readiness.test.ts`) exercising `select` → history load →
  message append.

#### 14. Implement PTY console components [M]
- **Files**: `lumina/web/src/components/PtyConsole.vue`,
  `lumina/web/src/components/PtyMessage.vue`
- **Depends on**: 13
- **Action**: `PtyConsole.vue` (Vapor SFC): subscribes to `usePtySession`
  and `usePtySessions`, renders the message list (using `PtyMessage`
  per row), a multi-line input box, send button, status pill, cancel +
  delete actions. Layout uses existing Tailwind 4 tokens + monospace font.
  `PtyMessage.vue` (Vapor SFC) takes `:message="PtyMessage"` prop and
  renders per `kind`: assistant_text as prose; tool_call as a collapsed
  summary (click to expand `content_json`); user_input prefixed `>`;
  error styled with `text-blocked` (existing theme token); parser_unknown
  falls back to `raw_text` in a monospace dim block.
- **Detail**: Use `<script setup vapor>` for both components. NO xterm.js,
  NO ANSI parsing (server already did it). Submit on Cmd/Ctrl+Enter
  (single Enter inserts newline in the input box). Auto-scroll to
  bottom on new message unless user has scrolled up (track via
  IntersectionObserver on a sentinel element at the bottom).
- **Acceptance**: `npm run build` clean. Visual smoke (manual) — spawn
  a session via UI, see messages arrive, send a prompt, see response.
  Null-project rendering: a session whose `project_id` becomes NULL (via
  `ON DELETE SET NULL` when the linked project is deleted) MUST render
  gracefully with a "(project deleted)" affordance rather than crashing
  the component.

#### 15. Wire PTY view into App.vue layout [S]
- **Files**: `lumina/web/src/App.vue`,
  `lumina/web/src/composables/useHierarchy.ts` (widen `view` union to `'focus' | 'tree' | 'pty'`),
  `lumina/web/src/components/CenterToolbar.vue` (add the third view-toggle button)
- **Depends on**: 14
- **Action**: Add a third view mode `'pty'` alongside existing `'focus'`/`'tree'`
  in the layout toggle. When active, mount `<PtyConsole />` in the
  center pane. Add a button in the existing toolbar to switch to it.
  Wire `usePtySessions().sessions` into the left spine so users can pick
  an existing session or click "+ new" to spawn.
- **Detail**: Reuse the `setView(mode)` pattern from `CenterToolbar.vue:40-54`.
  Status pill colour reflects PTY session status (idle=green, awaiting=amber,
  failed=red).
- **Acceptance**: `npm run build` clean. Manual smoke: toggle view,
  see PTY console.

### Phase 5: Tests + verification (sequential — 2 tasks, after Phase 4)

#### 16. Write integration test `tests/pty_e2e.rs` with stub binary [M]
- **Files**: `lumina/tests/pty_e2e.rs`, `lumina/tests/fixtures/pty_stub.rs`,
  `lumina/Cargo.toml` (declare `[[bin]] name = "pty_stub" path = "tests/fixtures/pty_stub.rs"` so `env!("CARGO_BIN_EXE_pty_stub")` resolves at test runtime — no `escargot` runtime build needed)
- **Depends on**: 11
- **Action**: Write a tiny Rust helper at `lumina/tests/fixtures/pty_stub.rs`
  that reads stdin lines and echoes each line back with a fake "assistant"
  prefix and a synthetic prompt line — simulates a structured claude
  conversation in plain ANSI. Resolve its path inside the test via
  `env!("CARGO_BIN_EXE_pty_stub")` (works because Task 1 added the
  `[[bin]]` manifest entry). Write an end-to-end test that: starts an
  in-process axum router + supervisor + an in-memory SQLite
  (`db::connect_in_memory`), POSTs `/api/pty/sessions` with `claude_args`
  set to point to the stub binary, opens a WebSocket via `tokio_tungstenite`
  (added to dev-deps in Task 1), submits an input frame, asserts the
  resulting message frames arrive on the WebSocket and the corresponding
  rows land in `pty_messages`.
- **Detail**: The stub binary's existence avoids requiring a real
  `claude` binary on CI. Two reads — REST history endpoint and the WS
  stream — should converge on the same data. Test must be deterministic:
  no `sleep` polling; use the actual end-of-turn signal from the parser
  (or a custom test-only `parser_strategy_version=test` that uses a
  trivial heuristic).
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml
  --test pty_e2e` passes when invoked 3× in a row by the verification
  harness with `--retries 0`, AND `grep -E
  'tokio::time::sleep|std::thread::sleep' lumina/tests/pty_e2e.rs`
  returns empty (test must be event-driven, never time-based).
  Coverage of session lifecycle: spawn → idle → input dispatched →
  assistant message persisted → cancel → completed.

#### 17. Run full verification suite + commit `.sqlx/` updates + doc refresh [M]
- **Files**: `lumina/.sqlx/*.json` (regenerated cache files),
  `lumina/CLAUDE.md` (extend `## HTTP routes` + `## MCP tool surface`),
  `CLAUDE.md` (root — tool-count reference if cited)
- **Depends on**: 16
- **Action**: Run all five verification gates from the Verification
  Commands block. If `cargo sqlx prepare --check` fails, regenerate
  via `cargo sqlx prepare --manifest-path lumina/Cargo.toml` and stage
  the resulting `.sqlx/` changes. Confirm `cargo audit --file
  lumina/Cargo.lock` shows no new advisories from `portable-pty` or
  `vt100`. Add a new subsection `### PTY sessions (\`http/pty_sessions.rs\`)`
  to `lumina/CLAUDE.md ## HTTP routes` listing the 10 new endpoints +
  the WebSocket route; add the 6 PTY MCP tools to the catalogue in
  `## MCP tool surface` (bump the round-3 tool count from 55 to 61).
- **Detail**: This is the final gate before the plan is considered
  complete. Coverage should not drop below 80% lines / 70% regions
  (the lumina LCOV threshold). The CLAUDE.md surface description is the
  authoritative catalogue future readers consult — leaving it stale
  guarantees drift; the canonical mcp tool count assertion at
  `mcp.rs:2549` is the binding contract and the doc must echo it.
- **Acceptance**: All five commands exit zero; `cargo llvm-cov` does
  not regress; `grep "PTY sessions" lumina/CLAUDE.md` returns a
  non-empty match; the MCP catalogue mentions all 6 new tool names.

## Dependency Graph

```
Batch 1 (parallel): 1, 2, 3
Batch 2a (parallel): 4, 5, 6           ; all depend on Phase 1 outputs
Batch 2b (sequential after 2a): 7      ; introduces query!/query_as! macros — regenerate `.sqlx/` cache and commit before any Phase 3 task starts
Batch 3 (sequential after Batch 2b): 8 ; integrates 4+5+6+7
Batch 4 (parallel after 8): 9, 10, 11 ; surface layer
Batch 5 (parallel after 11): 12 → 13 → 14 → 15 (chain by data dep, but
                                                12 can start as soon as 9 is in)
Batch 6 (sequential): 16, 17
```

Refined: Tasks 12, 13, 14, 15 are a sequential chain (each consumes the
previous task's exports), so the frontend batch effectively runs as
12→13→14→15 with each task picked up as its predecessor lands.
Tasks 9, 10, 11 are independent (different files) but task 11 reads
the supervisor handle whose contract is defined in task 8, so 11 starts
after 8 — 9 and 10 can start right after Phase 2 completes.

## Verification

End-to-end test plan (`/implement` Phase 3):

- **Build**: `cargo build --manifest-path lumina/Cargo.toml`
- **Backend tests**: `cargo nextest run --manifest-path lumina/Cargo.toml`
  — includes the new `pty_e2e.rs` (Task 16) and any inline unit tests
  in `pty/parser.rs`, `pty/registry.rs`, `pty/queue.rs`.
- **Lint**: `cargo clippy --manifest-path lumina/Cargo.toml --all-targets
  --no-deps`
- **SQLx offline cache**: `cd lumina && cargo sqlx prepare --check`
- **Web build**: `cd lumina/web && npm run build` (chains type-check + vite build)
- **Web tests**: `cd lumina/web && npm test` (bun runs composable unit tests)
- **Audit**: `cargo audit --file lumina/Cargo.lock` (no new advisories)
- **Coverage**: `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest
  --lcov --output-path lcov.info --fail-under-lines 80 --fail-under-regions 70`

Manual smoke (requires a local `claude` binary):
- Start lumina (`cargo run --manifest-path lumina/Cargo.toml`).
- Open the SPA, switch to the new PTY view.
- Click "+ new session", accept defaults.
- Type a short prompt, press Cmd/Ctrl+Enter.
- Observe assistant response stream in as typed messages (NO terminal
  rendering, NO ANSI codes visible).
- Submit a second prompt; observe FIFO queueing.
- Click "cancel"; observe session status transitions to `cancelled`.
- Reload the page; the session appears in the list with its full
  message history retrievable from the REST endpoint.

## Risks

- **TUI parser fragility (HIGH)** — Claude Code's interactive REPL is a
  TUI that may change rendering between releases; the `vt100` + heuristic
  pipeline could break silently. **Mitigation**: store `raw_text` on
  every parsed message as a fallback; emit unrecognised content as
  `parser_unknown` rather than dropping it; version the parser via
  `parse_strategy_version` on `pty_sessions`; document an escape hatch
  to swap to the `-p --input-format stream-json --replay-user-messages`
  long-running structured process if interactive parsing proves
  unsustainable (the Phase 5 alternative — drop in as a new `Transport`
  implementation).
- **End-of-turn detection (HIGH)** — The "idle output + prompt pattern"
  heuristic can produce false positives (sending the next input mid-
  response if the model pauses) or false negatives (a session that
  never re-prompts). **Mitigation**: configurable `idle_threshold_ms`
  on `SpawnConfig`; queue dispatcher refuses to send input when the
  parser reports "active output within last N ms"; expose a manual
  "advance queue" UI button as a backstop.
- **Windows ConPTY edge cases (MEDIUM)** — `portable-pty` supports ConPTY
  but resize-race and signal-forwarding history is rough. **Mitigation**:
  test target list includes Windows CI; fix-forward via `portable-pty`
  issue tracker; document the worst-case fallback (kill + respawn) for
  users who hit a wedge.
- **Process orphan on hard crash (MEDIUM)** — If lumina is killed -9, the
  child `claude` processes survive. **Mitigation**: best-effort
  cleanup on graceful shutdown; document that users may need to
  `pkill claude` after a hard crash; future enhancement is a
  prctl(PR_SET_PDEATHSIG) on Linux / job-object on Windows but those
  are non-portable and out of scope.
- **Broadcast lag under bursty output (MEDIUM)** — A slow WS consumer
  could fall behind a fast PTY producer; `broadcast::Lagged` would
  surface. **Mitigation**: capacity 1024 frames + UI "skipped N
  frames" indicator; user can refresh from REST history endpoint to
  recover full state.
- **SQLite write contention (LOW)** — Per-byte PTY output produces many
  small writes. **Mitigation**: parser emits `TypedMessage` only on
  finalised regions (not per byte); WAL mode + 5s busy-timeout
  already in `db::init`; batch persistence in `pty_messages` (chunk
  the message persist loop to commit every 100ms or 16 messages,
  whichever first) if profiling shows contention.
- **Cold restart loses live sessions (LOW, by design)** — A lumina
  restart kills every child `claude`. **Mitigation**: documented
  behaviour; UI shows the prior session as `completed` with full
  history readable; user can re-spawn a fresh session with the
  same `cwd` + agent config (config_json is persisted) and use
  Claude's own `~/.claude/projects/` continuity if applicable.
- **No native Rust ACP SDK / `claude-code-acp-rs` MEDIUM confidence**
  (FUTURE) — When the ACP transport slot is filled, dep choice will
  need re-evaluation. **Mitigation**: out of scope for this plan;
  noted here so the future plan re-reads Phase 3 research findings.
- **OTEL env passthrough requires user-side collector** — Setting the
  env vars without a sidecar collector running just adds harmless
  noise (claude will fail to export). **Mitigation**: config flag
  defaults to `false`; document required sidecar setup in CLAUDE.md
  when the feature is enabled.
- **Forward-only migration; feature back-out leaves orphan tables
  (MEDIUM)** — lumina's migration chain is forward-only (0001-0007
  precedent, no `_down.sql`). Once 0008 ships, reverting the feature
  leaves `pty_sessions` + `pty_messages` + `pty_queue` orphan in every
  user DB. **Mitigation**: documented behaviour; if back-out is needed,
  a follow-up migration drops the three tables (separate plan).
- **`project_id ON DELETE SET NULL` semantics (LOW)** — a deleted
  project leaves attached PTY sessions with `project_id = NULL`. The
  session itself continues to function but the UI must render the
  null gracefully. **Mitigation**: Task 14 acceptance includes
  null-project rendering ("project deleted" affordance).
- **Upstream Claude Code TTY-handling bugs (MEDIUM)** — the codebase
  has multiple open issues touching exactly the paths the supervisor
  exercises: anthropics/claude-code#12507 (stdin consumed by
  shell-detection subprocesses), #36156 (Windows hook stdin
  classified as TTY when piped), #48440 (general TTY handling). The
  Task 16 stub binary does not exercise these — only manual smoke
  against a real `claude` will. **Mitigation**: track those issues;
  if a fix is required, document a workaround in the supervisor or
  patch upstream.
- **`vt100` crate maintenance signal mixed (LOW)** — vt100 0.16.2
  (2025-10-24) is current but a community fork
  (ChrisTitusTech/vt100-rust, "fix functionality on abandoned vt100
  crate") exists, implying perceived abandonment. The 2025-10 release
  argues the original is still alive. **Mitigation**: pin to 0.16.2
  explicitly (Task 1); pre-Task-5 sanity check that
  `vt100::Screen::rows_formatted` (or equivalent) exists on 0.16.2 docs;
  swap to the fork if upstream stalls.
