# Plan: lumina PTY — JSONL-tail message extraction (replaces vt100 parser)

> **Intended flow slug**: `lumina-pty-jsonl-tail` (this file will be renamed from `tranquil-forging-dove.md` to `lumina-pty-jsonl-tail.md` in Phase 9, since plan-mode forces the initial random name).
>
> **Plan path** (final): `docs/plans/lumina-pty-jsonl-tail.md`
> **Created**: 2026-05-28
> **Status**: draft

## Context

The previous lumina PTY supervisor approach (`pty/parser.rs` — vt100-backed row-finalisation transcript extractor) produces garbled output when driving the real `claude` TUI, because fullscreen TUI mode redraws the screen by absolute cursor positioning rather than streaming lines — the "finalised rows behind the cursor" heuristic cannot extract a coherent transcript from a 2D canvas application.

We are pivoting to a **JSONL-tail architecture**: keep driving `claude` interactively in the PTY (preserves subscription billing — `-p`/`--print` and the Agent SDK are explicitly out of scope; non-interactive use will land later via ACP), but read the **canonical structured transcript from the session JSONL file** that Claude Code writes to `~/.claude/projects/<sanitized-cwd>/<uuid>.jsonl` on every interactive session. This file carries typed records (`user`, `assistant` with `content` blocks for text / `tool_use` / thinking, `tool_result`, `summary`) that map cleanly to the existing `pty_messages` schema, so the web UI can render a real chat-bubble layout instead of a partially-working TUI viewport.

The user has explicitly cleared all existing `pty_sessions` / `pty_messages` / `pty_queue` rows so no backfill is required.

## Scope

**In-scope** (single concern: replace the parser with a JSONL-tail watcher):
- `lumina/src/pty/parser.rs` — **delete**
- `lumina/src/pty/spawn.rs` — rewrite the bridge task to read JSONL records
- `lumina/src/pty/pty_transport.rs` — strip parser instantiation; set `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`, `--session-id`, `--permission-mode acceptEdits` on spawn
- `lumina/src/pty/session.rs` — remove `Mutex<Parser>` field
- `lumina/src/pty/supervisor.rs` — swap parser idle check for JSONL-driven Idle signal
- `lumina/src/pty/transport.rs` — touch-up imports AND drop `prompt_pattern` from `SpawnConfig`
- `lumina/src/pty/protocol.rs`, `mod.rs` — touch-up imports
- `lumina/src/mcp.rs` — drop `prompt_pattern` from `SpawnPtySessionParams`
- `lumina/src/http/pty_sessions.rs` — drop `prompt_pattern` from `SpawnSessionBody`
- `lumina/src/pty/jsonl_tail.rs` — **new** module: JSONL watcher + record parser
- `lumina/migrations/0009_pty_jsonl_path.sql` — **new** migration: nullable `jsonl_path TEXT`
- `lumina/src/repo.rs` `pub mod pty` — extend `list_pty_sessions`, `get_pty_session` SELECT lists for the new column
- `lumina/src/domain.rs` — add `jsonl_path: Option<String>` to `PtySession`
- `lumina/.sqlx/` — regenerate offline query cache
- `lumina/tests/pty_e2e.rs` + `lumina/tests/fixtures/pty_stub.rs` — adapt to write synthetic JSONL records to a tempdir projects path
- `lumina/web/src/api/pty.ts` — extend `PtyMessage` types (no shape break — `content: z.unknown()` is already shape-agnostic)
- `lumina/web/src/components/PtyMessage.vue` — refine the per-kind chat-bubble rendering (`tool_call` / `tool_result` slots already exist at lines 89-118 — extend with `tool_use_id` pairing affordance)
- `lumina/web/src/components/PtyConsole.vue` — drop terminal-viewport framing; layout as chat transcript
- `lumina/web/src/composables/usePtySession.ts` — no shape change required (frame schema already shape-agnostic)
- `lumina/web/src/__tests__/pty-session.test.ts` — extend fixtures for new content shapes

**Out-of-scope** (deferred):
- Interactive overlay parsing (slash-command picker, permission prompts, option selectors). v1 ships text-only input; permission prompts pre-resolved via `--permission-mode acceptEdits`.
- ACP path (covered by separate future plan).
- xterm.js fallback / terminal viewport.
- `--print` / `--stream-json` mode (billing path constraint).
- Migration of pre-existing `pty_sessions` rows (none exist — DB already cleared).

**Affected areas** (for the active-flow `scope` glob): `lumina/src/pty/**`, `lumina/src/repo.rs`, `lumina/src/domain.rs`, `lumina/migrations/0009_pty_jsonl_path.sql`, `lumina/tests/pty_e2e.rs`, `lumina/tests/fixtures/pty_stub.rs`, `lumina/web/src/api/pty.ts`, `lumina/web/src/components/PtyMessage.vue`, `lumina/web/src/components/PtyConsole.vue`, `lumina/web/src/__tests__/pty-session.test.ts`.

**Estimated file count**: ~14 unique files (under the 15-file split threshold).

## Exploration Notes

### Data flow today (to be replaced)

```
claude stdout bytes
  → PtyTransport::spawn (pty_transport.rs:103)
    → reader_task (pty_transport.rs:177-210): mpsc<Bytes>
    → parser_task (pty_transport.rs:230-249): Parser::feed → broadcast<TypedMessage>
  → spawn.rs bridge_task (spawn.rs:166-244):
    - persist via repo::pty::insert_pty_message (line 188)
    - first-message gate: bridge_session.set_status(Idle) (line 220)
    - forward to registry broadcast (line 238)
  → WS handler (http/pty_sessions.rs:440-661) → FrameOut::Message JSON
```

### Parser dependencies (full delete list)

- `pty_transport.rs:75` — `use crate::pty::parser::Parser;` (remove)
- `pty_transport.rs:238` — `Parser::new()` instantiation (remove)
- `pty_transport.rs:240` — `parser.feed(&chunk)` (remove — see below for replacement)
- `session.rs:15` — `use crate::pty::parser::Parser;` (remove)
- `session.rs:26` — `parser: Mutex<Parser>` field (remove)
- `session.rs:44` — `parser: Mutex::new(Parser::new())` (remove)
- `supervisor.rs:286` — `parser.check_idle(now, IDLE_THRESHOLD)` (replace with JSONL-driven idle signal)
- `mod.rs:11` `pub mod parser;` (remove)
- `mod.rs:22` `pub use parser::Parser;` (remove)

### Idle/Awaiting state machine (state-transition call sites)

| File | Line | Transition | Trigger today | Trigger new |
|------|------|-----------|--------------|-------------|
| `spawn.rs` | 220 | Spawning → Idle | first `TypedMessage` from parser broadcast | first JSONL record OR JSONL file appears |
| `supervisor.rs` | 269 | Idle → Awaiting | after `Queue::pop_next_pending` dispatch | unchanged |
| `supervisor.rs` | 314 | Awaiting → Idle | `parser.check_idle` true | JSONL "turn-quiescent" signal (see User Decisions) |
| `supervisor.rs` | 240 | → Failed | input channel closed | unchanged |
| `http/pty_sessions.rs` | 360 | → Cancelled | DELETE handler | unchanged |

### What survives untouched

`supervisor.rs::dispatch_one`, `Queue::*`, `SessionRegistry::*`, `Session::next_sequence`, `PtyTransport::spawn`'s ConPTY workarounds (`portable-pty=0.8.1` pin + Windows slave-keep-alive in cancel task) — all unaffected.

### Web UI surface (already partially shape-ready)

- `PtyMessage.vue:89-118` already has `tool_call` and `tool_result` template slots discriminating on `message.kind`.
- WS frame schema (`api/pty.ts:138-146`) uses `content: z.unknown()` — shape-agnostic.
- `usePtySession.ts` is the **module-singleton composable** (aligned with project preference: no Pinia, no provide/inject).
- Tailwind v4 + CSS-token palette (`var(--ink)`, `var(--surface-2)`, etc.) — bubble layout reuses existing tokens.

### Migration & repo impact

- `lumina/migrations/0008_pty_sessions.sql` is the most recent migration. 0009 will be a one-statement `ALTER TABLE pty_sessions ADD COLUMN jsonl_path TEXT;` (no FK, no trigger).
- `repo::pty::list_pty_sessions` (lumina/src/repo.rs:4659-4693) and `repo::pty::get_pty_session` (4697-4728) use `sqlx::query_as!(PtySession, ...)` — extending the SELECT list requires regenerating `lumina/.sqlx/`.
- `PtySession` struct in `lumina/src/domain.rs:300-314` — append `jsonl_path: Option<String>` at end (preserve field order).
- e2e test currently uses `pty_stub` binary to emit PTY bytes; reshape so the stub writes synthetic JSONL lines into a tempdir-based projects path.
- `notify` crate is NOT in `lumina/Cargo.toml` — must add (or fall back to polling).

## Research Notes

### Claude Code session JSONL — record schema

- **Topic**: JSONL record envelope and content shape | **Source**: [GitHub Issue #53516 (schema-stability request)](https://github.com/anthropics/claude-code/issues/53516); community reverse-engineering via [samkeen gist](https://gist.github.com/samkeen/dc6a9771a78d1ecee7eb9ec1307f1b52) and [databunny Medium article](https://databunny.medium.com/inside-claude-code-the-session-file-format-and-how-to-inspect-it-b9998e66d56b) | **Evidence-grade**: B (community-reproduced; no official spec).
- **Claim**: Every record carries a shared envelope `{type, uuid, parentUuid, sessionId, timestamp, cwd, message}`. `type` ∈ `{user, assistant, system, summary, file-history-snapshot, queue-operation, ...}` with newer ephemeral types. `user.message.content` is either a string or an array of `tool_result` blocks `{type:"tool_result", tool_use_id, content, is_error}`. `assistant.message.content` is always an array of `{type:"text", text}` / `{type:"tool_use", id, name, input}` / `{type:"thinking", thinking, signature}` blocks; `message.usage` carries token counts. `summary` records carry `{summary, leafUuid}`. **No `result`-shape turn-completion sentinel exists** — end-of-turn must be inferred. Each line is a finalised, immutable event (NOT delta/incremental).
- **Impact on plan**: Tail reader parses one JSON line at a time; no streaming-delta assembly needed. `parentUuid` chain (not position) is the correct link for `tool_use` → `tool_result` correlation. Schema is UNSTABLE (Anthropic explicitly declined to commit in #53516) — build a **tolerant parser with unknown-`type` passthrough** as a `system` row with `raw_text` filled and `content_json` containing the unparsed record.

### Claude Code — schema stability status

- **Topic**: Official schema commitment | **Source**: [GitHub Issue #53516](https://github.com/anthropics/claude-code/issues/53516) | **Evidence-grade**: A (primary GitHub issue text).
- **Claim**: No stable spec exists. Anthropic has not committed to one as of May 2026.
- **Impact on plan**: Use defensive parsing. Unknown `type` MUST NOT crash the watcher; unknown fields on known types are silently ignored.

### `--session-id` in interactive mode

- **Topic**: Filename binding | **Source**: [GitHub Issue #44607](https://github.com/anthropics/claude-code/issues/44607) (closed as duplicate) | **Evidence-grade**: B (community-reproduced; spot-check verified the issue text mid-research).
- **Claim**: `claude --session-id <uuid>` in interactive (non-`-p`) mode sets only an API/telemetry id; the JSONL file is named after an internally minted UUID shown in the "Resume this session with" banner. No CLI flag or env var aligns them. `-p` mode does respect the flag for the filename.
- **Impact on plan**: We CANNOT pre-compute the JSONL filename from `--session-id`. The plan must watch the projects directory for the newest `*.jsonl` file appearing after spawn time and bind it to `pty_sessions.jsonl_path`. Single-spawn-at-a-time serialisation makes the race trivially safe; otherwise compare against a snapshot of the directory at spawn-start.

### Sanitised-cwd algorithm

- **Topic**: cwd → projects-dirname transformation | **Source**: [GitHub Issue #19972 (pseudo-code)](https://github.com/anthropics/claude-code/issues/19972); [Issue #7009 (collision examples)](https://github.com/anthropics/claude-code/issues/7009) | **Evidence-grade**: B (community-reverse-engineered).
- **Claim**: Replace each non-`[A-Za-z0-9-]` character with `-`; no consecutive-`-` collapsing; no leading-dash rule (on Windows). `C:\Users\rossa\dev\dev-tools` → `C--Users-rossa-dev-dev-tools` (matches the on-disk directory in this repo).
- **Impact on plan**: Plan can deterministically reproduce the directory name from the resolved cwd, BUT since the filename is non-deterministic (per #44607) we still need a directory-watch step. The sanitised-cwd algorithm is used only to *locate the parent directory*, not the file.

### `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`

- **Topic**: Disabling fullscreen renderer | **Source**: [Claude Code official CHANGELOG.md](https://raw.githubusercontent.com/anthropics/claude-code/refs/heads/main/CHANGELOG.md) | **Evidence-grade**: A (primary changelog; spot-check verified).
- **Claim**: Added in v2.1.132. "Added `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` env var to opt out of the fullscreen alternate-screen renderer and keep the conversation in the terminal's native scrollback." No supersession entries.
- **Impact on plan**: Set unconditionally on every spawn — even though the JSONL is the source of truth for messages, disabling alt-screen makes the PTY observable for debugging and avoids the alt-screen `CSI ?1049h` sequence muddying the WS / log channels.

### `notify` crate (Rust file-watcher)

- **Topic**: Latest stable, MSRV | **Source**: [docs.rs/notify](https://docs.rs/notify/latest/notify/) (spot-check verified 8.2.0 displayed); [GitHub releases](https://github.com/notify-rs/notify/releases) | **Evidence-grade**: A.
- **Claim**: Stable is **8.2.0** (Aug 2025; MIT/Apache-2.0). Pre-release **9.0.0-rc.4** (May 2 2026; MSRV 1.88).
- **Impact on plan**: Pin `notify = "8.2.0"` (stable, MSRV-compatible with project's 1.95). Skip the RC.

### `notify` Windows backend behaviour

- **Topic**: Event semantics | **Source**: [notify wiki — The Event Guide](https://github.com/notify-rs/notify/wiki/The-Event-Guide) | **Evidence-grade**: A.
- **Claim**: On Windows (ReadDirectoryChangesW), data writes and metadata writes both surface as `EventKind::Modify(ModifyKind::Any)` — sub-kinds are not distinguishable. Buffer overflow under high event rates causes silent batch drops, signalled via `event.need_rescan()`. For single-file conversational JSONL append rates, overflow risk is low.
- **Impact on plan**: Watcher must trigger on **any** `Modify` event for the target path (don't filter on `ModifyKind::Data`). On `need_rescan()=true`, full re-read from last-known position.

### File-creation race pattern

- **Topic**: Watching a not-yet-existent file | **Source**: notify wiki + [extended-notify crate](https://crates.io/crates/extended-notify) | **Evidence-grade**: A.
- **Claim**: Canonical pattern: `watcher.watch(parent_dir, RecursiveMode::NonRecursive)` + filter `EventKind::Create(CreateKind::File)` on the target filename. The parent dir must exist.
- **Impact on plan**: Watch `~/.claude/projects/<sanitized-cwd>/` non-recursively; on `Create(File)` for a `*.jsonl`, open + begin tailing. The parent dir is guaranteed to exist (Claude Code creates it on first spawn in any cwd).

### Pure-polling fallback

- **Topic**: Zero-dep alternative | **Source**: [tokio BufReader docs](https://docs.rs/tokio/latest/tokio/io/struct.BufReader.html); `serde_jsonlines` | **Evidence-grade**: A.
- **Claim**: `tokio::fs::File` + `AsyncBufReadExt::lines()` + a 100ms `tokio::time::interval` gives worst-case 100ms latency, zero new deps. Partial-line handling is automatic (`lines()` accumulates until `\n`).
- **Impact on plan**: Viable fallback if `notify` proves flaky on Windows. Default to `notify` (event-driven, lower latency, mature); polling is the contingency.

### `linemux` (alternative crate)

- **Topic**: Higher-level tail abstraction | **Source**: [linemux GitHub](https://github.com/jmagnuson/linemux) | **Evidence-grade**: B.
- **Claim**: Wraps `notify`; supports pre-registration of non-existent files; no substantive release since late 2022; Windows untested in own CI.
- **Impact on plan**: Skip. Unmaintained intermediary over `notify` adds risk without benefit.

## User Decisions

1. **End-of-turn detection (Awaiting → Idle)** — *Quiescence after last assistant record*. The supervisor's `maybe_finalise_turn` flips Idle when **both**: (a) every `tool_use` block emitted in the current turn has a matching `tool_result` (by `tool_use_id`), AND (b) no new JSONL line has been written for ≥ `IDLE_THRESHOLD` (750 ms). *Prompted by:* JSONL-schema research finding (no `result`-shape sentinel exists; community schema reverse-engineering, GitHub issue #53516).
2. **File-watcher implementation** — `notify = "8.2.0"` (event-driven). Watch the sanitised-cwd parent directory non-recursively; on `Create(File)` for `*.jsonl` bind that path and begin tailing. On `Modify(Any)` read-to-EOF; on `event.need_rescan()` re-seek from last known offset. *Prompted by:* notify Windows-backend research (Modify(Any) coalescing, ReadDirectoryChangesW reliability).
3. **Tool-call/tool-result UI pairing** — *Paired card*. `PtyMessage.vue` renders one unified card per `tool_use`: header `Tool: <name>`, expandable body showing input JSON, and below that the matched `tool_result` (success/error styled). Independent `tool_result` rows are suppressed in `PtyConsole.vue`'s render list. *Prompted by:* Web UI exploration (PtyMessage.vue:89-118 already discriminates on these kinds — pairing is the natural next refinement).

### Phase 5 outcome

_Skipped — Phase 4 answers introduced no library/API terms unrepresented in `## Research Notes`. All key terms (`tool_use`/`tool_result` correlation via `parentUuid`/`tool_use_id`, `notify 8.2.0` Windows behaviour, `RecursiveMode::NonRecursive` + `Create(File)` pattern) appear under the JSONL-schema and notify findings above. The paired-card UI pattern is an internal design choice, not a library lookup._

### Additional decisions made by the orchestrator (sensible defaults, can be overridden in review)

4. **`--permission-mode acceptEdits` is hard-coded on every spawn** for v1, baked into `pty_transport::spawn`'s CommandBuilder. (User specified deferred permission prompts in the invocation — making it a SpawnConfig field would be premature configuration.)
5. **Unknown JSONL record types** fall through to a `pty_messages` row with `kind = "system"`, `raw_text` containing the original line, and `content_json = {raw: <line>, type: <unknown-type>}`. Defensive parsing — schema is unstable per Anthropic's response in #53516.
6. **PTY stdout is drained-and-discarded** (no parsing of bytes). The transport's reader task still needs to drain so the child doesn't block on PTY backpressure, but the bytes are dropped on the floor — JSONL is the canonical message source.
7. **JSONL file binding** uses *snapshot-then-watch* over banner-parsing: at spawn-start, snapshot the set of `*.jsonl` files in the projects/<sanitised-cwd>/ directory (or empty set if the dir doesn't exist yet); after spawn, the first `*.jsonl` whose path was NOT in the snapshot is the bound file (path-set diff, not mtime comparison — `SystemTime` is non-monotonic and FAT/exFAT mtime granularity is 2 s). A single in-process `Mutex<()>` around the spawn-and-bind window serialises concurrent spawns in the same cwd (low cost — spawns are user-initiated, not high-throughput).

## Approach

The rewrite splits cleanly into a **schema-and-types foundation wave** that lands independently, then a **single coordinated PTY cut** that deletes the parser and rewires the transport/session/supervisor/spawn pipeline against the new `jsonl_tail` module, then **test adaptation**, then **web UI work**.

### Module shape: `lumina/src/pty/jsonl_tail.rs`

New module owns three concerns:

1. **`sanitise_cwd(p: &Path) -> String`** — pure function reproducing the Claude Code algorithm (replace each non-`[A-Za-z0-9-]` byte with `-`; no collapsing). Hand-rolled, no regex dep (consistent with `parser.rs`'s prior aversion to the `regex` crate).
2. **`JsonlRecord`** — tolerant deserialise via `#[serde(tag = "type")]` with a default `JsonlRecord::Unknown { raw: String }` variant that captures any record whose `type` isn't recognised. Known variants: `User { uuid, parentUuid, message }`, `Assistant { uuid, parentUuid, message }` (where `message.content` is `Vec<AssistantContentBlock>`), `Summary { uuid, leafUuid, summary }`. `AssistantContentBlock` has variants `Text { text }`, `ToolUse { id, name, input }`, `Thinking { thinking, signature }`. `User.message.content` is either a string or `Vec<{type:"tool_result", tool_use_id, content, is_error}>`.
3. **`tail(jsonl_path: PathBuf, tx: broadcast::Sender<JsonlRecord>) -> impl Future`** — the watcher task. Sequence: `notify::recommended_watcher` watching the parent dir non-recursively → wait for `Create(File)` matching the target filename (skip if file already exists at start) → open + seek-to-zero → BufReader → loop on (recv from event channel, lines() until `Ok(0)`, broadcast each `serde_json::from_str` result, on `need_rescan()` re-seek). Errors logged + swallowed (matches supervisor's per-session error policy).
4. **`bind_jsonl_path(cwd: &Path, spawn_started: SystemTime) -> Result<PathBuf>`** — directory-snapshot-then-poll: snapshot `*.jsonl` files at spawn-start, then poll the dir up to 5 s for the first `*.jsonl` whose path was NOT in the spawn-start snapshot (path-set diff; `spawn_started: SystemTime` is retained only for logging — not used as a filter, since wall-clock is non-monotonic). Errors as `AppError::Validation` if no file appears (the bind never blocks the spawn pipeline indefinitely).

### State-machine swap (`supervisor.rs`)

`Session` gains two new fields (replacing `parser: Mutex<Parser>`):
- `outstanding_tool_uses: Mutex<HashSet<String>>` — `tool_use.id` values seen in `assistant` records that have not yet been resolved by a matching `tool_result.tool_use_id`.
- `last_record_at: AtomicU64` — wall-clock millis of the last JSONL record observed (or 0 if none yet).

`maybe_finalise_turn` becomes:
```
fn check_idle(session, now, threshold) -> bool {
    if !session.outstanding_tool_uses.is_empty() { return false; }
    let last = session.last_record_at.load();
    if last == 0 { return false; }
    now.duration_since_epoch_millis() - last >= threshold.as_millis()
}
```

The JSONL bridge updates `outstanding_tool_uses` and `last_record_at` per record before broadcasting.

### Bridge task (`spawn.rs`)

Replaces the `transport.outbound` consumer with a `jsonl_tail::tail`-driven loop:

```
let jsonl_path = jsonl_tail::bind_jsonl_path(&cwd, spawn_started).await?;
repo::pty::set_pty_jsonl_path(pool, &session_id_str, &jsonl_path.to_string_lossy()).await?;
let (jsonl_tx, mut jsonl_rx) = broadcast::channel(1024);
tokio::spawn(jsonl_tail::tail(jsonl_path, jsonl_tx));
tokio::spawn(async move {
    while let Ok(rec) = jsonl_rx.recv().await {
        // 1. update Session.outstanding_tool_uses + last_record_at
        // 2. map JsonlRecord → Vec<TypedMessage> (one assistant record may explode into N typed messages: text + each tool_use)
        // 3. for each typed msg: persist pty_messages row; broadcast on registry_tx
        // 4. on first record: flip Spawning → Idle (preserved gate)
    }
});
```

The transport's reader task is reduced to a drain-and-discard loop.

### Tool-use ↔ tool-result correlation in `pty_messages`

`TypedMessage` (in `protocol.rs`) gains a nullable `tool_use_id: Option<String>` field. For `tool_use` rows, `tool_use_id = Some(id_from_record)`; for `tool_result` rows, `tool_use_id = Some(tool_use_id_from_record)`. Persisted into `content_json` (no schema migration needed — `content_json` is a JSON blob; the field is read by the web client). This is enough for the UI pairing.

### Spawn flags (`pty_transport.rs`)

The `CommandBuilder` gains three additions:
```rust
cmd.env("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN", "1");
cmd.arg("--session-id").arg(&session_id_str);      // telemetry alignment only
cmd.arg("--permission-mode").arg("acceptEdits");   // v1 deferral of permission prompts
```

### Test fixture reshape

`pty_stub.rs` becomes a tiny program that:
1. Reads two env vars: `PTY_STUB_PROJECTS_DIR` (tempdir-based) and `PTY_STUB_SESSION_UUID`.
2. Reads stdin (PTY input from supervisor) and acknowledges by appending one synthetic `assistant` record per received line to `<PTY_STUB_PROJECTS_DIR>/<sanitised-cwd>/<PTY_STUB_SESSION_UUID>.jsonl`.
3. The e2e test sets up the temp projects dir, sets env vars, spawns through the supervisor (the same `Transport` impl), POSTs an input, polls `/api/pty/sessions/<id>/messages` for the synthetic assistant row.

This keeps the test exercising the supervisor + bridge + DB persistence flow end-to-end. The vt100/PTY-byte concern is eliminated.

## Verification Commands

```bash
# Rust build (gates all downstream)
cargo build --manifest-path lumina/Cargo.toml

# Lint (no clippy regressions)
cargo clippy --manifest-path lumina/Cargo.toml --all-targets -- -D warnings

# Test (nextest)
cargo nextest run --manifest-path lumina/Cargo.toml

# Offline query cache (must pass after migration 0009 + repo column changes)
(cd lumina && cargo sqlx prepare --check)
# Benign warning "potentially unused queries found in .sqlx" is EXPECTED.

# Web SPA build
(cd lumina/web && npm run build)

# Web SPA tests (vitest / bun test runner per CLAUDE.md)
(cd lumina/web && bun test)
```

## Tasks

### Wave 1 (parallel — schema + types foundation)

#### T1: Add migration 0009 — `jsonl_path` column on `pty_sessions`
- **Files**: `lumina/migrations/0009_pty_jsonl_path.sql` (new)
- **Depends on**: none
- **Action**: Create a single-statement migration `ALTER TABLE pty_sessions ADD COLUMN jsonl_path TEXT;`. Match the comment-header / pragma conventions of `lumina/migrations/0008_pty_sessions.sql`. No triggers, no FK.
- **Detail**: SQLite accepts `ALTER TABLE ADD COLUMN` for a nullable TEXT without a default. The column is `NULL` until the bridge binds the discovered JSONL path after spawn.
- **Acceptance**: `sqlx migrate run` against a fresh DB succeeds; subsequent `cargo sqlx prepare --check` (after T2 lands) exits 0. If `sqlite3` CLI is available, `sqlite3 lumina.db ".schema pty_sessions"` may be used to visually confirm; not required.
- **Effort**: S

#### T2: Extend `PtySession` struct + repo SELECT lists + regenerate sqlx cache
- **Files**: `lumina/src/domain.rs`, `lumina/src/repo.rs`, `lumina/.sqlx/` (regenerated)
- **Depends on**: T1
- **Action**: Append `jsonl_path: Option<String>` to `PtySession` (after `previous_session_id`). Extend the `sqlx::query_as!` SELECT lists in `repo::pty::list_pty_sessions` (lumina/src/repo.rs:4659-4693), `repo::pty::get_pty_session` (4697-4728), AND the post-insert SELECT inside `create_pty_session` (lumina/src/repo.rs:4544-4567) to include `jsonl_path AS "jsonl_path?"`. Add a new `pub async fn set_pty_jsonl_path(pool, session_id, path) -> Result<(), AppError>` next to the other status setters. Regenerate `.sqlx/` via `cd lumina && cargo sqlx prepare -- --all-targets`. Commit the updated cache.
- **Detail**: The `as "jsonl_path?"` annotation tells sqlx the column is nullable. `set_pty_jsonl_path` updates `updated_at` in the same UPDATE statement (mirrors `update_pty_session_status`).
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; `(cd lumina && cargo sqlx prepare --check)` exits 0 (benign warning expected).
- **Effort**: M

#### T3: Extend web TS PtyMessage types for tool-use/tool-result payloads
- **Files**: `lumina/web/src/api/pty.ts`
- **Depends on**: none
- **Action**: Promote `PtyMessage.kind` to a literal-union zod schema `z.enum(['user_input', 'assistant_text', 'tool_use', 'tool_result', 'system', 'error'])`. Tighten `content_json` field shape per kind in TS types (export `AssistantTextContent`, `ToolUseContent { name, input, tool_use_id }`, `ToolResultContent { tool_use_id, output, is_error }`, etc.). The wire `WsFrameSchema` keeps `content: z.unknown()` — the union types are TS-side ergonomics.
- **Detail**: Drop dead kinds (`prompt`, `parser_unknown`) — the new pipeline never emits them.
- **Acceptance**: `(cd lumina/web && npm run build)` succeeds; existing tests still compile.
- **Effort**: S

#### T4: New `jsonl_tail` module + add `notify` dep
- **Files**: `lumina/src/pty/jsonl_tail.rs` (new), `lumina/src/pty/mod.rs` (add `pub mod jsonl_tail`), `lumina/Cargo.toml` (add `notify = "8.2.0"`)
- **Depends on**: none (independent of schema work)
- **Action**: Implement `sanitise_cwd`, `resolve_projects_root` (reads `LUMINA_PTY_PROJECTS_ROOT` env if set, else the platform default: `%USERPROFILE%\.claude\projects` on Windows, `~/.claude/projects` elsewhere), the `JsonlRecord` tolerant deserialise with `#[serde(tag = "type")]` + `Unknown` variant, the `tail()` watcher task (parent-dir watch, `Create(File)` gate, BufReader-on-File, `lines()` loop, `need_rescan` handler, broadcast emission), and `bind_jsonl_path(cwd, spawn_started)` which composes `resolve_projects_root()` + `sanitise_cwd(cwd)` + snapshot-then-poll (5 s timeout). Unit tests for `sanitise_cwd` (Windows + Unix cases), `JsonlRecord` deserialise (each known type + unknown fallback), and `bind_jsonl_path` (file-appears-after-start, never-appears-timeout).
- **Detail**: Use `tokio::sync::broadcast<JsonlRecord>` with capacity 1024 (matches existing pattern in spawn.rs:145). Errors inside the tail loop logged via `eprintln!` and swallowed (per supervisor policy).
- **Acceptance**: `cargo test --manifest-path lumina/Cargo.toml jsonl_tail` passes; `cargo clippy` clean.
- **Effort**: L

### Wave 2 (sequential — coordinated PTY cut)

#### T5: Delete `parser.rs` + rewire transport / session / supervisor / spawn / protocol against `jsonl_tail`
- **Files**: `lumina/src/pty/parser.rs` (DELETE), `lumina/src/pty/mod.rs`, `lumina/src/pty/protocol.rs`, `lumina/src/pty/transport.rs`, `lumina/src/pty/pty_transport.rs`, `lumina/src/pty/session.rs`, `lumina/src/pty/supervisor.rs`, `lumina/src/pty/spawn.rs`
- **Depends on**: T2, T4 (compiles only against the new column + jsonl_tail module)
- **Action**: One coordinated cut — delete `parser.rs`; strip `Parser` imports + instantiation everywhere; in `pty_transport.rs::spawn` set `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` env + `--session-id <uuid>` + `--permission-mode acceptEdits` args, replace the parser-bridge task with a drain-and-discard reader; replace `Session::parser` with `outstanding_tool_uses: Mutex<HashSet<String>>` + `last_record_at: AtomicU64`; replace `supervisor::maybe_finalise_turn`'s parser idle check with the new outstanding-tool-uses + quiescence logic; rewrite `spawn::spawn_pty_session_internal`'s bridge task to bind the JSONL path via `jsonl_tail::bind_jsonl_path`, persist via the new `repo::pty::set_pty_jsonl_path`, spawn `jsonl_tail::tail`, then run the JSONL→TypedMessage→pty_messages→broadcast bridge. Add `tool_use_id: Option<String>` to `TypedMessage` in `protocol.rs`. Rewrite `MessageKind` to exactly six variants: `UserInput`, `AssistantText`, `ToolUse`, `ToolResult`, `System`, `Error` (PascalCase Rust idiom — `Tool_Use` is invalid Rust). Update `MessageKind::as_wire` and `FromStr` impls at `protocol.rs:122-157` to map these six variants to wire strings `"user_input"` | `"assistant_text"` | `"tool_use"` | `"tool_result"` | `"system"` | `"error"` — these MUST match the T3 zod enum `z.enum(['user_input','assistant_text','tool_use','tool_result','system','error'])` exactly; any deviation breaks the WS frame parse on the client. Drop the prior `ToolCall`, `Prompt`, `ParserUnknown` arms from both impls. Verify by grepping `MessageKind::` across `lumina/src/` after the change and confirming every site uses one of the six new variants.
- **Detail**: This is one L-effort `flow-implement-deep` task because the changes interlock — splitting them creates intermediate compile-broken states. Treat it as a single atomic refactor; preserve every comment that documents ConPTY workarounds in `pty_transport.rs`. The first-message Spawning → Idle gate is **relocated** from `spawn.rs:218-233` (currently inside the deleted `transport_rx.recv()` loop) into the new JSONL bridge task — on first successful `jsonl_rx.recv()` of any `JsonlRecord` variant (including `Unknown`), flip `idle_flipped = true` and call `bridge_session.set_status(SessionStatus::Idle).await` + `repo::pty::update_pty_session_status(..., "idle", None)`. Preserve the `idle_flipped` local-guard pattern verbatim (never read `session.status()` to gate this — race against cancel path). The `mod.rs` edit removes `pub mod parser;` + `pub use parser::Parser;` but preserves the `pub mod jsonl_tail;` line that T4 added. `previous_session_id` column semantics unchanged (still used by cancel→spawn chaining).
- **Acceptance**: `cargo build --manifest-path lumina/Cargo.toml` succeeds; `cargo clippy --all-targets -- -D warnings` clean; `cargo nextest run` of in-module tests (in `supervisor.rs`, `spawn.rs`, etc.) passes. The `lumina/src/pty/parser.rs` file is removed from `git ls-files`. The kind-vocab comments at `domain.rs:318-320` and `web/src/api/pty.ts:73-76` are updated to the new six-variant taxonomy; the new migration `0009_pty_jsonl_path.sql` carries a header comment recording the vocab shift (the original SSOT comment in `migrations/0008_pty_sessions.sql:49` cannot be amended in place); the `spawn.rs:205-217` and `tests/pty_e2e.rs:219` comments referencing `MessageKind::Prompt` are rewritten.
- **Effort**: L

### Wave 3 (sequential — tests)

#### T6: Reshape e2e + pty_stub fixture for JSONL flow
- **Files**: `lumina/tests/fixtures/pty_stub.rs`, `lumina/tests/pty_e2e.rs`
- **Depends on**: T5
- **Action**: `pty_stub.rs` becomes: read env `PTY_STUB_PROJECTS_DIR` + `PTY_STUB_SESSION_UUID`; read stdin line-by-line; for each input line append one synthetic `{"type":"assistant","uuid":<gen>,"parentUuid":<last>,"message":{"content":[{"type":"text","text":"echo: <line>"}]}}` JSONL record to `<PROJECTS_DIR>/<sanitised-cwd>/<SESSION_UUID>.jsonl`. Update `pty_e2e.rs` to: create a tempdir for `PTY_STUB_PROJECTS_DIR`, set env vars before spawning, override the supervisor's projects-dir resolver (introduce a `LUMINA_PTY_PROJECTS_ROOT` env var in `jsonl_tail::resolve_projects_root` for tests; production reads `~/.claude/projects/` as default), assert `pty_messages` rows match the synthetic record stream, assert `pty_sessions.jsonl_path` is set after spawn.
- **Detail**: The `LUMINA_PTY_PROJECTS_ROOT` env var addition is the only production-code hook required for testability — it has precedent (`LUMINA_WORKTREE_ROOT` is read by `resolve_and_validate_cwd` in `spawn.rs`). Keep the existing ConPTY regression test (`tests/conpty_minimal_repro.rs`) untouched — it remains the PTY-spawn-only smoke test.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml --test pty_e2e` passes; the test runs green when `claude` (the real CLI) is absent — verified mechanically by `rg -n 'Command::new("claude")' lumina/tests/` returning zero hits, with all spawns routed through the `pty_stub` binary via the existing `pty_transport` override mechanism.
- **Effort**: L

### Wave 4 (parallel — web UI)

#### T7: PtyMessage + usePtySession pairing logic
- **Files**: `lumina/web/src/components/PtyMessage.vue`, `lumina/web/src/composables/usePtySession.ts`
- **Depends on**: T3
- **Action**: Delete the now-dead `v-else-if="message.kind === 'prompt'"` and `v-else-if="message.kind === 'parser_unknown'"` template branches at `PtyMessage.vue:123,145` (after T3 narrows the TS `kind` union, these branches are unreachable). In `usePtySession.ts`, add a derived `pairedMessages` computed view that walks `messages.value`, builds a `Map<tool_use_id, ToolUseContent>` index, attaches matched `tool_result` rows to their `tool_use` parent, and emits a list of `RenderableMessage` (either a standalone `assistant_text` / `user_input` / `system` row, or a `tool_use` row with an embedded `tool_result?`). Independent `tool_result` rows that DID find a parent are dropped from the render list. In `PtyMessage.vue`, refine the `tool_use` template slot to render the unified paired card: header `Tool: <name>`, expandable body with input JSON, then the matched `tool_result` block (success → muted text; `is_error` → red border). Orphaned `tool_result` (no matching `tool_use`) renders as today (independent card with a "no matched call" badge).
- **Detail**: Preserve existing tokens (`var(--accent)`, `var(--surface-2)`, `var(--border)`). Use Tailwind v4 utility classes consistent with existing PtyMessage.vue.
- **Acceptance**: `(cd lumina/web && npm run build)` clean; a new bun test (added to `src/__tests__/pty-session.test.ts` or a sibling) feeds a 3-message fixture (assistant_text, tool_use id=`x`, tool_result tool_use_id=`x`) to `usePtySession().pairedMessages` and asserts: result-list length is 2; the `tool_use` entry has `.matchedResult` populated; the standalone `tool_result` does not appear at the top level.
- **Effort**: M

#### T8: PtyConsole transcript layout (drop terminal-viewport framing)
- **Files**: `lumina/web/src/components/PtyConsole.vue`
- **Depends on**: T3
- **Action**: Replace the current terminal-viewport DOM (monospace block, auto-scroll-to-bottom, terminal frame) with a chat-transcript layout: vertical flex of `<PtyMessage>` rows over `pairedMessages` (the new derived view from T7), each row gets vertical spacing (e.g. `py-2`), assistant rows align left, user rows align right (subtle indent + accent border). Keep the auto-scroll-to-bottom behaviour. Keep the input box and statusline regions if they exist.
- **Detail**: The render source switches from `messages.value` to `usePtySession().pairedMessages` (introduced by T7). Coordinate with T7 — pairedMessages must be exported before T8 can consume it; tackle them as a tight pair within Wave 4.
- **Acceptance**: `(cd lumina/web && npm run build)` + `vue-tsc --build` clean; `rg -n 'monospace|terminal|xterm' lumina/web/src/components/PtyConsole.vue` returns zero matches (terminal-viewport CSS removed); `rg -n 'pairedMessages' lumina/web/src/components/PtyConsole.vue` returns at least one match (the new render source is wired). Live-browser inspection is a recommended follow-up but not a gating acceptance.
- **Effort**: M

### Wave 5 (sequential — web tests)

#### T9: Extend pty-session web tests for new content shapes + pairing
- **Files**: `lumina/web/src/__tests__/pty-session.test.ts`
- **Depends on**: T7, T8
- **Action**: Add fixtures for `tool_use` and `tool_result` messages (matched and orphaned). Assert: paired tool_result is dropped from the render list, orphaned tool_result is rendered. Update existing fixtures where `kind: 'tool_call'` was used to `kind: 'tool_use'`. Extend WS-frame assertions to validate the new content shapes round-trip correctly.
- **Detail**: The test harness (bun) supports the existing `makeMessage` factory shape — extend it to take a `content` arg.
- **Acceptance**: `(cd lumina/web && bun test)` passes; coverage of paired-card and orphaned-result branches.
- **Effort**: M

## Dependency Graph

```
T1 ──┐
     ├──> T2 ──┐
T3   │         │
     │         ├──> T5 ──> T6
T4 ──┘         │
               │
T3 ────────────┴──> T7 ──┐
                         ├──> T9
T3 ──────────────────> T8 ─┘
```

- **Wave 1** (parallel): T1, T2 (depends on T1 → therefore strictly after T1 completes), T3, T4. *Note: T2's dependency on T1 collapses Wave 1 to T1 alone, then T2/T3/T4 in parallel — adjust below.*
- **Wave 2** (sequential): T5 — depends on T2 + T4.
- **Wave 3** (sequential): T6 — depends on T5.
- **Wave 4** (sequential): T7 then T8 — T8 imports `pairedMessages` from `usePtySession.ts` (the symbol T7 introduces), so T8 cannot type-check until T7 lands. (Files are disjoint but the TS symbol dependency forces ordering.) If parallelism is desired, split T7 into T7a (declare `pairedMessages` signature returning `[]`) and T7b (implement the pairing) so T8 may proceed against the stub.
- **Wave 5** (sequential): T9 — depends on T7 + T8.

**Revised wave plan** (accounting for T2 → T1 strict order):
- **Wave 1a**: T1
- **Wave 1b** (parallel): T2, T3, T4
- **Wave 2**: T5
- **Wave 3**: T6
- **Wave 4** (parallel): T7, T8
- **Wave 5**: T9

## Verification

End-to-end smoke procedure (run after all waves complete):

1. **DB integrity**: `sqlite3 lumina.db ".schema pty_sessions"` shows `jsonl_path TEXT` column. `cargo sqlx prepare --check` is clean.
2. **Build + lint**: `cargo build`, `cargo clippy -- -D warnings`, `cargo nextest run` (full suite, including the reshaped `pty_e2e`).
3. **Web build + tests**: `npm run build && npm test -- --run` under `lumina/web/`.
4. **Live integration** (manual): start `lumina serve` (or however the binary is launched), open the SPA, click "Spawn session", type a short message ("hello"), confirm: (a) the assistant response appears as a chat bubble; (b) if claude invokes a tool, the tool_use renders as a paired card with the tool_result nested; (c) typing `/effort high` as plain text works (slash command via text input); (d) the input box re-enables once the model finishes (Idle flip). Cross-check `~/.claude/projects/C--Users-rossa-dev-dev-tools/<uuid>.jsonl` exists and matches the rendered transcript.
5. **Negative path**: kill `claude.exe` mid-turn; confirm `pty_sessions.status` flips to `failed` via the existing exit-reap path (`supervisor::reap_exit`); the bridge task exits cleanly when the JSONL file stops growing.

## Risks

1. **JSONL filename binding race**: spawn-and-bind window protected by a `Mutex<()>` around the bind. Risk: if multiple `claude` processes were spawned from outside lumina in the same cwd within the window, we could mis-bind. *Mitigation*: only lumina spawns into this projects dir; external usage is out-of-scope for the supervisor. If hit in practice, fall back to parsing claude's "Resume this session with `<uuid>`" banner from PTY stdout (deferred).
2. **JSONL schema instability** (Anthropic declined to commit per #53516): tolerant parser + `Unknown` variant absorbs new types as `system` rows. Risk: a future Claude Code release reshapes the `assistant.message.content` block format, breaking `Text`/`ToolUse` dispatch. *Mitigation*: schema is hand-rolled in `JsonlRecord` (one file), trivial to patch; e2e test uses synthetic records, so it won't catch real-world schema drift — periodic manual smoke is needed.
3. **Tool-use orphan on claude crash**: if `claude` exits mid-tool-call, a `tool_use` may have no `tool_result`. UI handles this (orphan badge); supervisor `Awaiting → Idle` blocks forever because `outstanding_tool_uses` is non-empty. *Mitigation*: the exit-reap path (`supervisor::reap_exit`) already flips status to `failed`/`completed` on transport exit, bypassing the idle check.
4. **Windows `notify` reliability**: ReadDirectoryChangesW buffer overflow under sustained high event rate. Single-file JSONL append is well below that rate; `need_rescan()` handler covers the edge case. *Fallback*: drop in a 100 ms polling tail watcher (already designed, just not built).
5. **`--session-id` telemetry vs filename mismatch** is documented and accepted — the snapshot-then-path-set-diff strategy handles it.
6. **`cargo sqlx prepare --check`'s benign warning** is documented in `lumina/CLAUDE.md`; flag this to reviewers so the regen step doesn't get "fixed" by dropping `--all-targets`.
7. **`--permission-mode acceptEdits` × `HOST=0.0.0.0` widens lumina to remote auto-edit/RCE**: acceptEdits auto-approves file edits plus a curated Bash set (`rm`, `mv`, `cp`, `sed`, etc.); lumina's HTTP server defaults to loopback but supports `HOST=0.0.0.0` opt-in, and the Origin allowlist only gates the WS route — `POST /api/pty/sessions` is reachable on a public bind. *Mitigation*: gate the spawn route behind an explicit env opt-in (e.g. `LUMINA_ALLOW_PTY_ON_PUBLIC_BIND=1`) when bind is non-loopback; document the posture in `lumina/CLAUDE.md` operator notes.
8. **JSONL files are unbounded; restart replays from byte 0**: the tail loop seeks-to-zero on every bridge start. For v1's cleared-DB greenfield this is acceptable, but a future operator restarting against a long-lived session will replay every record and risk duplicate `pty_messages` rows. *Mitigation*: either persist last-read byte offset alongside `jsonl_path`, or make the insert idempotent on the JSONL record `uuid` (the natural dedup key). Deferred to v2 — out of scope for the cleared-DB v1 cut.

## Rollback

This change is a single-commit cut with no feature flag. Rollback path is `git revert <T5-commit-sha>`. A phased rollout (`parser.rs` preserved behind `LUMINA_PTY_PARSER=vt100|jsonl`) was considered and rejected because the vt100 parser cannot be exercised against the real `claude` TUI in fullscreen mode (the original reason for the cut, see § Context). If post-merge testing reveals JSONL schema drift not absorbed by the `Unknown` variant, revert + hotfix the `JsonlRecord` enum in a follow-up cut. The `flow-contract-apply-rollback-protocol` skill governs `/optimise-apply` / `/review-apply` and does NOT cover plan-execution rollback — operator awareness only.
