<!-- This CLAUDE.md was initialised by /test-bootstrap. -->

## Workspace

`lumina/` is a standalone sibling Cargo workspace (the successor to `tomlctl`): a SQLite-canonical flow-tracking store fronted by an MCP server + axum JSON API + Vue SPA, with a git-export audit trail. Always pass `--manifest-path lumina/Cargo.toml` — that root IS the workspace, and `cargo nextest run` from it covers every member.

Four members: `core` (`lumina-core` — domain/repo/db/export/import + migrations), `server` (`lumina-server` — the `lumina` + `pty_stub` bins, http/mcp/pty/assets), `protocol` (`lumina-protocol` — the serde-only control↔execution wire types of [ADR-0006](../docs/adr/0006-git-execution-companion.md)), and `companion` (`lumina-companion` — the git EXECUTION plane; it dials `/api/companion/ws` and runs git on the server's behalf).

The dev DB `lumina/lumina.db` is gitignored and recreated on demand by `db::init` (which also runs the embedded migrations) or `sqlx migrate run`.

## Build & install gotchas

> In a flow these are the orchestrator's `verification` step, not a per-edit checklist — see **Build discipline in flows** below.

- `cargo install --path lumina/server --bin lumina` — `--bin lumina` is REQUIRED (the package ships two bins and has no `default-run`). Same for `cargo run -p lumina-server --bin lumina`.
- `cargo install --path lumina/companion` — the server's `--with-companion` co-launch resolves the companion as a SIBLING of the current executable, so install both into the same bin dir. For a debug co-launch, `cargo build --workspace` first — `cargo run` builds only the server bin — or pass `--companion-bin <PATH>`.
- `cd lumina/web && bun ci && bun run build` — build the SPA into `lumina/web/dist/`. Release builds bake that dir into the binary via `rust-embed`; debug builds serve it from the filesystem. `LUMINA_SKIP_WEB_BUILD=1` skips bun entirely for Rust-only work.
- `cargo audit --file lumina/Cargo.lock` — same cadence as tomlctl (weekly / before releases). The lockfile lives at the workspace root.

**Three compile-time purity gates. Each must report ZERO matches:**

- `rg -c 'sqlx::query(_as|_scalar)?!\(' lumina/core/src lumina/server/src` — macro-eradication. A stray bang-macro reintroduces the compile-time DB dependency the `DbClient`/`DbTx` seam removed.
- `cargo tree --manifest-path lumina/Cargo.toml -p lumina-server -e normal | rg -i '\b(git2|gix)'` — control-plane purity. The record-only server never links a git engine; ADR-0006's plane split is compile-time-enforced. (`-e normal` excludes the `companion_e2e` dev-dep, which does not link git code into the shipped bin.)
- `cargo tree --manifest-path lumina/Cargo.toml -p lumina-companion -e normal | rg 'lumina-(core|server)|sqlx|axum'` — companion isolation. The companion depends on `lumina-protocol` only; no DB or server types cross the wire boundary. It needs `git` on PATH at runtime.

## Migrations

**Never edit a migration that has already been applied or committed.** sqlx records each migration's checksum; changing the file makes every existing DB (including yours and CI's) fail to start, and the only recovery is wiping the dev DB. Add a NEW migration instead — `ls lumina/core/migrations/` for the next free number.

Related trap: `0001_init.sql` is CRLF on disk but LF in the git index, so a renormalise or a fresh clone can flip its checksum on its own. That is a line-ending divergence, not content corruption, but it forces the same dev-DB recreate.

<!-- LUMINA-SECURITY START -->
## Security: claude PTY auto-approve scope

Every PTY session lumina spawns runs `claude --permission-mode bypassPermissions --settings '{"skipDangerousModePermissionPrompt":true}'` (`pty/pty_transport.rs`). claude inside that session auto-approves **every** tool call — Bash, Read, Edit, Write, WebFetch, network — not just file edits. Rationale: claude emits permission prompts only inside its TUI, never in JSONL, so the SPA has no way to render or answer them. The v2 hardening target is a per-session `permission_mode` override on `SpawnConfig`.

**Do not expose lumina externally.** It binds `127.0.0.1` by default; on `0.0.0.0` or behind a reverse proxy, any caller hitting the HTTP API drives arbitrary tool execution on the host.

Three spawn-path details are load-bearing and each has an observable failure mode if it regresses:

- **`--settings skipDangerousModePermissionPrompt` is not optional.** Claude Code 2.1.x gates interactive `bypassPermissions` behind a one-time full-screen warning dialog that is TUI-only, so lumina cannot answer it; the first prompt's trailing `\r` would confirm its default "No, exit" and kill claude with exit code 1 the instant the session goes Awaiting. The flag feeds claude's `flagSettings` layer, scoped to the child rather than mutating `~/.claude.json`. If a release renames the key or the dialog regresses, sessions die code 1 right after the first prompt.
- **`claude` is resolved to an ABSOLUTE path by lumina** (`resolve_claude_bin`: `LUMINA_CLAUDE_BIN` override → PATH walk skipping empty/relative entries), never passed as a bare name. portable-pty rebuilds the child PATH from the registry hives and resolves bare names with a search in which an EMPTY entry (a stray `;;`) probes the server's cwd — and any checkout of this repo contains a directory named `claude`, which then shadows the real binary and fails `CreateProcessW` with `Access is denied (os error 5)`.
- **Workspace trust is pre-seeded — the ONE deliberate `~/.claude.json` mutation.** claude shows a TUI-only "Do you trust the files in this folder?" dialog on first launch in an unfamiliar directory, and lumina spawns each sprint into a FRESH worktree. There is no flag, env var, or settings key that suppresses it (`bypassPermissions` is evaluated after trust; only `-p` print mode skips it, which abandons the interactive PTY), so `pty/trust.rs` writes the cwd's `projects.<cwd>.hasTrustDialogAccepted` entry before claude reads it. It is best-effort (a write failure logs and proceeds), never-clobber (atomic temp+rename; a malformed store is left alone), and idempotent. Cleanup is MANUAL via `lumina prune-trust`, which only sweeps entries under `<repo>/.lumina/worktrees/` whose directory is gone — no teardown path runs it, and a non-worktree cwd's entry is never auto-reclaimable.

To re-verify any of this after a Claude Code bump, spawn `claude --permission-mode bypassPermissions --settings '{"skipDangerousModePermissionPrompt":true}'` under a PTY and read the startup/JSONL bytes. lumina discards the PTY stream, so a throwaway portable-pty reader probe is what reveals the dialogs, the picker layout, and the JSONL flush timing.
<!-- LUMINA-SECURITY END -->

## PTY interaction

**AskUserQuestion cannot be driven via the JSONL tail.** While an AUQ picker is open, the session JSONL contains only the user prompt — claude buffers the assistant `tool_use(AskUserQuestion)` AND its `tool_result` out of the transcript and flushes them together only after the question is answered. A JSONL-tailing consumer can therefore never observe an *open* AUQ. This is a transport-visibility gap, not a bug in the picker code; do not debug it as one.

The resolution is `mcp__lumina-ask__ask_user_question` (`pty/ask.rs`), a structured tool the spawned agent calls *instead of* the native AUQ. A per-session `--append-system-prompt` forbids the native tool and bakes in the session id; a per-session temp `.mcp.json` registers a minimal HTTP MCP server at `/mcp-ask` exposing ONLY that tool — the full work-item surface at `/mcp` is deliberately NOT exposed to spawned sessions. The tool broadcasts a synthetic `tool_use` in the same shape `jsonl_tail` produces (so the existing SPA picker renders unchanged) and blocks until the SPA POSTs an answer. Residual gap: a model that ignores the steering prompt and calls the native tool is still invisible. The verified keystroke infra (`computeAuqKeystrokes`, `POST /keystrokes`) remains in the tree but is no longer on the AUQ path.

**Prompt submission needs a separate Enter.** claude's TUI paste-detects a large single write and swallows an inline trailing CR as a soft newline. The input bridge writes the body, settles `PROMPT_SUBMIT_SETTLE_MS`, then sends the submitting Enter as its own write. Short prompts submit either way; this is what stops long prompts silently not submitting.

**Readiness is signalled by PTY output, NEVER by JSONL.** The supervisor holds a spawned session's first queued prompt until claude produces its first PTY byte plus `READY_DELAY_MS`. Dispatch any earlier and the prompt body lands before claude's readline is live: the text sits in the input box, the submitting Enter is dropped, and the session hangs at `Awaiting` forever with no JSONL. A JSONL-based gate would deadlock outright — interactive claude writes no JSONL until it processes a prompt, so the supervisor would be waiting for the record that only the undispatched prompt could produce. `first_output_at` is one-way, so it only ever delays the first prompt. Zero PTY output by `MAX_STARTUP_MS` marks the session `Failed`, which surfaces on the `pty_sessions` row for a client that re-fetches — not via a live quiescence signal or WS push. The constants are calibrated against a specific claude build; re-verify after a bump with `cargo test --manifest-path lumina/Cargo.toml --test pty_readiness_probe -- --ignored --nocapture` from a claude-trusted cwd.

<!-- TEST-BOOTSTRAP:STACK START -->
## Testing Stack (Rust crate)

**Framework**: cargo-nextest 0.9.x (runner) + rstest 0.26 + pretty_assertions 1.4 + proptest 1.9 + insta 1.47
**Coverage tool**: cargo-llvm-cov 0.8.7 (gate: 80% line, 70% region; `--fail-under-file-lines 90` approximates the 90% changed-lines target)
**Mutation tool**: (none — opt-in via --with-mutation; not in default CI)
**Showcase tests**: tests/showcase_test.rs
**CI workflow**: (none — deferred; re-run /test-bootstrap when ready to add)
**Bootstrapped**: 2026-05-25 via /test-bootstrap

### One-time installs (binaries, not crate deps)

```bash
cargo install cargo-nextest --locked      # runner
cargo install cargo-llvm-cov --locked     # coverage
cargo install cargo-insta                  # snapshot review CLI (optional but recommended)
```

### Local commands

- `cargo nextest run --manifest-path lumina/Cargo.toml` — full suite, process-per-test; `--profile ci` adds a retry + JUnit XML at `target/nextest/ci/junit.xml`
- `cargo test --workspace --manifest-path lumina/Cargo.toml` — same `#[test]` functions under rustc's built-in runner
- `cargo llvm-cov --workspace --manifest-path lumina/Cargo.toml nextest --lcov --output-path lcov.info --fail-under-lines 80 --fail-under-regions 70` — coverage with the recommended gate (swap `--html --output-dir target/coverage/html` for a report)

Scope a single member with `-p lumina-core` / `-p lumina-server` / `-p lumina-companion`. **`git` must be on PATH** for the full suite — the companion tests (`companion/tests/{shell_git,executor}.rs`, `server/tests/companion_e2e.rs`) drive a real `git` against temp repos.

`lumina/.config/nextest.toml` defines `default` (no retries), `ci` (one retry, JUnit), and `quick` — an agent inner-loop profile whose `default-filter` excludes the e2e binaries that spawn a REAL nested `claude` from PATH. A slow run of those is an instrumented rebuild, not a stall.
<!-- TEST-BOOTSTRAP:STACK END -->

## Build discipline in flows

lumina is the heaviest crate here to compile, so the repo-wide rule (root `CLAUDE.md`) bites hardest in this subtree. A sub-agent may run `cargo clippy --workspace --manifest-path lumina/Cargo.toml` (or scope with `-p`) and its task's own narrow test (`--profile quick -E 'test(<area>)'`, or `cargo test --test <name>`), and leaves the full `cargo build --workspace` + whole nextest suite to the orchestrator's single `verification` pass. N parallel sub-agents each rebuilding the crate against the shared `lumina/target` lock is exactly the waste to avoid.

## Transactions

Repo-layer mutations open a write transaction through the backend-erased seam — `db.begin()` on the `DbClient` surface, whose SQLite arm issues `BEGIN IMMEDIATE`. **Not** the default `pool.begin()`, which is `BEGIN DEFERRED`. IMMEDIATE takes the RESERVED lock at begin-time, so writer contention surfaces before any statement executes; with WAL plus the 5s `busy_timeout` set in `db::init`, concurrent MCP writes serialise without `SQLITE_BUSY` flake. `db::begin_write(pool)` is the legacy raw-pool equivalent.

The export drain in `export.rs` deliberately stays on auto-commit reads plus one auto-commit `UPDATE events SET exported_at` per event — a long read transaction there would defeat WAL's reader/writer concurrency. Do not "fix" it into a transaction.

There is no `.sqlx` offline cache and no `cargo sqlx prepare` gate: every query is a runtime `sqlx::query*` behind the `DbClient`/`DbTx` seam (the `migrate` feature survives only for the compile-time `sqlx::migrate!` in `db`).

**Post-commit notify bus** (`core/src/notify.rs` — a `tokio::sync::broadcast` behind `OnceLock`) signals derived-view consumers, notably `/api/stream`, with zero mutator edits: `record_event` calls `DbTx::note_change`, which the `NotifyingTx` wrapper BUFFERS on the in-flight tx. `commit()` runs the inner commit FIRST and only then flushes the buffer. That ordering is load-bearing — publishing pre-commit broadcasts WAL-isolated state a reader cannot yet see. It is best-effort (a zero-receiver or lagged send is ignored; the snapshot-not-delta stream self-heals) and a rollback discards the buffer, so a tx that never commits never emits a phantom signal. The `TopicResolver` seam over-approximates deliberately: a broad `interested()` predicate is absorbed by a cheap `resolve()` behind a 150 ms coalesce + dedupe-on-equal, so over-firing costs one redundant recompute, never a wrong push.

**SOLE-WRITER CAVEAT:** the bus only fires for writes made by THIS server process. An out-of-process writer on the same `lumina.db` — `lumina import-flow`, or a second server sharing the file — commits straight to SQLite and bypasses it entirely, so a live SPA shows a STALE snapshot until the client reconnects or an in-process write happens to re-fire the topic. Treat the live stream as authoritative only for single-server, in-process writes.

## Store invariants

These constrain every new tool, route, and repo function:

- **Single mutation path**: one domain write ⇒ one `work_items` row change and one `events` row, atomically or neither. Every `repo::*` mutator opens exactly one write tx and commits both together.
- **Export-inert exceptions**: bulk and audit paths deliberately record ONE coarse event whose `aggregate_type` is never `work_item` — currently `run`, `sprint`, `finding`, `batch`, `session`, `worktree`, `task_files`, `plan_epoch`. The export drain renders only `work_item` aggregates, so these are never git-exported. Batch-created and spawned items are therefore not exported individually (an accepted trade-off).
  - Consequence for `plan_epoch`: the epoch COLUMN rides the story snapshot and self-heals on the next `work_item` event, but a bump alone does not re-render it. Assert the epoch against the DB column, never against a freshly-bumped snapshot.
- **The server is RECORD-ONLY with respect to git** — it never shells out. The `execute_*` tools drive the separate `lumina-companion` process over loopback WS; the git itself runs there.
- **PTY state is not a domain entity**: `pty_sessions`, `pty_messages`, `pty_queue` sit outside the +1/+1 invariant, and the PTY surface is HTTP-only by design — it has no MCP tools, so PTY work never moves the tool count.

## MCP tool surface

The authoritative per-tool catalogue is `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`. **Two things are DERIVED — do not maintain them here and do not trust a number in prose:** the current tool count (read the count-invariant assertion in `lumina/server/src/mcp/mod.rs`) and the next free migration (`ls lumina/core/migrations/`). Both had multiple contradictory copies in this file that went stale simultaneously.

What the source does not state:

- **Deliberately non-unified vocabularies.** `Severity::{Critical, Major, Minor, Suggestion}` (findings) and `RiskSeverity::{Low, Medium, High, Critical}` (risks) are distinct enums. So are the dispatch `Tier::{Lite, Deep}` (how much agent to spend) and the interaction `GatingTier::{Full, Light, Autonomous}` (how much human to spend). Do not merge them.
- **`compute_task_batches` reads task→task edges ONLY.** It ignores `blocked_by_question_id` and `status`, loading all task children unfiltered and running Kahn's. Question-blocking is a separate mechanism, and `claim_next_task` has its OWN readiness predicate (`status='todo'` AND `blocked_by_question_id IS NULL` AND no unsatisfied dep). Never reason about claimability from `compute_task_batches`.
- **Claim guards.** A sprint's tasks are claimable only while the sprint is `status='active'`. Additionally, while ANY checkpoint task in the sprint is `in_progress`, `claim_next_task` returns `Ok(None)` — a sprint-wide barrier. `get_sprint_quiescence` mirrors both, so a frozen-but-incomplete sprint does not falsely report `done`. `Ok(None)` is not an error.
- **File overlap is advisory, never a gate** ([ADR-0002](../docs/adr/0002-sprint-execution-architecture.md)): `files_touched` is best-effort, so the claim never skips a candidate on overlap — it returns `file_overlap_warnings` computed post-commit and lets the team coordinate.
- **A Critical finding refuses `spawn_task`** (auto-rework) as `invalid_params`. It needs the operator `block` path, which creates no task, parks the host, and raises a durable open question resolved later via `resolve_open_question`.
- **`set_task_spec.files_touched` is the EXPECTED set**, written to `task_files(kind='expected')` with REPLACE semantics. The ACTUAL set is appended during execution and is never pruned; at close, reconciliation CLEARS untouched EXPECTED rows only.
- **After a merge, the operator's primary checkout is STALE.** `Outcome::Merged` carries a `target_checkout` hint because the target branch is usually checked out elsewhere; that stale checkout shows spurious "undo-the-merge" diffs, and committing there reverts the merge. The companion merges in a DETACHED integration worktree and advances the branch by `git update-ref` compare-and-swap, so a merge succeeds while the operator sits on the target branch; a lost CAS surfaces as `TargetMoved` (re-run — no rollback needed). A `Conflicted` outcome writes NOTHING to the DB and returns the paths for the caller to surface.

## HTTP routes

The axum API mirrors the MCP write surface, each handler delegating to the same single `repo::*` mutation, so the store invariants above hold identically on both. Per-family sub-routers in `lumina/server/src/http/*.rs` are mounted under `/api` by `app::build_router`.

**The route list is DERIVED, not documented here** — regenerate with `rg '\.route\(' lumina/server/src/http/`. The file names are the index: `work_items`, `repo_links`, `acceptance_criteria`, `research_notes`, `risks`, `rejected_alternatives`, `task_dependencies`, `open_questions`, `findings`, `activity`, `context_blocks`, `readiness`, `queries`, `runs`, `sprints`, `worktrees`, `execution`, `sessions`, `companion`, `structured_patches`, `settings`, `export`, `sprint_run`, `stream`, and the `pty_sessions/` submodule.

- Task-graph cycles surface as `{"error":{"kind":"cycle","message":…,"edges":[…]}}` (422) from `AppError::Cycle`.
- Structured scalar PATCHes take `{ "value": <enum> }`. The nullable three (`task-kind`, `tier`, `lane`) accept `null` as a clear signal; the rest reject it with 422.
- **Export is OPERATOR-TRIGGERED, not continuous.** The background drain loop was removed; outbox rows accumulate until a drain is requested. The transactional-outbox invariant means none are lost meanwhile.
- **`/api/stream` pushes SNAPSHOTS, never deltas.** One multiplexed read-only WebSocket carries every subscription. A dropped or `skipped` frame self-heals on the next push, and a reconnecting client just re-subscribes for fresh `init` snapshots — there is no client-side delta replay to keep consistent. Adding live coverage for a resource is one `TopicResolver` registration, not a new endpoint.

## SPA wire mirrors

`lumina/web/src/api/` hand-mirrors the Rust wire types in zod, and no flow task covers it. Rust-only verification (`LUMINA_SKIP_WEB_BUILD=1`) skips the SPA entirely, so run `bun run type-check` and `bun test` in `lumina/web/` in final verification whenever you touch a wire shape.

The coupling is strict in one direction: `wire-enums.ts` builds its schemas with `z.enum`, which HARD-REJECTS an unknown value. Adding a `domain::Status` variant without adding it to `STATUS_VALUES` makes the SPA fail to parse every response carrying it. Adding a field to a `z.object` mirror (e.g. `SprintQuiescence` in `execution.ts`) is softer — the unknown key is silently dropped — but the full-literal test snapshot helpers still need updating in the same change.

## Session corpus

Migration 0015 ([ADR-0004](../docs/adr/0004-harness-session-corpus.md)) keeps a **lossless verbatim transcript corpus** of every `claude` session lumina touches — both sessions it SPAWNS and terminal sessions INGESTED after the fact via the SessionEnd hook — one `session_records` row per non-empty JSONL line.

- **Lossless at rest; redaction is egress-only and DEFERRED.** Nothing is scrubbed on the way in. Assume raw transcript content, including whatever the session saw.
- **Asymmetric drop rule**: an INGESTED transcript with no `mcp__lumina__*` call persists NOTHING (it carries no correlation worth keeping). SPAWNED sessions are always captured.
- Re-ingest is idempotent on `UNIQUE(session_id, dedup_key)`, and the coarse `session.ingested` event is emitted only when net-new rows land — so repeated re-ingests cannot accumulate undrained outbox rows.
- `session_records.session_id` is `ON DELETE RESTRICT`. Deletes are soft today (`pty.rs` tombstones), so nothing fires; a future hard `DELETE FROM pty_sessions` with corpus rows will FAIL LOUDLY rather than cascade them away. That is intentional.
- Correlation (`sprint_id`, `agent_id`, `task_id`) is harvested from `claim_next_task` records by one shared `CorrelationAccumulator` that both the batch-ingest and live-tail paths feed, so they cannot drift. `sprint_id` is a `sprints.id` and carries NO foreign key — a hard FK would abort a lossless ingest on a deleted or cross-instance sprint.
- `POST /api/sessions/ingest` is the hook endpoint (202 + a 4-permit semaphore, unauthenticated loopback-only, `transcript_path` confined to `~/.claude/projects`). `lumina init-hooks` merges it into a project's `.claude/settings.json` idempotently.

## Story-block skills plugin

Lumina's tool surface is driven by the `/lumina:<block>` skills in `claude/plugins/lumina-story-blocks/`. The catalogue and prerequisites (server running, MCP registered as `lumina`) live in that plugin's `README.md`; the conventions it enforces are in its `CONVENTIONS.md`.

An agent drives the two chained runners (`/lumina:plan-story`, `/lumina:create-project`) by `Skill()`-dispatching each block sibling — `Skill("lumina:<block>", <id>)` — per CONVENTIONS §l.4. That is the canonical path: the `disable-model-invocation` flag that once forced inline replication was removed plugin-wide, and `scripts/verify-plan-story-blocks.sh` fails the commit if it reappears. Do not collapse or paraphrase blocks in place of dispatching them.

**Claude loads this plugin from a cache snapshot, not the repo** (`~/.claude/plugins/cache/dev-tools-local/lumina/<version>/`). After a plugin-touching change merges, copy the repo plugin over that cache and restart the session, or you will keep running the old skills.

Install: `claude plugin install --scope project ./claude/plugins/lumina-story-blocks` (persists to `.claude/settings.json`), or `claude --plugin-dir claude/plugins/lumina-story-blocks` for a one-off session.
