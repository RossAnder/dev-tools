# Plan: Harness session corpus — capture, lossless store & transcript-harvest correlation (layer 2)

**Plan path**: docs/plans/harness-session-corpus.md
**Created**: 2026-06-02
**Status**: ready for `/review-plan` → `/implement` (built out via `/plan-new` 2026-06-04; 5 Open Design Questions resolved)
**Architecture**: layer 2 of [ADR-0004](../adr/0004-harness-session-corpus.md). Builds on layer 1 (`docs/plans/repo-clone-path-resolution.md`, **implemented** — migration 0014 + `resolve_cwd_to_project` are on disk). The stitch/retrieval API + dreaming seam are layer 3 (`docs/plans/corpus-stitch-and-dreaming-seam.md`); the dreaming engine is deferred.
> Last revised: 2026-06-04. Paths reflect the joyful-singing-crane refactor: `repo`/`mcp`/`domain` are submodule dirs, `pty/jsonl_tail/{mod,parse}.rs`, `http/pty_sessions/` is a submodule dir (but `http/pty_sessions.rs` per HTTP-routes doc — see T7 note).

## Objective

Capture every harness-controlled `claude` **Session** — terminal (via a `SessionEnd` **http-hook**) and SPA-spawned (existing live-tail) — into a durable, **lossless**, cross-project **Corpus** (one DB row per JSONL line, verbatim), and recover each session's `{project, sprint, agent, task}` correlation by **harvesting lumina's own `mcp__lumina__*` tool records from the ingested transcript**.

## Constraints

- **Additive, forward-only** migration; **ADD-COLUMN rule** preferred (no NOT NULL→nullable table rebuild — see Approach §1). **Runtime sqlx only** (`rg` macro gate stays 0).
- **Sessions are export-inert** — observations, not work intent. They never join the `+1 work_items / +1 events` invariant: one coarse `record_inert_event(tx, "session", …)` per ingest (mirrors the migration-0011 Part-B batch precedent; the export drain renders only `work_item` aggregates, so a `"session"` event is recorded-but-never-rendered).
- **`SessionEnd` http-hook only** — no per-turn/PreToolUse/PostToolUse hooks. The hook fires-and-forgets; lumina ingests async. Hard-close OR server-down loss is tolerated (http-hook is non-blocking, no retry).
- **Lossless at rest** — verbatim raw line per `session_records` row; `pty_messages` stays the derived render-view. **Redaction is egress-only** (layer 3) — nothing redacted at rest.
- **Reuse the PTY family** — extend `pty_sessions`, reuse `jsonl_tail` parse + `sanitise_cwd` + `resolve_cwd_to_project`; don't regress the live SPA tail.

## Scope

- **In**: `POST /api/sessions/ingest` (async ingest of `{session_id, transcript_path, cwd}` from the http-hook); `pty_sessions` extension (`source`, `sprint_id`, `agent_id`); new lossless `session_records` table; idempotent ingest keyed `(session_id, dedup_key)`; transcript-harvest correlation (parse `mcp__lumina__*` `tool_use`/`tool_result` → ids); a `get_session_context` MCP read tool (count **73→74**); make the SPA-spawn bridge also write `session_records` (uniform losslessness); a `lumina init-hooks` writer for the project http-hook; **drop-at-ingest** for terminal transcripts with no lumina calls.
- **Out**: stitch/retrieval API + dreaming seam + engine (layer 3); secret-redaction scanners (egress, layer 3); per-machine path layer; rewriting historical records; any SPA/web change (no `lumina/web/` edits).
- **Affected areas**: `lumina/migrations/` (0015), `lumina/src/domain/{enums,work_items}.rs`, `lumina/src/repo/{pty,sessions(new),events,mod}.rs`, `lumina/src/pty/jsonl_tail/{parse,mod}.rs`, `lumina/src/pty/spawn.rs`, `lumina/src/mcp/{sessions(new),mod}.rs`, `lumina/src/http/{sessions(new),mod}.rs`, `lumina/src/app.rs`, `lumina/src/{cli,main}.rs`, `lumina/tests/` (new e2e), `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/`.

## Resolved decisions

### Grilling 2026-06-02 (carried from stub)
Capture = `SessionEnd` hook (terminal) + live tail (SPA). Correlation = transcript-harvest, NOT env-injection. A "harness session" = a transcript containing lumina tool calls. Store = extend `pty_sessions` + new lossless `session_records`; `pty_messages` is the derived view. Lossless at rest; redact on egress (layer 3).

### Open Design Questions — resolved 2026-06-04 (`/plan-new` Phase 4/5)
- **Q1 — session-registration shape → add a thin `get_session_context` MCP read tool** (count 73→74). Harvest of `claim_next_task` records correlates *execution* sessions (sprint/agent in the tool_use input), but *planning/read-only* `/lumina:*` sessions emit only story/work_item ids. `get_session_context(work_item_id) → {project_id?, sprint_id?, story_id?, epic_id?}` lets those skills surface lumina-minted ids into the transcript for harvest. It is a **complement** to claim-record harvest, not a replacement (Plan-agent correction: the tool returns what lumina already knows; `claim_next_task` input stays the primary execution signal).
- **Q2 — hook delivery → native `http`-type `SessionEnd` hook (no script).** Body = the same event JSON a command-hook gets on stdin: `{session_id, transcript_path, cwd, hook_event_name, reason}` (Grade-A docs). lumina's ingest route consumes it directly. Non-blocking, no retry → best-effort delivery.
- **Q2b — hook scope → project `.claude/settings.json`** (committed, per-repo opt-in; `hooks` use override precedence). `lumina init-hooks` writes the hook entry + (when gated) an `allowedHttpHookUrls` allowlist for the localhost URL.
- **Q3 — `session_records` shape → raw TEXT + best-effort index columns + a NOT NULL `dedup_key`.** Index cols (`record_type`, `record_uuid?`, `parent_uuid?`, `ts?`, `is_sidechain?`) are best-effort (lossless raw is drift-proof — no per-type schema). `dedup_key = record_uuid` when present else `l<line_ordinal>`; `UNIQUE(session_id, dedup_key)` with `ON CONFLICT DO NOTHING`. **`line_ordinal` = 1-based index among NON-EMPTY lines**, defined identically in live-tail and ingest (the live tail skips empties before parse — diverging here would break dedup).
- **Q4 — multi-sprint harvest → last-wins scalar + lossless detail.** `pty_sessions.sprint_id`/`agent_id` take the last correlation by `line_ordinal`; every lumina call is preserved in `session_records`, so full multi-sprint detail is recoverable at layer-3 stitch.
- **Q5 — ingest concurrency → `tokio::spawn` + `Arc<Semaphore>` (4 permits) on `AppState`.** The route returns `202` immediately; the spawned task acquires a permit before touching the DB. Idempotent re-ingest makes a dropped task safe (no worker/mpsc lifecycle needed — the supervisor is a registry-keyed lifecycle loop and is the wrong tool).
- **Drop-at-ingest asymmetry**: a *terminal* transcript with no `mcp__lumina__*` call persists **nothing** (decided from one full in-memory pass before any write — the SessionEnd transcript is complete & bounded). *Spawned* sessions are **always** captured losslessly (they're lumina-orchestrated and the live tail can't know in advance whether a lumina call will come). This is an ingest-path-only policy — make it explicit.

## Research Notes

> Vetted 2026-06-04 (3 research agents). Sources: `code.claude.com/docs/en/{hooks,settings}` (Grade A); repo `pty/jsonl_tail/parse.rs` as ground truth; community RE (Grade B). `vet: Agent-1 (claude-code-hooks) — 4 sampled, 0 dropped, 1 downgraded`; `vet: Agent-2 (transcript-jsonl) — 4 sampled, 0 dropped, 0 downgraded`; `vet: Agent-3 (http-hook-spec) — 3 sampled, 0 dropped, 1 downgraded`. (`[[vet_events]]` durable append deferred — no ledger in plan mode.)

- **`http`-hook config (A)**: `{ "type":"http", "url":"<required>", "headers":{...}?, "allowedEnvVars":[...]?, "timeout":<secs> }`; POST, `Content-Type: application/json`. Verbatim doc example uses `"Authorization":"Bearer $TOKEN"` + `"allowedEnvVars":["TOKEN"]`. Optional security gates `allowedHttpHookUrls` / `httpHookAllowedEnvVars` may need the localhost URL allowlisted.
- **Request body = the command-hook stdin JSON (A)** — for SessionEnd: `{session_id, transcript_path, cwd, hook_event_name:"SessionEnd", reason}`. `transcript_path` = absolute path to the session `.jsonl`. → ingest route deserialises this exact shape; `hook_event_name`/`reason` accepted-and-ignored.
- **Non-blocking, no retry (A)**: SessionEnd "does not support blocking"; 2xx-empty = success, non-2xx/timeout/conn-fail = silent non-blocking error. → ingest is best-effort; a down lumina loses that session (accepted-loss widens from "hard-close" to "hard-close OR server-down").
- **JSONL is drift-proof for a raw store (A/B)**: no closed `type` set (`user`/`assistant`/`summary`/`system`+subtype + internal `mode`/`permission-mode`/`file-history-snapshot`/`attachment`/`ai-title` + novel types); `uuid` NOT universal (`summary.uuid` Option; `system`/`mode` key off `sessionId`). Append-only with stable ordinals; lumina's tailer **re-reads from offset 0 on Windows `need_rescan`** → consumer MUST dedup (idempotent upsert).
- **tool_use/tool_result (A — parse.rs:96-135)**: `tool_use = {type,id,name,input}`, MCP `name = mcp__<server>__<tool>`; `tool_result = {type, tool_use_id, content, is_error}` (`content` empirically a string, API permits `[{type:"text",text}]` — handle both). Richest return also in a top-level **`toolUseResult`** field (B). `LIKE 'mcp__lumina__%'` matches the work-item server and excludes `mcp__lumina-ask__*` (single hyphen).
- **Sidechain (C, conflicting)**: sub-agent (Task) records may be inline (`isSidechain:true`) and/or separate files → harvest ALL records regardless of `isSidechain`.

## Approach

1. **Schema (ADD-COLUMN only).** `pty_sessions` gains `source TEXT NOT NULL DEFAULT 'spawned' CHECK(source IN ('spawned','ingested'))` (existing rows ARE spawned), `sprint_id TEXT REFERENCES work_items(id)` (nullable), `agent_id TEXT` (plain TEXT — agents aren't work_items). Ingested rows have no `SpawnConfig`, so they store a **sentinel** `config_json='{}'`, `status='completed'` (the session is over by SessionEnd), `started_at/updated_at = ingest now()`, `cwd =` the raw hook value (lexical-only, never `resolve_and_validate_cwd`). This avoids a NOT-NULL→nullable table rebuild and keeps the migration pure ADD-COLUMN. The `pty_sessions_project_kind_check` trigger fires on the ingested INSERT — resolve `project_id` via `resolve_cwd_to_project` FIRST (it returns a real, non-tombstoned `kind='project'` id) or leave NULL.
2. **Lossless raw capture (minimal blast radius).** Thread `raw: String` onto `JsonlRecordParsed` for **all** variants (today only `UnknownRaw` keeps it) — `drain_and_broadcast` already has the line in scope, zero new IO. The live-tail writes `session_records` via a **separate broadcast consumer** (not inline in the dense `spawn.rs` bridge), so a corpus-write failure can never stall message persistence; ordering is by `line_ordinal`, not insert order.
3. **Async ingest.** `AppState` gains `session_ingest_sem: Arc<Semaphore>` (4 permits). `POST /api/sessions/ingest` returns `202` immediately and `tokio::spawn`s the ingest, which acquires a permit before DB work. Re-ingest idempotency makes dropped/abandoned tasks safe.
4. **Harvest.** One full in-memory pass over parsed records: if no `tool_use.name LIKE 'mcp__lumina__%'` → drop (persist nothing). Else, in one write txn: upsert the `pty_sessions` row (ON CONFLICT DO NOTHING — never clobber an existing `spawned` row), bulk-insert `session_records` (ON CONFLICT DO NOTHING on `(session_id, dedup_key)`), and one coarse inert event. Correlation: sprint/agent last-wins from `claim_next_task` tool_use **input** (matched to its result by `tool_use_id`); task from the `claim_next_task`/`complete_task` timeline; project floor from `resolve_cwd_to_project(cwd)`; `get_session_context` results as an additional signal for planning sessions.
5. **Tool + hook + docs.** `get_session_context` is a read-only MCP tool composing existing ancestry/sprint reads (count 73→74 + count-test + name-catalogue). `lumina init-hooks` writes the http-hook block into project `.claude/settings.json`. Docs + the `/lumina:*` entrypoint skills call `get_session_context` at session start.

## Verification Commands

```
build: cargo build --manifest-path lumina/Cargo.toml
test:  cargo nextest run --manifest-path lumina/Cargo.toml -E 'not (binary(pty_e2e) | binary(conpty_minimal_repro))'
lint:  cargo clippy --manifest-path lumina/Cargo.toml --all-targets
```
Also gating: `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` = **0**; `mcp` count-invariant test asserts **74**. No SPA build (no `lumina/web/` change).

## Tasks

### Phase 1: Schema & domain

#### T1: Migration 0015 — `session_records` table + `pty_sessions` correlation columns
- **Files**: `lumina/migrations/0015_session_corpus.sql` (new)
- **Action**: Three ADD-COLUMNs on `pty_sessions`: `source TEXT NOT NULL DEFAULT 'spawned' CHECK (source IN ('spawned','ingested'))`, `sprint_id TEXT REFERENCES work_items(id)` (nullable), `agent_id TEXT` (nullable). New `session_records` table: `id TEXT PRIMARY KEY` (uuidv7), `session_id TEXT NOT NULL REFERENCES pty_sessions(id) ON DELETE CASCADE`, `line_ordinal INTEGER NOT NULL`, `record_type TEXT`, `record_uuid TEXT`, `parent_uuid TEXT`, `ts TEXT`, `is_sidechain INTEGER NOT NULL DEFAULT 0`, `raw TEXT NOT NULL`, `dedup_key TEXT NOT NULL`, `created_at TEXT NOT NULL`, `UNIQUE(session_id, dedup_key)`. Indexes: `(session_id, line_ordinal)`, `(session_id, record_type)` (harvest), `(record_uuid) WHERE record_uuid IS NOT NULL`. Forward-only header comment (no down-migration); note the `source` default backfills existing spawned rows.
- **Acceptance**: migration applies on top of 0014 against a fresh DB; `cargo nextest run` migration test green; `pty_sessions` has the 3 new columns and `session_records` exists with the UNIQUE. 0015 is the next free number (latest on disk = `0014_repo_local_path.sql`).
- **Blocked-by**: none

#### T2: Domain types — `SessionSource` enum + `SessionRecord` struct + `PtySession` fields
- **Files**: `lumina/src/domain/enums.rs`, `lumina/src/domain/work_items.rs`
- **Action**: In `enums.rs`, add `#[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize,JsonSchema)] #[serde(rename_all="snake_case")] pub enum SessionSource { Spawned, Ingested }` (wire `spawned|ingested`, matching the SQL CHECK). In `work_items.rs`, add to `PtySession`: `pub source: String` (mirrors `status: String`), `pub sprint_id: Option<String>`, `pub agent_id: Option<String>` (the two Options carry `#[serde(skip_serializing_if = "Option::is_none")]`). Add a new `pub struct SessionRecord` mirroring the table columns (Options carry `skip_serializing_if`). `domain/mod.rs` already `pub use`s both submodules — no re-export edit.
- **Acceptance**: `cargo build` clean; `SessionSource` + `SessionRecord` reachable at `crate::domain::*`; `PtySession` carries the 3 fields.
- **Blocked-by**: T1

#### T3: `PtySession` FromRow + the `SELECT … FROM pty_sessions` audit + `SessionRecord` FromRow
- **Files**: `lumina/src/repo/pty.rs`
- **Action**: Extend the hand-written `PtySession` FromRow (pty.rs:28-56) with `source`/`sprint_id`/`agent_id` `try_get`s. **Runtime-sqlx footgun (the single most likely prod-panic): audit EVERY `SELECT … FROM pty_sessions` that maps to `PtySession`** (`create_pty_session` read-back, `get_pty_session`, `list_pty_sessions`, any other) and add the 3 new columns to each SELECT — a missing column makes `try_get` panic at runtime with no compile-time check. Add a `SessionRecord` FromRow (mirror the PtySession recipe; restate the `Option<String>: Decode+Type` where-bound).
- **Acceptance**: `cargo build` clean; `rg 'FROM pty_sessions' lumina/src/repo/pty.rs` — every PtySession-mapping SELECT lists `source, sprint_id, agent_id`; existing `get`/`list_pty_sessions` round-trip unchanged (spawned rows read `source='spawned'`).
- **Blocked-by**: T2

#### T4: Lossless parse plumbing — raw line on all variants + index-field extraction
- **Files**: `lumina/src/pty/jsonl_tail/parse.rs`, `lumina/src/pty/jsonl_tail/mod.rs`
- **Action**: Add `raw: String` to `JsonlRecordParsed` for BOTH `Known` and `UnknownRaw` (or lift to a struct `{ raw, record }`) — populate it from the line already in scope at `parse_line`. Add `pub fn record_index_fields(p: &JsonlRecordParsed) -> SessionRecordIndex { record_type, record_uuid, parent_uuid, ts, is_sidechain }` (best-effort, all Option/default). In `mod.rs::drain_and_broadcast`, **pin the `line_ordinal` contract**: 1-based among NON-EMPTY lines (it already skips empties before parse) — document it as the shared definition the ingest path MUST replicate. No behaviour change to existing `map_record_to_typed` consumers (they ignore `.raw`).
- **Acceptance**: `cargo build` clean; existing `jsonl_tail` tests green; `JsonlRecordParsed::Known` now carries the raw line; `record_index_fields` returns `record_type` for User/Assistant/Summary/System and `None` uuid for a `mode`/`ai-title` fixture.
- **Blocked-by**: none _(independent of schema — parallel with T1)_

### Phase 2: Persistence & correlation (repo layer)

#### T5: `repo/sessions.rs` — persistence helpers + inert-event vocab
- **Files**: `lumina/src/repo/sessions.rs` (new), `lumina/src/repo/mod.rs` (`pub mod sessions; pub use sessions::*;`), `lumina/src/repo/events.rs` (docstring only)
- **Action**: `insert_session_record(tx, session_id, ordinal, raw, index, dedup_key)` — INSERT … `ON CONFLICT(session_id, dedup_key) DO NOTHING`. `upsert_session_row(tx, id, source, cwd, project_id?, sprint_id?, agent_id?, started_at, ended_at?)` — INSERT pty_sessions with the sentinel `config_json='{}'` for `source='ingested'`, `status='completed'`, `parse_strategy_version=1`; `ON CONFLICT(id) DO NOTHING` (never clobber an existing spawned row, but the caller may still backfill its `session_records`). Centralise the one coarse `record_inert_event(tx, "session", session_id, "session.ingested", payload)`. Update the `record_inert_event` docstring (events.rs:56-69) to add `session` to the inert vocabulary (run/sprint/finding/batch/**session**) — the guard already passes it (only `"work_item"` is rejected) and the export drain never renders it.
- **Acceptance**: `cargo build` clean; `cargo clippy` clean; unit test (co-located) that a duplicate `(session_id, dedup_key)` insert is a no-op and that `upsert_session_row` writes one `session`-typed `events` row.
- **Blocked-by**: T3, T4

#### T6: `repo/sessions.rs` — harvest + ingest routine
- **Files**: `lumina/src/repo/sessions.rs`
- **Action**: `harvest_correlation(records: &[(ordinal, JsonlRecordParsed)]) -> Correlation { has_lumina, sprint_id?, agent_id?, task_id? }` — scan for `tool_use.name LIKE 'mcp__lumina__%'` (sets `has_lumina`); pair `claim_next_task` tool_use input (sprint_id, agent_id) with its result by `tool_use_id`; last-wins by ordinal; task from the `claim_next_task`-result/`complete_task`-input timeline; also read `get_session_context` results. `ingest_transcript(db, session_id, transcript_path, cwd) -> Result<IngestOutcome>` — read the file, split into non-empty lines (ordinal contract per T4), parse each, `harvest_correlation`; **if `!has_lumina` return `Dropped` (persist nothing)**; else `resolve_cwd_to_project(db, cwd)` for the project floor, then one write txn: `upsert_session_row` + bulk `insert_session_record` + coarse inert event. Idempotent on re-call.
- **Acceptance**: `cargo build`/`clippy` clean; co-located unit tests: an inline transcript with a `claim_next_task` pair yields `{sprint_id, agent_id, task_id}`; a transcript with no `mcp__lumina__` call yields `has_lumina=false` and `ingest_transcript` → `Dropped`; re-calling `ingest_transcript` inserts 0 new `session_records`.
- **Blocked-by**: T5

### Phase 3: Transports & tool (parallel after deps)

#### T7: SPA-spawn bridge writes `session_records` (uniform losslessness)
- **Files**: `lumina/src/pty/spawn.rs`
- **Action**: Subscribe a **second, lightweight broadcast consumer** in `spawn_pty_session_internal` that, for each parsed record, calls `insert_session_record` with the raw line (T4) + `record_index_fields` + `dedup_key` (uuid-or-`l<ordinal>`). Spawned sessions are **always** captured (no drop-gate — they're lumina-orchestrated); set `source='spawned'` (the migration default already covers the create path, but pass it explicitly if `create_pty_session` is touched). Keep the existing bridge/message-persistence path untouched.
- **Acceptance**: `cargo build`/`clippy` clean; existing pty tests green; a spawned session's tailed lines land as `session_records` rows with `source='spawned'`, deduped on `need_rescan` re-read.
- **Blocked-by**: T5 (the insert helper), T4 _(parallel with T6)_

#### T8: `get_session_context` MCP read tool (count 73→74)
- **Files**: `lumina/src/mcp/sessions.rs` (new), `lumina/src/mcp/mod.rs`
- **Action**: New family `#[tool_router(router = tool_router_sessions, vis = "pub(crate)")] impl LuminaTools { … }` with ONE read tool `get_session_context(work_item_id: String) -> CallToolResult` returning `{ project_id?, sprint_id?, story_id?, epic_id? }` — composes existing ancestry reads (walk to `kind='project'`) + `sprint_tasks` membership; read-only (`open_world_hint=false`), no migration, no write. Thread `+ Self::tool_router_sessions()` into the `with_state` router-sum and **bump the count-invariant assertion 73→74** plus any tool-name-catalogue/annotations test in `mcp/mod.rs`.
- **Acceptance**: `cargo build` clean; `cargo nextest run` — the `mcp` count test asserts **74** and passes; `get_session_context` returns the project/sprint/story ids for a seeded story under a sprint.
- **Blocked-by**: T2

#### T9: `POST /api/sessions/ingest` route + AppState semaphore
- **Files**: `lumina/src/http/sessions.rs` (new), `lumina/src/http/mod.rs`, `lumina/src/app.rs`
- **Action**: In `app.rs`, add `session_ingest_sem: Arc<Semaphore>` to `AppState` (e.g. `Semaphore::new(4)`), constructed alongside the existing Arc fields. New `http/sessions.rs`: `router()` with `POST /sessions/ingest` (relative to the `/api` nest), body `IngestBody { session_id, transcript_path, cwd, hook_event_name: Option<String>, reason: Option<String> }` (the latter two accepted-and-ignored). Handler returns `202 Accepted` (empty body) immediately, then `tokio::spawn`s a task that `let _permit = state.session_ingest_sem.clone().acquire_owned().await` then calls `repo::ingest_transcript`. Mount via `pub mod sessions; … .merge(sessions::router())` in `http/mod.rs`.
- **Acceptance**: `cargo build`/`clippy` clean; `POST /api/sessions/ingest` with a valid body returns 202; a follow-up `GET` (via the e2e) shows the ingested session; a down/garbage `transcript_path` does not 500 the response (best-effort task logs and exits).
- **Blocked-by**: T6

### Phase 4: Hook, tests, docs

#### T10: `lumina init-hooks` — project http-hook writer
- **Files**: `lumina/src/cli.rs`, `lumina/src/main.rs`
- **Action**: Add an `init-hooks` subcommand that merges a `SessionEnd` `http`-hook entry into the target project's `.claude/settings.json` (default `./.claude/settings.json`; never clobber existing `hooks` — read-modify-merge): `{ "type":"http", "url":"http://127.0.0.1:<port>/api/sessions/ingest", "timeout":30 }` under `hooks.SessionEnd[].hooks[]`. When the `allowedHttpHookUrls` gate is present, append the localhost URL. Port resolves from lumina's bind config (or a `--url` flag override). Print the written block.
- **Acceptance**: running `lumina init-hooks` against a temp dir writes a valid `.claude/settings.json` containing the SessionEnd http-hook; re-running is idempotent (no duplicate entry); `cargo build`/`clippy` clean.
- **Blocked-by**: T9

#### T11: e2e + unit coverage
- **Files**: `lumina/tests/sessions_e2e.rs` (new); co-located unit tests already in T4/T6/T7
- **Action**: In-process e2e (mirror `tests/e2e.rs`: shared in-memory `AnyPool`, no socket): construct an **inline fixture transcript string** (several JSONL lines incl. an `assistant` `tool_use(mcp__lumina__claim_next_task)` + the paired `user` `tool_result`, plus a `summary` and a `mode` line), write it to a tempfile, `POST /api/sessions/ingest` (tower oneshot) OR call `ingest_transcript` directly; assert: lossless `session_records` (one row per non-empty line, raw verbatim), derived correlation scalars on `pty_sessions` (sprint/agent/task), `GET /api/work-items/{project}` / a session read reflects it; **idempotent re-ingest** (re-POST → 0 new rows); **non-lumina transcript dropped** (no `pty_sessions`/`session_records` rows). Add a spawned-path assertion that the live-tail consumer writes `session_records`.
- **Acceptance**: `cargo nextest run --manifest-path lumina/Cargo.toml -E 'not (binary(pty_e2e) | binary(conpty_minimal_repro))'` green incl. the new e2e; `cargo llvm-cov … --fail-under-lines 80 --fail-under-regions 70` passes; new files meet `--fail-under-file-lines 90`.
- **Blocked-by**: T6, T7, T8, T9

#### T12: Docs + plugin skills
- **Files**: `lumina/CLAUDE.md`, `CLAUDE.md` (root), `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` (+ the `/lumina:*` entrypoint skills that should call `get_session_context`, e.g. `plan-story`, `next-block` — representative, not exhaustive)
- **Action**: `lumina/CLAUDE.md` — new § *Session corpus*: `session_records` table, `pty_sessions` `source`/`sprint_id`/`agent_id`, the `record_inert_event` `session` vocab extension, `POST /api/sessions/ingest` (in § HTTP routes), `get_session_context` (count **73→74** — update the count claim everywhere it says 73), the http-hook + `lumina init-hooks`, drop-at-ingest, lossless-at-rest/redact-on-egress-deferred. Root `CLAUDE.md` — note layer-2 landed + tool count 74. `SKILL.md` — document `get_session_context` and the "call at session start to stamp correlation" convention; wire it into the named entrypoint skills.
- **Acceptance**: `rg 'session_records|get_session_context|/api/sessions/ingest' lumina/CLAUDE.md` shows the entries; the "73 tools" claim is updated to 74 wherever it appears.
- **Blocked-by**: T8, T9, T10

## Dependency Graph

```
T1 ─→ T2 ─→ T3 ─┐                ┌─→ T7 ───────────┐
                ├─→ T5 ─→ T6 ─┬──┤                 ├─→ T11
T4 ─────────────┘             │  └─→ T9 ─→ T10 ────┤
                              │                     │
T2 ─→ T8 ─────────────────────┴─────────────────────┴─→ T12
```
- **Level 0 (parallel)**: T1, T4
- **Then**: T2 → T3; T8 branches off T2
- **Then**: T5 → T6; T7 branches off T5 (parallel with T6); T9 after T6 → T10
- **Convergence (serialise edits)**: `mcp/mod.rs` (count+router — T8 only), `http/mod.rs`+`app.rs` (T9 only), `repo/mod.rs` (T5 only), `domain/mod.rs` (none — already re-exports). No two parallel tasks share a file.

## Verification

- `cargo build --manifest-path lumina/Cargo.toml`
- `cargo nextest run --manifest-path lumina/Cargo.toml -E 'not (binary(pty_e2e) | binary(conpty_minimal_repro))'`
- `cargo clippy --manifest-path lumina/Cargo.toml --all-targets`
- `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/src lumina/tests` = **0**
- `mcp` count-invariant test asserts **74**
- e2e: inline terminal transcript ingests losslessly, correlates by harvest, re-ingest idempotent, non-lumina dropped, spawned captured
- Manual smoke: `lumina init-hooks` in a project, run a `/lumina:*` command in a terminal, close it, confirm the session + records + correlation appear

## Risks

- **Runtime-sqlx column footgun (T3)** — the highest-likelihood "passes review, panics in prod" defect: every `SELECT … FROM pty_sessions`→`PtySession` must list the 3 new columns or `try_get` panics at runtime (no compile check). T3's acceptance pins the audit.
- **Ordinal divergence (T4)** — live-tail and ingest MUST assign `line_ordinal` identically (1-based among non-empty lines); a mismatch breaks `(session_id, dedup_key)` dedup for uuid-less rows. Pin and test the contract.
- **`pty_sessions` project-kind trigger on ingested INSERT** — `project_id`, when set, must reference `kind='project'`; resolve via `resolve_cwd_to_project` (which only yields real, non-tombstoned projects) FIRST, or insert NULL. Easy to forget on the new write path.
- **`record_inert_event` "session" vocab extension** — passes the guard (only `"work_item"` is rejected) and is never exported, but the docstring lists run/sprint/finding/batch; update it (T5) or a reviewer reads `"session"` as unhandled.
- **Drop-at-ingest vs lossless asymmetry** — ingest-path-only (terminal sessions drop-if-no-lumina); spawned sessions are always captured. Document explicitly (ADR + CLAUDE.md) or it reads as a contradiction.
- **Hard-close OR server-down loss (accepted, widened)** — http-hook is non-blocking with no retry, so a down lumina silently drops that session in addition to the ADR's hard-close case. Document; re-POST/`need_rescan` recovers if the file is still present.
- **JSONL schema drift** — raw store is drift-proof by construction (`UnknownRaw` path); index columns are best-effort. No per-type schema to maintain.
- **Harvest misattribution (multi-sprint)** — last-wins scalar by `line_ordinal` + lossless detail. A genuinely multi-sprint orchestrator session keeps full detail in `session_records` for layer-3 stitch.
- **Volume** — keep-forever v1; drop-at-ingest for non-lumina terminal sessions limits growth; deferred prune knob (ADR). Spawned sessions are always captured (bounded by SPA usage).
- **http-hook soft version dependency** — standard documented type, no min version located; don't pin a version. `allowedHttpHookUrls`/managed-settings layers may require operator allowlisting (T10 writes it when present).
- **Plan size (~20 files across 12 tasks)** — above the ~15-file guard, but one cohesive feature (an ADR layer) with a clean dependency chain and no parallel batch exceeding 6 files. Optional: implement Phases 1-3 (store + capture) and Phase 4 (correlation/tool/hook/docs) as two `/implement` passes.
- **Shared-shell serialisation (post-refactor)** — `mcp/mod.rs` (count+router), `http/mod.rs`+`app.rs`, `repo/mod.rs` each edited by exactly one task; no two parallel tasks converge on a shell.
