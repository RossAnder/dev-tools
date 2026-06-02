# Plan: Harness session corpus — capture, lossless store & transcript-harvest correlation (layer 2)

**Plan path**: docs/plans/harness-session-corpus.md
**Created**: 2026-06-02
**Status**: skeleton (seed — resolve the Open Design Questions and flesh the tasks before `/review-plan` → `/implement`)
**Architecture**: layer 2 of [ADR-0004](../adr/0004-harness-session-corpus.md). Builds on layer 1 (`docs/plans/repo-clone-path-resolution.md`). The stitch/retrieval API + dreaming seam are layer 3 (`docs/plans/corpus-stitch-and-dreaming-seam.md`); the dreaming *engine* is deferred.
> Last revised: 2026-06-02

## Objective

Capture every harness-controlled `claude` **Session** — terminal-initiated (via a `SessionEnd` hook) and SPA-spawned (existing live tail) — into a durable, **lossless**, cross-project **Corpus**, and recover each session's `{project, sprint, agent, task}` correlation by **harvesting lumina's own MCP tool records from the ingested transcript**.

## Constraints

- **Additive, forward-only** migrations; **runtime sqlx only** (`rg` gate stays 0).
- **Sessions are export-inert** — observations, not work intent; they never join the `+1 work_items / +1 events` invariant (coarse export-inert events only, mirroring the migration-0011 Part-B precedent).
- **`SessionEnd`-only hook** — no per-turn/PreToolUse/PostToolUse hooks; hook does the minimum and lumina ingests async. Hard-close-before-end loss is tolerated.
- **Lossless at rest** — one `session_records` row per JSONL line, verbatim; the curated `pty_messages` is a derived view. **Redaction happens on egress only** (not at rest).
- **Reuse the PTY family** — extend `pty_sessions`; reuse `jsonl_tail` parse + `pty_messages` mapping for the derived view.

## Scope

- **In**: `SessionEnd` hook script + install affordance; `POST /api/sessions/ingest` (async ingest of a `transcript_path`); `pty_sessions` extension (`source`, `sprint_id`, `agent_id`, spawn-only fields nullable); new `session_records` lossless table; widen the ingest to capture currently-dropped record types; idempotent ingest keyed `(session_id, record_uuid)`; transcript-harvest correlation (parse lumina `tool_use`/`tool_result` records → ids); a session-registration MCP tool that returns lumina-minted correlation ids; make the SPA-spawn bridge also write `session_records`.
- **Out**: the stitch/retrieval API + dreaming seam (layer 3); the dreaming engine; secret-redaction scanners (only needed on egress — layer 3); per-machine path layer.
- **Affected areas**: `lumina/migrations/`, `lumina/src/pty/` (`jsonl_tail`, `spawn`, `supervisor`), `lumina/src/repo.rs`, `lumina/src/mcp.rs`, `lumina/src/http/`, `lumina/src/domain.rs`, a hook script (new), `lumina/CLAUDE.md`, `CLAUDE.md`, `claude/plugins/lumina-story-blocks/skills/`.

## Resolved decisions (grilling 2026-06-02)

- Capture = `SessionEnd` hook (terminal) + existing live tail (SPA). Correlation = transcript-harvest, NOT env-injection (no launcher needed). A "harness session" = a transcript containing lumina tool calls. Store = extend `pty_sessions` + new lossless `session_records`; `pty_messages` is the derived render-view. Lossless at rest; redact on egress (egress is layer 3).

## Open Design Questions (resolve before fleshing tasks)

1. **Session-registration tool shape** — does a `/lumina:*` skill call a dedicated `register_session(...) → {session_token, agent_id?}`, or is harvesting the EXISTING `claim_next_task`/planning-tool records (which already carry sprint/agent ids) sufficient? (Lean: harvest existing where possible; add a thin `get_session_context` only if a gap exists.)
2. **Hook install/distribution** — `lumina init-hooks` writing user-level `~/.claude/settings.json`, project `.claude/settings.json`, or documented manual install? Scoping so it only fires for opted-in projects.
3. **`session_records` shape** — raw line as TEXT + parsed `(type, uuid, parent_uuid, ts)` index columns? Dedup key `(session_id, record_uuid)`; how to key records with no uuid (system/meta)?
4. **Harvest robustness** — exact match on lumina tool names (`mcp__lumina__*`); how to handle a session that touched multiple sprints/agents (last-wins? all-of?).
5. **Ingest concurrency** — async task pool vs the existing supervisor; backpressure if many `SessionEnd`s arrive at once.

## Tasks (skeleton — phased)

### Phase 1: Schema & domain
- **T1**: migration — `pty_sessions` add `source`/`sprint_id`/`agent_id`, nullable spawn-only fields; new `session_records` table + indexes.
- **T2**: domain types + row mapping; `SessionSource` enum.

### Phase 2: Ingest path
- **T3**: `POST /api/sessions/ingest` (accepts `{session_id, transcript_path, cwd}`; async).
- **T4**: ingest routine — read JSONL, write lossless `session_records` (incl. currently-dropped types), derive `pty_messages`, idempotent on `(session_id, record_uuid)`.
- **T5**: SPA-spawn bridge also writes `session_records` (uniform losslessness).

### Phase 3: Correlation
- **T6**: transcript-harvest — parse lumina tool records → `{project, sprint, agent}`; cwd→project floor (layer 1); keep/drop decision per discriminator.
- **T7**: task attribution from the claim/complete timeline within the records.
- **T8**: session-registration MCP tool per Q1 (+ count-invariant bump if added).

### Phase 4: Hook & tests
- **T9**: `SessionEnd` hook script + install affordance (Q2).
- **T10**: e2e — ingest a fixture terminal JSONL → lossless rows + derived view + harvested correlation → HTTP read; idempotent re-ingest; non-lumina session dropped.

### Phase 5: Docs
- **T11**: `lumina/CLAUDE.md` (corpus surface, tool count), `CLAUDE.md`, plugin skills note.

## Verification

- `cargo build` / `cargo nextest run` / `cargo clippy`; `rg` macro gate = 0.
- e2e: fixture terminal transcript ingests losslessly, correlates by harvest, renders; re-ingest is idempotent.
- Manual smoke: run a `/lumina:*` command in a terminal, close it, confirm the session + records + correlation appear.

## Risks

- **Hard-close loss** (accepted) — `SessionEnd` may not fire on kill; document it.
- **JSONL schema drift** — reuse `jsonl_tail`'s tolerant `UnknownRaw` path; raw store is drift-proof by construction.
- **Volume** — lossless across all projects grows; retention is keep-forever v1 with a deferred prune knob.
- **Harvest misattribution** — a multi-sprint session (Q4) needs a defined rule.
