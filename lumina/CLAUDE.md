<!-- This CLAUDE.md was initialised by /test-bootstrap. -->

<!-- LUMINA-SECURITY START -->
## Security: claude PTY auto-approve scope

Every PTY session lumina spawns runs `claude --permission-mode bypassPermissions --settings '{"skipDangerousModePermissionPrompt":true}'` (set in `lumina/src/pty/pty_transport.rs`). claude inside that session auto-approves every tool call: Bash, Read, Edit, Write, WebFetch, network access, glob/grep — NOT just file edits as the prior `acceptEdits` baseline.

Rationale: claude emits permission prompts only inside its TUI, never in JSONL; the SPA has no way to render or answer them. v1 ships with auto-approve; the v2 hardening target is exposing a per-session `permission_mode` override on `SpawnConfig`.

The `--settings skipDangerousModePermissionPrompt` flag is load-bearing, not optional. Claude Code 2.1.x gates interactive `bypassPermissions` behind a one-time full-screen warning dialog (`BypassPermissionsModeDialog`, default selection "No, exit") that is TUI-only — never surfaced over JSONL — so lumina cannot answer it; the first prompt's trailing `\r` would otherwise confirm "No, exit" and kill claude with exit code 1 the instant the session goes Awaiting. The flag feeds claude's `flagSettings` layer so its acceptance gate (`kp()`) returns true and bypassPermissions applies with no dialog — equivalent to clicking "Yes, I accept", but scoped to the spawned child rather than mutating `~/.claude.json`. A Claude Code update that resets the stored acceptance (as 2.1.156 did) is therefore a no-op for lumina. If a future Claude Code release renames the settings key or the dialog regresses, sessions die code 1 right after the first prompt — re-derive the current gate by spawning `claude --permission-mode bypassPermissions` under a PTY and reading the startup bytes (lumina discards them; a throwaway reader probe reveals the dialog).

Interaction risk: lumina binds to `127.0.0.1` by default. If a deployment binds to `0.0.0.0` (or sits behind a reverse proxy reachable from a hostile network), any caller hitting the HTTP API can drive arbitrary tool execution on the host — file writes, shell commands, network egress. Do not expose lumina externally without a permission wrapper.
<!-- LUMINA-SECURITY END -->

## PTY interaction: AskUserQuestion + prompt submission

**AskUserQuestion (AUQ) cannot be driven via the JSONL tail.** Verified against claude 2.1.156: while an AUQ picker is open and waiting, the session JSONL contains only the user prompt — claude buffers the assistant `tool_use(AskUserQuestion)` AND its `tool_result` out of the transcript and flushes them together only *after* the question is answered. So a JSONL-tailing consumer (lumina's bridge) can never observe an *open* AUQ, and the SPA's `pendingAuq` (an *unmatched* AUQ tool_use) can never fire. This is why the `lumina-interactive-prompts` picker "never came through" — not a bug in the picker/`computeAuqKeystrokes`/`/keystrokes` code (all verified correct; `down`+`enter` selecting option 2 was confirmed), but a transport-visibility gap.

**Resolution — the `ask_user_question` MCP tool (lumina/src/pty/ask.rs).** Rather than screen-scrape the native picker out of the PTY byte stream (fragile — geometry/wrapping/version-drift; and the TUI reveals multi-question AUQs one screen at a time, so full fidelity is unreachable that way), lumina gives the spawned agent a STRUCTURED tool it calls *instead of* the native AUQ:

- **Steering.** `pty_transport.rs` appends a per-session system prompt (`no_auq_system_prompt(session_id)`) via `--append-system-prompt` that forbids the native AskUserQuestion tool and directs claude to call `mcp__lumina-ask__ask_user_question` with the session id (baked into the prompt — that argument is how the tool correlates the call back to this PTY session).
- **Registration.** The spawn writes a per-session temp `.mcp.json` and passes `--mcp-config <path>` — Claude Code 2.1.x accepts only a FILE PATH there (not inline JSON), and the flag MERGES with the project's configured servers. It registers a single HTTP MCP server `lumina-ask` at `http://127.0.0.1:<PORT>/mcp-ask` carrying a `timeout` field (the per-tool-call limit lives in the mcp-config entry; 2.1.x has no `MCP_TOOL_TIMEOUT` env var). The temp file is removed when the child exits.
- **Server.** `ask.rs` is the `/mcp-ask` mount (`app::build_router`), a deliberately minimal MCP server exposing ONLY `ask_user_question` — the 58-tool work-item surface at `/mcp` is **not** exposed to spawned sessions. The tool resolves the session by `session_id`, registers a per-question `oneshot` in `Session::pending_questions`, marks the session non-quiescent (adds the synthetic question id to `outstanding_tool_uses` so the supervisor won't flip it Idle while the operator decides), broadcasts a synthetic `tool_use(AskUserQuestion)` (the SAME shape `jsonl_tail::map_record_to_typed` produces, so the EXISTING `PtyAuqPicker` SPA renders it unchanged), and BLOCKS on the oneshot (30-min cap; on timeout it closes the picker and returns a "no answer" result).
- **Answer.** The SPA POSTs to `POST /api/pty/sessions/{id}/ask/{question_id}/answer` (`http/pty_sessions.rs::answer_question`), which fulfils the oneshot, clears the bookkeeping, and broadcasts a synthetic `tool_result` closing the picker card with the answer. The tool then returns the selections to the agent. `usePtySession`'s `submitAuqAnswer`/`cancelAuq` POST here (NOT the keystroke path). The synthetic emit + persist + broadcast for all three producers (JSONL bridge, ask tool, answer endpoint) go through one helper, `pty::emit::persist_and_broadcast`.

Residual gap: a model that calls the *native* AskUserQuestion tool anyway (ignoring the steering prompt) is still invisible to the JSONL tail — same limitation as before. The steering prompt is the guard, and a structured tool is a more natural affordance than the prior "present a numbered list and wait for a typed number" prose, so this is more reliable than the inline-numbered-list mitigation it replaces. The verified keystroke infra (`computeAuqKeystrokes`, `POST /keystrokes`, `translate_keystroke_dsl`) remains in the tree but is no longer wired into the AUQ path.

**Prompt submission needs a separate Enter.** claude's TUI paste-detects a large single write and swallows an inline trailing CR as a soft newline rather than submitting. The input bridge therefore writes a prompt's body, settles `PROMPT_SUBMIT_SETTLE_MS`, then sends the submitting Enter as its own write. Short prompts submit either way; this fixes long prompts silently not submitting.

To re-verify any of the above after a Claude Code bump, spawn `claude --permission-mode bypassPermissions --settings '{"skipDangerousModePermissionPrompt":true}'` under a PTY and read the startup/JSONL bytes (lumina discards the PTY stream; a throwaway portable-pty reader probe reveals the dialog, the picker layout, and the JSONL flush timing).

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

- `cargo nextest run --manifest-path lumina/Cargo.toml` — run the full test suite (smoke + showcase + e2e + migration tests + in-module tests) with process-per-test isolation
- `cargo nextest run --manifest-path lumina/Cargo.toml --profile ci` — same with JUnit XML output at `target/nextest/ci/junit.xml`
- `cargo test --manifest-path lumina/Cargo.toml` — still works; the `#[test]` functions are the same; `cargo test` runs them under rustc's built-in runner
- `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest --html --output-dir target/coverage/html` — HTML coverage report (llvm-cov composes with nextest natively via the `nextest` subcommand)
- `cargo llvm-cov --manifest-path lumina/Cargo.toml nextest --lcov --output-path lcov.info --fail-under-lines 80 --fail-under-regions 70` — lcov export with the recommended gate

### Nextest config

`lumina/.config/nextest.toml` defines two profiles: `default` (no retries) and `ci` (one retry, JUnit XML emit, immediate failure output).
<!-- TEST-BOOTSTRAP:STACK END -->

## Transactions

Repo-layer mutations open a write transaction through the Part-A backend-erased seam — `db.begin()` on the `AnyPool`/`DbClient` surface (its SQLite arm issues `BEGIN IMMEDIATE` via `begin_with`), NOT the default `pool.begin()` (`BEGIN DEFERRED`). The legacy `db::begin_write(pool: &SqlitePool)` helper still exists for the few raw-pool call sites, and issues the same `BEGIN IMMEDIATE`. IMMEDIATE acquires the SQLite RESERVED lock at begin-time, so writer contention surfaces before any statements execute — combined with WAL + a 5s `busy_timeout` set in `db::init` on on-disk databases (in-memory pools skip WAL), concurrent MCP tool writes serialise without `SQLITE_BUSY` flake. The export drain in `export.rs` deliberately stays on auto-commit reads + a single auto-commit `UPDATE events SET exported_at` per event; it does NOT open a transaction, because a long read transaction in the drain would defeat WAL's reader/writer concurrency. The single-mutation-path invariant ("one domain write ⇒ one event row, atomically, or neither") is enforced by every `repo::*` mutator opening exactly one write tx and committing both rows together. (The migration-0011 Part-B batch-write paths are the deliberate exception: one batch tx records a single coarse, export-inert event covering all the bulk rows — see § MCP tool surface.) The `tests/concurrency.rs` smoke test exercises N=8 concurrent `create_work_item` calls against an on-disk pool to keep this discipline honest.

Note (Part A): the repo no longer depends on the sqlx `query!`/`query_as!` compile-time macros, so there is no `.sqlx` offline cache and no `cargo sqlx prepare` gate — every query is a runtime `sqlx::query*` call behind the `DbClient`/`DbTx` seam. The `migrate` feature is kept for the compile-time `sqlx::migrate!("./migrations")` in `db.rs`.

## MCP tool surface

The lumina MCP server exposes a domain-shaped tool surface for managing the work-item hierarchy and the planning/decision lifecycle. The authoritative catalogue lives at `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` — that doc enumerates every `mcp__lumina__*` tool, the parameter shapes, and the planning-tools section that the story-block plugin's skills reference. Skim it before invoking the plugin skills below; the plugin's skills compose those MCP tools and assume the reader has the catalogue's terminology in hand.

The migration-0005 story-planning-round-2 pass added the following tool families:
- **Risks CRUD** (`add_risk`, `update_risk`, `supersede_risk`, `remove_risk`) — first-class risk records on stories. Columns: `summary` (short label, NOT NULL), `body` (longer detail, optional), `severity` (typed `RiskSeverity::{Low, Medium, High, Critical}`, wire `low|medium|high|critical`, CHECK-enforced on the `risks` table), and `mitigation` (optional). Supersession chains preserve history.
- **Rejected alternatives CRUD** (`add_rejected_alternative`, `update_rejected_alternative`, `supersede_rejected_alternative`, `remove_rejected_alternative`) — design-decision records capturing what was considered but not chosen, along with the reason; supersession mirrors the research-note pattern.
- **Task graph** (`block_task_on_task`, `unblock_task_from_task`, `list_task_dependencies`, `compute_task_batches`) — fine-grained prerequisite edges between tasks within a story; `compute_task_batches` returns the topologically-sorted execution waves, respecting both task-on-question and task-on-task blocks.
- **Story readiness** (`get_story_readiness`) — derives a readiness verdict (ready / blocked / incomplete) from closure-gate criteria, open questions, unresolved risks, and pending acceptance criteria; used by the sprint composer to gate dispatch.
- **Task kind discriminator** (`set_task_kind`) — stamps a task's `task_kind` column (NOT the hierarchy `kind` column) with the typed `TaskKind` enum. Round-2 introduced the column with a four-value taxonomy (`foundation|vertical-slice|pattern-replacement|polish`); migration 0007 (round-3.5 review follow-up) culled the vocab to the genuinely task-level three-value set (`foundation|main|polish`, kebab-case wire form, CHECK-enforced). Three buckets describe a task's role WITHIN its phase: foundation (prerequisite; floats earliest in intra-phase sort), main (core body of work; default), polish (hardening / quality; sinks latest). Vertical-slice and pattern-replacement are NOT modelled at the task level — they are **intra-story task-subset groupings** (a story may contain 0+ such groups; a task may belong to 0+ groups) that mark units-of-implementation (the tasks in one group are implemented + tested + committed together). Round-3.5 does not model these groupings in schema; `/lumina:decompose-tasks` surfaces them in proposal prose and the implementer respects them informally until a future migration adds `task_groups` + `task_group_members` (driven by a real consumer such as `/lumina:run-batch`). Migration 0007 maps any pre-existing `vertical-slice` / `pattern-replacement` row values on `work_items.task_kind` to `main` (the closest equivalent task-level disposition for tasks that had been mislabelled with the grouping shorthand).
- **Widened `set_story_plan`** — the tool now accepts two additional JSON-merge fields: `not_doing` (a free-text scope-exclusion note) and `verification_commands` (a JSON array of shell commands that define the story's done-signal, mirroring the plan-file `## Verification Commands` convention).

The migration-0006 story-planning-round-3 pass added the following:
- **Dispatch tier** (`set_task_tier`, `get_task_dispatch_plan`) — typed `Tier::{Lite, Deep}` stored on the new `work_items.tier` column (CHECK-enforced). `set_task_tier` writes the column directly; `get_task_dispatch_plan` composes `compute_task_batches` with per-task spec reads and runs `compute_tier(effort, complexity, files_touched_count, has_cross_repo)` per row, returning `Vec<Vec<BatchEntry>>` (one inner Vec per parallel-safe batch). The derivation rule (Deep if complexity=high OR effort=L OR files>3 OR cross-repo; else Lite) lives in `repo::compute_tier` and is documented in CONVENTIONS.md §k of the lumina-story-blocks plugin.
- **Tightened `set_task_spec`** — the round-2 free-form `dispatch: Option<serde_json::Value>` field was renamed to `tier: Option<Tier>` (typed). When `tier` is present, the tool also makes a SECOND mutation through `set_task_tier`. Legacy callers passing `dispatch:` have their value silently dropped at deserialise (the field is gone from the struct).
- **Finding-severity typing**: `AddFindingParams.severity` / `UpdateFindingParams.severity` already accepted typed `Severity::{Critical, Major, Minor, Suggestion}` (the review-finding categorisation vocabulary). Round-3 documents this in the catalogue; the wire shape is unchanged. NOTE the deliberate vocab split — `RiskSeverity::{Low, Medium, High, Critical}` (CHECK-enforced on `risks.severity`) is a distinct enum for risk severity. The two vocabularies are not unified.

The migration-0008 PTY supervisor pass originally added six tools for managing interactive `claude` REPL sessions: `list_pty_sessions` (read-only) and `get_pty_session` (read-only) for inspection; `spawn_pty_session` to launch a session via `PtyTransport` (portable-pty 0.9, ConPTY on Windows / Unix98 elsewhere) under a validated `cwd`; `send_pty_input` to enqueue a typed `InputFrame` against the per-session FIFO; `cancel_pty_session` to push a Cancel control frame; `delete_pty_session` to tombstone the row. The lumina-interactive-prompts plan (2026-05-28) removed all six MCP tools: the PTY service is now driven exclusively via the HTTP API + the SPA, not MCP. The underlying transport (`PtyTransport`) and the HTTP routes in `lumina/src/http/pty_sessions.rs` remain. PTY tables (`pty_sessions`, `pty_messages`, `pty_queue`) deliberately stay outside the `+1 work_items / +1 events` invariant — runtime PTY state is not a domain entity. Tool surface was 55 after this pass.

The migration-0010 epic/focus-semantics pass (renaming `feature`→`focus`) added three tools: `set_shape` (set a focus's `vertical-slice|cross-cutting|foundational` shape, focus-kind-gated), `set_epic_plan` (revise an epic's `outcome`/`context` plan attributes, JSON-merge), and `set_focus_plan` (revise a focus's `framing`, JSON-merge). The `create_work_item` tool now threads `outcome` (mandatory for an `epic`) and `shape` (mandatory for a `focus`) through `repo::create_work_item_full`. `set_closure_gate` remains story-only (the epic-done gate is unconditional and does not read `closure_gate`).

The migration-0011 Part-B batch/query/triage-domain pass added nine tools across three families:
- **Batch-write** (B18): `add_findings` (bulk-insert findings under ONE transaction, optional top-level `run_id` applied to every element; the repo stamps each finding's dedup content hash, so a dedup-collapse onto a live row counts as `skipped` not an error; returns `{ added, skipped, skipped_ids }`), `create_work_items` (all-or-nothing bulk-create; a single invalid spec aborts the batch and zero rows persist; parents must already exist; returns `{ ids }` in input order; each spec may carry `spawned_from_finding_id`), `batch_update_findings` (all-or-nothing bulk NON-terminal triage update of `triage_state`/`severity`/`category`/`status`; a terminal disposition is rejected — use `resolve_finding` — and a missing finding id aborts the batch; returns `{ updated }`).
- **Query** (B21): `query_findings` (query LIVE findings with a static NULL-guard filter over `work_item_id`/`run_id`/`severity`/`category`/`status`/`triage_state` — an absent field is unconstrained, so one prepared statement covers every combination; `count_by="severity"` switches to grouped `{ counts }` mode; read-only), `get_story_finding_queue` (compose a story's queue: every live finding on the story itself OR a DIRECT task child, newest-flagged first, excluding tombstoned items; read-only).
- **Run/sprint/triage domain** (B24): `create_run` (open a `review|optimise` run over a `sprint|story` target; status `open`; returns `{ run_id }`), `create_sprint` (open a sprint with optional title; returns `{ sprint_id }`), `add_tasks_to_sprint` (attach tasks to a sprint under ONE transaction; idempotent at the junction via `ON CONFLICT DO NOTHING`, an already-attached pair is not counted in `added`; a non-task/missing id aborts the batch), `record_finding_decision` (`spawn_task|spawn_story|defer|dismiss|resolve` — a spawn creates a child under the finding's host returned as `spawned_work_item_id`, `resolve` delegates to `resolve_finding`, `defer`/`dismiss` set the triage state; returns `{ decision_id, spawned_work_item_id }`).

Domain model: a `run` = one review/optimise pass over a sprint or story (status `open→triaged→closed`); persisted `sprints` + the `sprint_tasks` junction; `finding_decisions` = an append-only triage audit (`spawn_task`/`spawn_story`/`defer`/`dismiss`/`resolve`); `findings` gained `run_id`/`triage_state`, and bulk-spawned items carry `work_items.spawned_from_finding_id`. The batch-write tools deliberately deviate from the per-call `+1 work_items / +1 events` invariant: each records exactly ONE coarse, export-INERT event (`aggregate_type` ∈ run/sprint/finding/batch, never `work_item`), so bulk-created / spawned items are NOT git-exported individually (the accepted D8/R-B4 trade-off). Tool surface is now 67.

## HTTP routes

The axum API mirrors the MCP write surface so a browser/SPA client (or any HTTP-capable tool) can drive the same store the MCP server drives. Every HTTP write delegates to a single `repo::*` mutation — the single-mutation-path invariant (+1 work_items / +1 events per call) is preserved alongside the MCP layer, and the per-family sub-routers in `lumina/src/http/*.rs` are mounted under `/api` by `app::build_router`. Routes below are listed with their final path (relative to `/api`) and the `repo::*` function each handler calls. The two structured PATCHes (`/story-plan`, `/task-spec`) compose via `repo::set_work_item_attributes` (read-modify-merge JSON-merge) and — for `/task-spec` — a second `repo::set_task_tier` mutation when `tier` is present, exactly mirroring the MCP `set_story_plan` / `set_task_spec` tools. Task-graph cycles surface via the envelope `{"error":{"kind":"cycle","message":...,"edges":[{"task_id":...,"depends_on_id":...},...]}}` (422), produced by `AppError::Cycle`'s `IntoResponse` impl on `POST /work-items/{task_id}/depends-on/{depends_on_id}` and `GET /work-items/{story_id}/task-batches`.

### Work-items CRUD (`http/work_items.rs`)

- `GET    /health`                          → liveness probe (no DB hit).
- `GET    /work-items`                      → `repo::list_work_items` (default: full nested tree of roots; with `?parent_id=`/`?kind=`: flat filtered list).
- `GET    /work-items/{id}`                 → `repo::get_work_item_detail`.
- `POST   /work-items`                      → `repo::create_work_item_with_origin` (migration 0010: body now carries `outcome` — mandatory for `kind:"epic"` — and `shape` — mandatory for `kind:"focus"`, ∈ `vertical-slice|cross-cutting|foundational`).
- `PATCH  /work-items/{id}`                 → `repo::update_work_item`.
- `DELETE /work-items/{id}`                 → `repo::delete_work_item` (soft-delete; round-4 T1 closed the "full mirror" gap against the MCP `delete_work_item` tool).

### Project↔repo-links (`http/repo_links.rs`, migration 0004)

- `POST   /work-items/{project_id}/repo-links`        → `repo::add_repo_link`.
- `DELETE /work-items/{project_id}/repo-links/{id}`   → `repo::remove_repo_link`.
- `PATCH  /work-items/{project_id}/repo-links/{id}`   → `repo::set_primary_repo` (body must be `{"is_primary": true}`; demotion happens implicitly via promoting another link).

### Structured patches (`http/structured_patches.rs`, round-4 T2)

Six scalar PATCHes — body shape `{ "value": <enum> }`. The four non-nullable scalars reject `{"value": null}` with 422; the two nullable scalars (`task-kind`, `tier`) accept `null` as a clear signal.

- `PATCH /work-items/{id}/relevance`      → `repo::set_relevance`.
- `PATCH /work-items/{id}/effort`         → `repo::set_effort`.
- `PATCH /work-items/{id}/complexity`     → `repo::set_complexity`.
- `PATCH /work-items/{id}/closure-gate`   → `repo::set_closure_gate`.
- `PATCH /work-items/{id}/task-kind`      → `repo::set_task_kind` (nullable).
- `PATCH /work-items/{id}/tier`           → `repo::set_task_tier` (nullable).
- `PATCH /work-items/{id}/shape`          → `repo::set_shape` (migration 0010; non-nullable scalar, focus-only; `vertical-slice|cross-cutting|foundational`).

Two structured PATCHes — JSON-merge bodies via `repo::set_work_item_attributes`; `task-spec` additionally calls `repo::set_task_tier` when `tier` is present (two mutations per call, mirroring the MCP `set_task_spec` tool).

- `PATCH /work-items/{id}/story-plan`     → composes `repo::set_work_item_attributes` (fields: `problem_statement`/`research_notes`/`execution_strategy`/`not_doing`/`verification_commands`).
- `PATCH /work-items/{id}/task-spec`      → composes `repo::set_work_item_attributes` (+ `repo::set_task_tier` when `tier` present; fields: `execution_detail`/`files_touched`/`outcome`/`tier`).
- `PATCH /work-items/{id}/epic-plan`      → `repo::set_epic_plan` (migration 0010; epic-only JSON-merge; body `{outcome?, context?}`).
- `PATCH /work-items/{id}/focus-plan`     → `repo::set_focus_plan` (migration 0010; focus-only JSON-merge; body `{framing?}`).

### Acceptance criteria (`http/acceptance_criteria.rs`, migration 0003, round-4 T3)

- `POST   /work-items/{id}/acceptance-criteria`  → `repo::add_acceptance_criterion`.
- `POST   /acceptance-criteria/{id}/check`       → `repo::check_acceptance_criterion` (appends a `verification` activity row inside the same txn).
- `POST   /acceptance-criteria/{id}/uncheck`     → `repo::uncheck_acceptance_criterion`.
- `DELETE /acceptance-criteria/{id}`             → `repo::remove_acceptance_criterion`.

### Research notes (`http/research_notes.rs`, migration 0003, round-4 T3)

- `POST  /work-items/{id}/research-notes`              → `repo::add_research_note`.
- `PATCH /research-notes/{id}`                         → `repo::update_research_note` (partial set-or-leave).
- `POST  /research-notes/{old_id}/supersede/{new_id}`  → `repo::supersede_research_note`.

### Risks (`http/risks.rs`, migration 0005, round-4 T4)

- `POST   /work-items/{id}/risks`              → `repo::add_risk` (typed `RiskSeverity`).
- `PATCH  /risks/{id}`                         → `repo::update_risk`.
- `POST   /risks/{old_id}/supersede/{new_id}`  → `repo::supersede_risk` (new id is path documentation only; repo mints a fresh `now_v7` uuid).
- `DELETE /risks/{id}`                         → `repo::remove_risk`.

### Rejected alternatives (`http/rejected_alternatives.rs`, migration 0005, round-4 T4)

- `POST   /work-items/{id}/rejected-alternatives`              → `repo::add_rejected_alternative`.
- `PATCH  /rejected-alternatives/{id}`                         → `repo::update_rejected_alternative`.
- `POST   /rejected-alternatives/{old_id}/supersede/{new_id}`  → `repo::supersede_rejected_alternative`.
- `DELETE /rejected-alternatives/{id}`                         → `repo::remove_rejected_alternative`.

### Task dependencies (`http/task_dependencies.rs`, migration 0005, round-4 T4)

- `POST   /work-items/{task_id}/depends-on/{depends_on_id}`  → `repo::add_task_dependency` (MCP tool name: `block_task_on_task`).
- `DELETE /work-items/{task_id}/depends-on/{depends_on_id}`  → `repo::remove_task_dependency` (MCP tool name: `unblock_task_from_task`).
- `GET    /work-items/{story_id}/task-dependencies`          → `repo::list_task_dependencies`.
- `GET    /work-items/{story_id}/task-batches`               → `repo::compute_task_batches` (Kahn's-algorithm per-phase batches; cycle → 422 envelope).

### Open questions (`http/open_questions.rs`, migration 0003, round-4 T5)

- `POST  /work-items/{story_id}/open-questions`               → `repo::add_open_question`.
- `POST  /open-questions/{id}/options`                        → `repo::add_question_option`.
- `POST  /work-items/{task_id}/block-on-question/{question_id}` → `repo::block_task_on_question`.
- `PUT   /work-items/{task_id}/enabling-option/{option_id}`   → `repo::set_enabling_option`.
- `POST  /open-questions/{id}/resolve`                        → `repo::resolve_open_question` (one event for the whole branch-unblock + sibling-cancel resolution).

### Findings (`http/findings.rs`, round-4 T5)

- `POST  /work-items/{id}/findings`                → `repo::create_finding`.
- `PATCH /findings/{id}`                           → `repo::update_finding` (partial set-or-leave).
- `POST  /findings/{id}/resolve`                   → `repo::resolve_finding` (terminal disposition).
- `POST  /findings/{old_id}/supersede/{new_id}`    → `repo::supersede_finding`.

### Findings/Runs/Sprints batch + query (migration 0011, Part B)

The HTTP mirrors of the migration-0011 Part-B MCP tools. Each delegates to the same `repo::*` mutation the matching MCP tool calls; the three batch-write routes deliberately stand outside the per-call `+1 work_items / +1 events` invariant, recording exactly one coarse, export-inert event per call (see § MCP tool surface). Routes live in `http/findings.rs` (batch), `http/queries.rs` (query), `http/runs.rs`, and `http/sprints.rs`.

- `POST /findings/batch`                         → `repo::add_findings` (bulk-add; optional top-level `run_id`; dedup-collapse counts as `skipped`; → `{ added, skipped, skipped_ids }`).
- `POST /findings/batch-update`                  → `repo::batch_update_findings` (all-or-nothing bulk NON-terminal triage update; terminal disposition rejected; → `{ updated }`).
- `GET  /findings/query`                         → `repo::query_findings` (static NULL-guard filter; `?count_by=severity` switches to grouped `{ counts }` mode; the static segment is registered ahead of the dynamic `/findings/{id}` paths to avoid a collision).
- `GET  /work-items/{story_id}/finding-queue`    → `repo::get_story_finding_queue` (story + DIRECT task children, newest-flagged first; excludes tombstoned).
- `POST /runs`                                   → `repo::create_run` (open a `review|optimise` run over a `sprint|story`; → `{ run_id }`).
- `POST /findings/{finding_id}/decision`         → `repo::record_finding_decision` (`spawn_task|spawn_story|defer|dismiss|resolve`; → `{ decision_id, spawned_work_item_id }`).
- `POST /sprints`                                → `repo::create_sprint` (open a sprint, optional title; → `{ sprint_id }`).
- `POST /sprints/{sprint_id}/tasks`             → `repo::add_tasks_to_sprint` (attach tasks; idempotent at the junction; → `{ added }`).

### Activity log (`http/activity.rs`, migration 0002, round-4 T5)

- `POST /work-items/{id}/activity` → `repo::append_activity` (entry-kind validation is delegated to `repo::validate_entry_kind`; `body`/`ref_id` fold into the activity payload object).

### Context blocks (`http/context_blocks.rs`, round-4 T5)

- `POST   /context-blocks`                          → `repo::create_context_block`.
- `POST   /work-items/{id}/context-blocks/{cb_id}`  → `repo::link_context_block`.
- `DELETE /work-items/{id}/context-blocks/{cb_id}`  → `repo::unlink_context_block`.

### Story readiness + dispatch plan (`http/readiness.rs`, migration 0005/0006, round-4 T5)

- `GET /work-items/{story_id}/readiness`      → `repo::get_story_readiness` (the `StoryReadiness` aggregate driving `/lumina:next-block` / `/lumina:plan-story`).
- `GET /work-items/{story_id}/dispatch-plan`  → `repo::get_task_dispatch_plan` (`Vec<Vec<BatchEntry>>` waves; cycle → 422 envelope).

### PTY sessions (`http/pty_sessions.rs`, migration 0008, T9)

- `GET    /api/pty/sessions`               → `repo::pty::list_pty_sessions`.
- `POST   /api/pty/sessions`               → composes `PtyTransport::spawn` + `repo::pty::create_pty_session` + registry insert + supervisor registration.
- `GET    /api/pty/sessions/{id}`          → `repo::pty::get_pty_session`.
- `GET    /api/pty/sessions/{id}/messages` → `repo::pty::list_pty_messages` (paginated via `?since=&limit=`).
- `GET    /api/pty/sessions/{id}/queue`    → `Queue::list` (per-session inbound FIFO inspection).
- `POST   /api/pty/sessions/{id}/input`    → `Queue::enqueue`.
- `POST   /api/pty/sessions/{id}/inputs/batch` → batched `Queue::enqueue` (atomic per call site).
- `POST   /api/pty/sessions/{id}/keystrokes` → direct-push keystroke frames to `Session::input_tx` (queue/supervisor bypass; retained but no longer on the AUQ path).
- `POST   /api/pty/sessions/{id}/ask/{question_id}/answer` → `answer_question`: fulfils the `oneshot` for a blocked `ask_user_question` MCP tool call (`pty::ask`), clears bookkeeping, broadcasts the closing synthetic `tool_result`. Body `{answers: AuqAnswer[], cancelled?: bool}`. 409 if no such pending question. (The matching ASK side is the `/mcp-ask` MCP mount, NOT an `/api` route.)
- `PATCH  /api/pty/sessions/{id}`          → currently returns 501 (no `update_pty_session_meta` helper in v1; future work).
- `DELETE /api/pty/sessions/{id}`          → soft cancel via Cancel InputFrame + `repo::pty::delete_pty_session`.
- `GET    /api/pty/sessions/{id}/ws`       → WebSocket upgrade; Origin-allowlist (localhost variants + `LUMINA_DEV_ORIGIN`), broadcast subscriber → JSON frame stream (Message/Status/Skipped/Error/Pong out; Input/Resize/Ping in).

## Story-block skills plugin

Lumina's MCP tool surface is driven by the `/lumina:<block>` skills in the plugin at
`claude/plugins/lumina-story-blocks/`. Round-1 shipped nine per-block writers (problem-statement, research-notes, user-interrogation, acceptance-criteria, approach, not-doing, edge-cases, relevance, closure-gate); round-2 added ten more (risks, alternatives, verification-commands, vet-research, story-review, next-block advisor, plan-story chained runner, decompose-tasks, set-task-spec, wire-task-deps); round-3 added two more research skills (research-explore for multi-agent parallel exploration; research-directed for post-decision verification) and amended four round-2 skills (plan-story now enforces a six-phase canonical sequence with hard gates + override-audit; set-task-spec captures effort+complexity and computes the dispatch tier; wire-task-deps renders the batch dispatch budget; vet-research parallelises spot-checks) — twenty-one `/lumina:*` slash commands; the migration-0010 epic/focus wave added four more (epic-outcome, focus-shape, focus-framing, epic-close-criteria), for twenty-five total. The prerequisites checklist (server running, MCP registered as `lumina`) and the full skill catalogue live in `claude/plugins/lumina-story-blocks/README.md`; the round-2 and round-3 MCP tool catalogue extensions are in `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md`.

Permanent install (persists to `.claude/settings.json`, all clones inherit):

```
claude plugin install --scope project ./claude/plugins/lumina-story-blocks
```

One-off session load (no persistence — for ad-hoc trials):

```
claude --plugin-dir claude/plugins/lumina-story-blocks
```
