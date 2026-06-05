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
- **Server.** `ask.rs` is the `/mcp-ask` mount (`app::build_router`), a deliberately minimal MCP server exposing ONLY `ask_user_question` — the full work-item tool surface at `/mcp` (74 tools as of migration 0015) is **not** exposed to spawned sessions. The tool resolves the session by `session_id`, registers a per-question `oneshot` in `Session::pending_questions`, marks the session non-quiescent (adds the synthetic question id to `outstanding_tool_uses` so the supervisor won't flip it Idle while the operator decides), broadcasts a synthetic `tool_use(AskUserQuestion)` (the SAME shape `jsonl_tail::map_record_to_typed` produces, so the EXISTING `PtyAuqPicker` SPA renders it unchanged), and BLOCKS on the oneshot (30-min cap; on timeout it closes the picker and returns a "no answer" result).
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

## Build discipline in flows

lumina is the heaviest crate here to compile, so the repo-wide **Build discipline in multi-agent flows** rule (root `CLAUDE.md`) bites hardest in this subtree: a sub-agent in `/implement` / `/optimise-apply` / `/review-apply` / `/tdd` must NOT run `cargo build` / `cargo test` / `cargo nextest` to self-verify. Reach for `cargo check --manifest-path lumina/Cargo.toml` (or `cargo clippy`) **sparingly** — at most once near the end of a cluster — and leave the full build + nextest run to the orchestrator's single `verification` pass. Parallel sub-agents each rebuilding the whole crate against the shared `lumina/target` lock is exactly the waste to avoid.

## Transactions

Repo-layer mutations open a write transaction through the Part-A backend-erased seam — `db.begin()` on the `AnyPool`/`DbClient` surface (its SQLite arm issues `BEGIN IMMEDIATE` via `begin_with`), NOT the default `pool.begin()` (`BEGIN DEFERRED`). The legacy `db::begin_write(pool: &SqlitePool)` helper still exists for the few raw-pool call sites, and issues the same `BEGIN IMMEDIATE`. IMMEDIATE acquires the SQLite RESERVED lock at begin-time, so writer contention surfaces before any statements execute — combined with WAL + a 5s `busy_timeout` set in `db::init` on on-disk databases (in-memory pools skip WAL), concurrent MCP tool writes serialise without `SQLITE_BUSY` flake. The export drain in `export.rs` deliberately stays on auto-commit reads + a single auto-commit `UPDATE events SET exported_at` per event; it does NOT open a transaction, because a long read transaction in the drain would defeat WAL's reader/writer concurrency. The single-mutation-path invariant ("one domain write ⇒ one event row, atomically, or neither") is enforced by every `repo::*` mutator opening exactly one write tx and committing both rows together. (The migration-0011 Part-B batch-write paths are the deliberate exception: one batch tx records a single coarse, export-inert event covering all the bulk rows — see § MCP tool surface.) The `tests/concurrency.rs` smoke test exercises N=8 concurrent `create_work_item` calls against an on-disk pool to keep this discipline honest.

Note (Part A): the repo no longer depends on the sqlx `query!`/`query_as!` compile-time macros, so there is no `.sqlx` offline cache and no `cargo sqlx prepare` gate — every query is a runtime `sqlx::query*` call behind the `DbClient`/`DbTx` seam. The `migrate` feature is kept for the compile-time `sqlx::migrate!("./migrations")` in `db.rs`.

## MCP tool surface

The lumina MCP server exposes a domain-shaped tool surface for managing the work-item hierarchy and the planning/decision lifecycle. The authoritative catalogue lives at `claude/plugins/lumina-story-blocks/skills/mcp/SKILL.md` — that doc enumerates every `mcp__lumina__*` tool, the parameter shapes, and the planning-tools section that the story-block plugin's skills reference. Skim it before invoking the plugin skills below; the plugin's skills compose those MCP tools and assume the reader has the catalogue's terminology in hand.

The migration-0005 story-planning-round-2 pass added the following tool families:
- **Risks CRUD** (`add_risk`, `update_risk`, `supersede_risk`, `remove_risk`) — first-class risk records on stories. Columns: `summary` (short label, NOT NULL), `body` (longer detail, optional), `severity` (typed `RiskSeverity::{Low, Medium, High, Critical}`, wire `low|medium|high|critical`, CHECK-enforced on the `risks` table), and `mitigation` (optional). Supersession chains preserve history.
- **Rejected alternatives CRUD** (`add_rejected_alternative`, `update_rejected_alternative`, `supersede_rejected_alternative`, `remove_rejected_alternative`) — design-decision records capturing what was considered but not chosen, along with the reason; supersession mirrors the research-note pattern.
- **Task graph** (`block_task_on_task`, `unblock_task_from_task`, `list_task_dependencies`, `compute_task_batches`) — fine-grained prerequisite edges between tasks within a story; `compute_task_batches` returns the topologically-sorted execution waves, considering ONLY task→task dependency edges. It does NOT consult `blocked_by_question_id` or `status` (verified at `repo.rs` — it loads all task children with no status filter and runs Kahn's over task→task edges alone). Question-blocking is a SEPARATE mechanism: resolving/raising an open question sets a task's `status='blocked'` + `blocked_by_question_id`, which removes the task from the migration-0013 CLAIM's ready set (the claim has its own readiness predicate — `status='todo'` AND `blocked_by_question_id IS NULL` AND no unsatisfied dep — NOT `compute_task_batches`).
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

Domain model: a `run` = one review/optimise pass over a sprint or story (status `open→triaged→closed`); persisted `sprints` + the `sprint_tasks` junction; `finding_decisions` = an append-only triage audit (`spawn_task`/`spawn_story`/`defer`/`dismiss`/`resolve`); `findings` gained `run_id`/`triage_state`, and bulk-spawned items carry `work_items.spawned_from_finding_id`. The batch-write tools deliberately deviate from the per-call `+1 work_items / +1 events` invariant: each records exactly ONE coarse, export-INERT event (`aggregate_type` ∈ run/sprint/finding/batch, never `work_item`), so bulk-created / spawned items are NOT git-exported individually (the accepted D8/R-B4 trade-off). Tool surface was 67 after this pass.

The migration-0013 team-execution pass added the atomic work-queue ([ADR-0002](../docs/adr/0002-sprint-execution-architecture.md) layer 1) that lets a team of agents execute a pre-planned task graph concurrently against the durable store — six new tools (four write, two read) plus their HTTP mirrors (`lumina/src/http/execution.rs`), taking the surface to 73:
- **Claim / lease** (write): `claim_next_task` (`{sprint_id, lane, tier?, agent_id, lease_ttl_secs} → {claimed: ClaimedTask|null}`) is ONE `BEGIN IMMEDIATE` txn — lazy-reclaim expired leases (one coarse, export-inert `leases.reclaimed` event only when rows are reclaimed) → SQL candidate select (first ready `todo` task with `assignee IS NULL`, matching `lane`/optional `tier`, `blocked_by_question_id IS NULL`, no unsatisfied task→task dep) → lease UPDATE (`status='in_progress'`, `assignee`, `lease_expires_at`) + `work_item.claimed` event. The SELECT→UPDATE in one writer txn is the race-free primitive the agent-teams shared list cannot give; `Ok(None)` (no ready candidate or a non-runnable sprint) returns `{claimed: null}`, NOT an error. `release_task` (`{task_id, agent_id} → {released}`) is owner-guarded (`WHERE assignee=:agent_id`) and resets `in_progress→todo` but LEAVES `blocked` blocked (park-after-question). `renew_lease` (`{task_id, agent_id, lease_ttl_secs} → {renewed}`) is the heartbeat (generous 30-min default TTL so heartbeats are infrequent).
- **Complete + cascade** (write): `complete_task` (`{task_id, agent_id} → {task_id, review_task_id?}`) runs two composed, idempotent txns — transition to `done` via `update_work_item_status` (closure-gate preserved) + clear the lease; then, ONLY for `lane='implement'` tasks, spawn a review task under the impl task's story (`lane='review'`, `tier=NULL`, `reviews_work_item_id` back-link, copied `files_touched`, a task→task dep edge, and a `sprint_tasks` binding so the claim can see it). A `lane='review'` or NULL-lane completion spawns NOTHING (prevents an infinite review↔review cascade). Re-running on an already-`done` task is idempotent (crash recovery for the two-txn window). Reviewers spawn rework via the existing findings→`record_finding_decision(spawn_task)` path (the SpawnTask path now stamps `lane='implement'` + `tier=NULL` on the rework task and increments the host finding's `rounds`; a `rounds`-cap defers to human escalation via `add_open_question`).
- **Quiescence + arbiter reads**: `get_sprint_quiescence` (`{sprint_id} → {claimable, in_progress, blocked_on_question, terminal, done, stalled}`) — the lead polls this to terminate (`done`) or escalate (`stalled`). `list_open_questions_for_sprint` (`{sprint_id} → [{question_id, story_id, text, options, age_secs}]`) — an arbiter agent resolves the unresolved, sprint-scoped questions.

Schema: migration `0013_team_execution.sql` adds four nullable `work_items` columns — `assignee` (the agent id holding the lease, canonical "who"), `lease_expires_at` (ISO-8601 deadline; reclaimed when past), `lane` (`CHECK (lane IS NULL OR lane IN ('implement','review'))`; NULL = not team-managed → invisible to the claim, preserving back-compat) and `reviews_work_item_id` (self-FK: review task → the impl task it covers) — plus two partial indexes (`(lane, tier, status) WHERE deleted_at IS NULL` for the claim hot path; `(lease_expires_at) WHERE assignee IS NOT NULL` for lazy reclaim). The four columns flow into `WorkItemDetail` and the git-export TOML snapshot automatically (export is event-driven off `work_items` rows).

File-overlap is **advisory, never a gate** (per [ADR-0002](../docs/adr/0002-sprint-execution-architecture.md)): because `files_touched` is best-effort, `claim_next_task` NEVER skips a candidate on overlap. After the lease commits, it computes — as a cheap read OUTSIDE the write txn (so no `files_touched` JSON parse runs inside the writer lock) — which other `in_progress` tasks in the sprint share any `files_touched` entry, returning them as `file_overlap_warnings` on the `ClaimedTask`. The team coordinates over peer `SendMessage` or proceeds with care; inter-sprint isolation is the consumer's per-sprint worktree (the layer-2 follow-up), not this check. The lazy lease-reclaim batch is the only place this pass deviates from the `+1 work_items / +1 events` invariant — one coarse, export-inert `leases.reclaimed` event covers the whole reclaim batch (precedented by the migration-0011 batch-write paths).

The migration-0014 repo-clone-path pass adds a per-machine clone-directory substrate to the migration-0004 `repo_links` row — and adds **NO MCP tool**: it left the surface at **73** (the next pass, migration-0015, is what takes it to 74 — see § Session corpus). The whole path substrate is HTTP-only (one new PATCH + one new read endpoint, both in `## HTTP routes`) plus internally-resolved helpers; an agent never needs an MCP tool to clone-resolve, so the tool surface is unchanged.
- **Schema**: migration `0014_repo_local_path.sql` adds one nullable column — `repo_links.local_path TEXT` (`ALTER TABLE repo_links ADD COLUMN local_path TEXT;`) — the per-machine ABSOLUTE clone directory the project's repo is checked out to ON THIS MACHINE; `NULL` = not cloned here. The column is **never canonicalised at store time** (the dir may not exist yet — the clone may not have happened), so it is purely lexical throughout. `RepoLink.local_path: Option<String>` (domain) flows through the generic `FromRow` + the `list_repo_links` SELECT into `WorkItemDetail` (project-kind only) and the git-export project snapshot, exactly like the other `repo_links` fields.
- **Mutator** `repo::set_repo_local_path(db, repo_link_id, local_path: Option<&str>)` (in `repo/repo_links.rs`) — single-mutation-path tx (one `UPDATE` + one event + commit). Resolves the owning project FIRST (absent id ⇒ `NotFound` before any write; the event aggregate is the project's `work_item`). `Some(raw)` SETS, `None` CLEARS to NULL. A `Some` value is first **trimmed** (review R13), then run through `normalise_path_structural` (verbatim-prefix strip + separator-fold + repeated-separator collapse + trailing-slash strip — **case-PRESERVED**, review R7) and that structural form is what is **validated and stored**: validation is `is_absolute_normalised` (a `/`-rooted OR drive-anchored `^[A-Za-z]:/` path), NOT raw `Path::is_absolute` — validating the normalised form keeps a Linux CI executor and a Windows operator consistent (review P5); a non-absolute value is `Validation`. The stored value therefore **preserves the operator's casing** — the host-keyed case fold is applied only at COMPARISON time (see caveat (b)). Emits `repo_link.local_path_changed` on the project's `work_item` aggregate (so the export drain re-renders the project).
- **Path-resolution helpers** (pure/lexical, no `canonicalize`, in `repo/repo_links.rs`): `resolve_repo_path(local_path, rel) -> PathBuf` joins a repo-relative `rel` against the clone dir, CLAMPING — every `..` (`ParentDir`) lexically CANCELS the last pushed component, clamped at the base (it can never pop into or above the base — review R5), `.` skipped, and any absolute anchor (`RootDir` / drive-or-UNC `Prefix`) in `rel` is IGNORED so an absolute `rel` resolves relative-to-base instead of REPLACING it; `rel`'s `\` separators are folded to `/` first so a Windows-style `rel` splits on Unix too (a security invariant: a `..`-escaping or absolute `rel` can never escape the clone dir). `select_longest_prefix_project(cwd, candidates) -> Option<String>` is the pure cwd→project resolver: it normalises `cwd` and each candidate `local_path`, matches on a COMPONENT BOUNDARY (`cwd == base` OR `cwd` starts with `base + "/"` — so `/dev/foobar` does NOT match `/dev/foo`), keeps the LONGEST matching `local_path` (deepest repo wins nesting), and returns `Some(project)` iff EXACTLY ONE DISTINCT project_id holds that longest length — a tie between two distinct projects ⇒ `None`. `resolve_cwd_to_project(db, cwd)` is its DB wrapper: it loads every `(project_id, local_path)` where `local_path IS NOT NULL` AND the project is NOT soft-deleted (`w.deleted_at IS NULL` — a cwd never binds to a tombstoned project), then defers to the pure resolver. Two private normalisers underpin all of the above: `normalise_path_structural` (verbatim-strip + separator-fold + repeated-`/` collapse preserving a UNC leading `//` + trailing-slash strip — **case-preserved**, used for storage and as the comparison base) and `normalise_path_for_compare` (= structural + `cfg(windows)` case fold — used by the matchers, so cwd↔`local_path` comparison is case-insensitive on Windows while the stored value stays case-faithful).
- **Caveats (deferred topology — both correct single-machine-now, both revisit with the per-machine path layer; see ADR-0004)**: (a) **shared-remote export-leak** — `local_path` lives on the SHARED `repo_links` row and flows into the git-export project snapshot; harmless while one lumina serves one machine, but a per-machine value leaking into a shared remote is the thing to relocate when the shared-remote layer lands. (b) **host-keyed case-fold** — `normalise_path_for_compare` (the COMPARISON normal form used by the cwd→project matcher; storage uses the case-PRESERVED `normalise_path_structural`) lowercases on `cfg!(windows)` of the lumina HOST, not the path's own filesystem; correct when the cwd and the stored `local_path` were both produced on the same host, but a cross-host *comparison* would need a path-keyed casing policy.

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
- `PATCH  /work-items/{project_id}/repo-links/{id}/local-path` → `repo::set_repo_local_path` (migration 0014; body `{"local_path": <string|null>}` → `{"ok": true}`; an absent/`null` `local_path` CLEARS the column, a string SETS it; the mutator normalises + validates-absolute + stores, resolving the owning project from the row itself, so `project_id` in the path is documentation only).

### Per-machine settings (`http/settings.rs`, migration 0014)

A single read-only, env-driven endpoint surfacing machine-local clone/export roots to the SPA — NO DB hit, NO write surface. Home is `http/settings.rs`; mounted via `http/mod.rs` (`settings::router()` merged under the `/api` nest — `app.rs` is untouched).

- `GET /api/settings`  → `{ "clone_root": <string|null>, "export_root": <string> }`. `clone_root` resolves from the `LUMINA_CLONE_ROOT` env var via `settings::resolve_clone_root()` (`null` when unset — there is NO compiled-in default); `export_root` mirrors `export::resolve_export_root()` (always present, with its `./.lumina/export` default). `LUMINA_CLONE_ROOT` is per-machine and mirrors `LUMINA_EXPORT_ROOT`, except it has no fallback default; it backs the SPA's "offer to clone" affordance and is read here only (never persisted to the DB).

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

### Git-export trigger (`http/export.rs`)

Export is OPERATOR-TRIGGERED, not continuous. The former 5-second background drain loop (`export::spawn` / `ExportHandle`) was removed — `app::serve` no longer spawns it. Outbox rows accumulate until a drain is requested; the transactional-outbox recovery invariant means none are ever lost in the meantime.

- `POST /export`  → `export::export_pending` (one drain pass over the events outbox; writes per-item TOML snapshots under the resolved root and stamps `exported_at`; idempotent — an empty outbox drains 0; → `{ drained, export_root }`). The root resolves from `LUMINA_EXPORT_ROOT` (default `./.lumina/export`) via `export::resolve_export_root`. Render/DB failure → 500; the failing events stay un-stamped for the next request. `export::export_pending` remains the directly-callable core the e2e drives without a socket bind.

### PTY sessions (`http/pty_sessions/` — submodule dir: `mod.rs` + `ask.rs` + `ws.rs`; migration 0008, T9)

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

### Team-execution work-queue (`http/execution.rs`, migration 0013)

The HTTP mirrors of the migration-0013 team-execution MCP tools. Each delegates to the same `repo::*` mutation/read the matching MCP tool calls; the single-mutation-path invariant holds (the lazy lease-reclaim batch inside `claim_next_task` is the one precedented coarse/export-inert exception). File-overlap is advisory (computed post-commit, never a gate — per ADR-0002).

- `POST /sprints/{sprint_id}/claim`              → `repo::claim_next_task` (body `{lane, tier?, agent_id, lease_ttl_secs}`; → `{claimed: ClaimedTask|null}` — `null` when no ready candidate or a non-runnable sprint, NOT an error).
- `POST /work-items/{task_id}/release`           → `repo::release_task` (body `{agent_id}`; owner-guarded; `in_progress→todo`, leaves `blocked`; → `{released}`).
- `POST /work-items/{task_id}/renew-lease`       → `repo::renew_lease` (body `{agent_id, lease_ttl_secs}`; heartbeat; → `{renewed}`).
- `POST /work-items/{task_id}/complete`          → `repo::complete_task` (body `{agent_id}`; done + idempotent review-task spawn for `implement`-lane tasks; → `{task_id, review_task_id?}`).
- `GET  /sprints/{sprint_id}/quiescence`         → `repo::get_sprint_quiescence` (→ `{claimable, in_progress, blocked_on_question, terminal, done, stalled}`).
- `GET  /sprints/{sprint_id}/open-questions`     → `repo::list_open_questions_for_sprint` (unresolved, sprint-scoped; → `[{question_id, story_id, text, options, age_secs}]`).

### Session-corpus ingest (`http/sessions.rs`, migration 0015)

One write endpoint backing the SessionEnd transcript-ingest hook (see § Session corpus). It deliberately stands OUTSIDE the per-call `+1 work_items / +1 events` invariant — a successful ingest records AT MOST ONE coarse, export-inert `session.ingested` event (never git-exported), and only when net-new corpus rows land (a fully-collapsing re-ingest records none).

- `POST /api/sessions/ingest` → `repo::sessions::ingest_transcript` (best-effort). Accepts the SessionEnd http-hook body `{session_id, transcript_path, cwd, hook_event_name?, reason?}` and returns **202 Accepted** immediately, spawning the ingest behind a 4-permit `AppState` semaphore (back-pressure, not a queue). **UNAUTHENTICATED by design** — it relies on lumina's loopback-only (`127.0.0.1`) deployment rule, the same posture as the rest of `/api`. Before returning 202 it CONFINES `transcript_path` to `~/.claude/projects`: canonicalise + `starts_with` the projects root, rejecting `..` / symlink-escape / out-of-root paths up front. The ingest itself is drop-if-no-lumina (a terminal transcript carrying no `mcp__lumina__*` call persists nothing — see § Session corpus drop-at-ingest asymmetry).

## Session corpus

Migration 0015 ([ADR-0004](../docs/adr/0004-harness-session-corpus.md) layer 2) adds a **lossless verbatim transcript corpus** of every `claude` session lumina touches — both the sessions it SPAWNS (via the PTY supervisor) and TERMINAL sessions INGESTED after the fact via the SessionEnd hook — plus the read-only `get_session_context` MCP tool that lets a session stamp its own work-item correlation into the transcript for later harvest. The corpus is **lossless-at-rest**; redaction is **egress-only and DEFERRED to layer 3** (nothing is scrubbed on the way in).

### Schema (`0015_session_corpus.sql`)

- **`pty_sessions` gains three columns**:
  - `source TEXT NOT NULL DEFAULT 'spawned' CHECK(source IN ('spawned','ingested'))` — discriminates a lumina-spawned PTY session from one ingested post-hoc from a terminal transcript.
  - `sprint_id TEXT` — nullable correlation hint, **NO foreign key**. A harvested sprint id is a `sprints.id` (migration 0011), NOT a `work_items.id`, and a hard FK would abort the lossless ingest on a deleted or cross-instance sprint; so this is a plain nullable TEXT hint (exactly like `agent_id`). Full multi-sprint detail stays in `session_records`.
  - `agent_id TEXT` — nullable correlation hint (the agent id last seen claiming work in the transcript).
- **New `session_records` table** — the lossless corpus, ONE row per non-empty JSONL line:
  - `id` (uuidv7), `session_id` (FK → `pty_sessions(id)` `ON DELETE RESTRICT` — protects the lossless corpus: deletes are SOFT today (`pty.rs` tombstones, never `DELETE`s), so no referential action fires; a FUTURE hard `DELETE FROM pty_sessions` that still has corpus rows FAILS LOUDLY rather than cascading them away), `line_ordinal` (**1-based among NON-EMPTY lines**), `record_type`, `record_uuid`, `parent_uuid`, `ts`, `is_sidechain` (INTEGER 0/1), `raw` (the VERBATIM JSONL line), `dedup_key` (namespaced `u:<record_uuid>` when the line carries a uuid, else the synthetic `o:<ordinal>` — the prefixes stop a uuid like `o:5` colliding with line-5's synthetic key; derived by `repo::corpus_dedup_key`, the single source shared by the ingest + live-tail paths), `created_at`.
  - `UNIQUE(session_id, dedup_key)` (the idempotency anchor for chunked re-ingest) plus 3 indexes.

### Repo layer (`repo/sessions.rs`)

- `insert_session_record` — one corpus row.
- `upsert_session_row` — ensures a `pty_sessions` row for an ingested transcript (sentinel `config_json='{}'`, `status='completed'`, `source='ingested'`).
- `harvest_correlation` — scans `mcp__lumina__claim_next_task` records to derive the correlation hints: `sprint_id`/`agent_id` are **last-wins by ordinal**; `task_id` comes from the highest-ordinal SUCCESSFUL claim result, paired by `tool_use_id` so a later `complete_task` does not change attribution.
- `ingest_transcript` — the orchestrator: **drop-if-no-lumina** (a transcript with no `mcp__lumina__*` call persists NOTHING), chunked idempotent txns (re-ingest collapses on the `UNIQUE(session_id, dedup_key)` constraint), and AT MOST ONE coarse export-inert `session.ingested` event per ingest — emitted in its OWN final post-loop txn and ONLY when net-new corpus rows actually landed (its payload `records` is that net-new count). A re-ingest that collapses entirely (zero net-new rows) writes NO event, so repeated (re)ingests can never accumulate never-drained export-inert outbox rows.
- `record_inert_event`'s inert-aggregate vocab is widened to include **`session`** (now run / sprint / finding / batch / **session**) — these events are NEVER git-exported (export renders only `work_item` aggregates).

### Spawned-session parity (`pty/spawn.rs`)

A cheap broadcast DRAINER forwards every record into an unbounded buffer that a batched WRITER persists into `session_records`, so spawned and ingested sessions reach the same lossless-at-rest corpus WITHOUT the bounded render broadcast ever dropping a corpus line under DB-write backpressure (R9 — the unbounded buffer, not the bounded broadcast, backs the corpus; a `RecvError::Lagged` in the drainer is now near-unreachable but still logged as corpus loss). That writer ALSO folds each drained record into the same single-source `CorrelationAccumulator` the ingest path runs and, at end-of-session, backfills the recovered `sprint_id`/`agent_id` onto the spawned `pty_sessions` row (R3 — uniform correlation with the ingest path; best-effort, since the corpus rows are already durable). A spawned session is ALWAYS captured (the drop-at-ingest asymmetry below applies only to ingested terminal transcripts).

### MCP tool (`get_session_context`)

`get_session_context(work_item_id) → { project_id?, sprint_id?, story_id?, epic_id? }` — read-only; composes the work-item's ancestry walk with its sprint membership. A session calls it at session start against a known `work_item_id` so the resolved correlation ids land in the transcript JSONL, where `harvest_correlation` later picks them up. This is the single tool migration-0015 adds: the surface goes 73 → 74 and the count-invariant test in `mcp/mod.rs` asserts **74**.

### HTTP hook + `lumina init-hooks` CLI

- `POST /api/sessions/ingest` (in `http/sessions.rs`) — the SessionEnd http-hook endpoint; see § HTTP routes for the body shape, the 202 + 4-permit-semaphore back-pressure, the unauthenticated loopback-only posture, and the `~/.claude/projects` `transcript_path` confinement.
- `lumina init-hooks` — a new CLI subcommand that read-modify-merges a SessionEnd http-hook into a project's `.claude/settings.json` (idempotent, never-clobber; default url `http://127.0.0.1:24817/api/sessions/ingest`).

### Drop-at-ingest asymmetry

A TERMINAL (ingested) transcript with NO `mcp__lumina__*` call persists NOTHING (drop-if-no-lumina — it carries no correlation worth keeping). SPAWNED sessions are ALWAYS captured regardless. The corpus is lossless-at-rest; egress-time redaction is layer 3 and deferred.

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
