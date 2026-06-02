# 0004 — Harness session corpus: lossless capture, transcript-harvest correlation, repo-relative paths, dreaming-as-seam

**Status:** accepted (2026-06-02)

## Decision

lumina becomes the durable, **cross-project** store of every harness-controlled `claude` **Session** (one process's JSONL transcript = one Session), captured from both SPA-spawned and terminal-initiated runs, so the **Corpus** can be **Stitched** into sprint/story-level transcripts for later analysis and harness tuning. Six things are fixed:

1. **Capture trigger = `SessionEnd` hook only.** Terminal sessions are captured once, at end, by a minimal `SessionEnd` hook that fires-and-forgets `{session_id, transcript_path, cwd}` to lumina, which ingests the JSONL **asynchronously**. No per-turn / `PreToolUse` / `PostToolUse` hooks (must not tax every interaction). **Accepted loss:** a hard terminal-close before `SessionEnd` runs loses that one session. SPA-spawned sessions keep their existing live JSONL-tail.

2. **Correlation is harvested from the transcript at ingest — not injected.** A `/lumina:*` skill calls a lumina MCP tool that **returns lumina-minted ids** (sprint/agent/…); because each MCP `tool_use` **and** its `tool_result` are recorded verbatim in the JSONL, lumina recovers `{project, sprint, agent, task}` by parsing **its own tool records** in the session it just ingested. `cwd → repo_links.local_path → project` is the always-on floor. **Task** attribution follows the agent's `claim_next_task`/`complete_task` timeline (also in the records). This unifies terminal + SPA on one path and needs no launcher/env-injection.

3. **A "harness session" is defined by content.** A session enters the Corpus with full correlation **iff its transcript contains lumina tool calls**; one with none binds to its project by cwd only and may be dropped. The hook fires indiscriminately; the keep/correlate decision is made at ingest.

4. **Lossless at rest, redact on egress.** The canonical store is a new `session_records` table — **one row per JSONL line, verbatim** (including the records the SPA pipeline drops: `system/turn_duration`, `assistant.message.usage` tokens, `permission-mode`, `file-history-snapshot`, …). The curated `pty_messages` becomes a **derived render-view**. Secrets live at rest at the same trust level as the on-disk JSONL (localhost); redaction is concentrated **only** where transcript content **leaves the box** — a dreaming prompt or an adhoc git-export.

5. **Storage extends the existing PTY family.** `pty_sessions` gains a `source` discriminator (`spawned|ingested`), `sprint_id`/`agent_id` correlation, and spawn-only fields go nullable for ingested rows. A Session stays **export-inert** — an *observation*, not work intent — so it never joins the `+1 work_items / +1 events` invariant. (Glossary calls out the `pty_sessions` runtime-vs-corpus double role.)

6. **Dreaming is built as a seam, not an engine.** v1 ships the Corpus + the **Stitch / retrieval API** (query by sprint/story/task/agent/project/time/global; returns *both* a session-bundle and a timestamp-interleaved transcript; form selectable raw | curated | redacted) + an operator/external-cron-callable trigger (`POST /dream` + `lumina dream`), mirroring the **operator-triggered export** precedent (no reintroduced internal background loop). The analysis *intelligence* (SQL metrics and/or an LLM pattern pass) is a **separate later layer**, per ADR-0002's substrate-vs-engine discipline.

### Path substrate (enabling decision)

`cwd → project` rides on a new **Clone directory**: a nullable `repo_links.local_path` (NULL = "not cloned here") plus a per-machine **Clone root** setting (e.g. `~/dev`) driving an "offer to clone → `<root>/<name>`" action. Repo-relative paths (`files_touched`, finding file:line) resolve to absolute via `local_path + path`. **Deliberately single-machine-now:** the path lives on the *shared* repo-link row, which is ambiguous under a shared-remote lumina + local stubs — that topology would force a per-machine path layer (deferred).

## Considered options

- **Env-injection + launcher wrapper for correlation** (the launcher sets `LUMINA_AGENT_ID`/`LUMINA_SPRINT_ID`; the hook forwards them) — **rejected** in favour of transcript-harvest: it needed a lumina-provided launcher for terminal runs (or silently degraded to project-only when the export was forgotten), and split the terminal vs SPA paths. Harvesting lumina's own recorded tool calls needs neither.
- **Filesystem watcher over `~/.claude/projects/**`** — **rejected**: slurps unrelated sessions, can't correlate beyond cwd, maximises volume + secret surface. The `SessionEnd` hook is targeted and lifecycle-clean.
- **MCP-call-triggered capture** — **rejected**: lumina's MCP layer has no native visibility into the calling session's JSONL id (verified — the `ask_user_question` tool only knows its session because the id is baked into the spawn-time system prompt).
- **Redact on ingest** — **rejected**: lossy, defeats "maximal detail", and scanners both miss and false-positive. Redaction is concentrated on egress instead.
- **Per-machine path layer / machine-local config now** — **deferred**: `local_path` on the repo-link row is the simplest correct thing while one lumina serves one machine; revisit when the shared-remote split actually lands.
- **Full dreaming engine now** — **rejected**: you cannot design good pattern-analysis against an empty corpus; build the substrate, let real transcripts accrue, then design the engine.

## Consequences

- **Three additive, forward-only plans** (layered like ADR-0002): (1) repo clone-directory & path resolution → (2) corpus capture & lossless store + transcript-harvest correlation → (3) stitch/retrieval API + dreaming trigger seam. The dreaming engine is a later, separate plan and must not be stubbed here.
- **Uniform losslessness**: the SPA-spawn live-tail bridge must *also* write raw `session_records` (not just curated `pty_messages`), so spawned and ingested sessions share one canonical store and one render-view derivation.
- **Idempotent ingest** keyed on `(session_id, record_uuid)` — re-ingest (or `need_rescan` re-reads) collapse onto existing rows; this also de-conflicts an SPA-spawned run that the indiscriminate hook also reports (lumina spawns with its own `--session-id`).
- **A new lightweight session-registration MCP tool** returns the lumina-minted correlation ids the `/lumina:*` skills surface (so they land in the transcript for harvest); exact shape is a remaining open question for plan 2.
- **Retention**: keep-forever, no auto-prune in v1; optional compression of old raw rows; a prune knob only if volume bites.
- **Deferred-with-engine**: a dreaming session is itself a captured Session — tag/`source` it so it is excluded from its own input (avoid feedback); redaction-scanner choice; the dream output artifact (records vs doc-draft, never auto-commit).

Glossary: `lumina/CONTEXT.md` § "Observation & analysis plane" (Session, Transcript, Corpus, Stitch, Dreaming, Clone directory, Clone root) + the three new Flagged ambiguities. Layer plans: `docs/plans/repo-clone-path-resolution.md`, `docs/plans/harness-session-corpus.md`, `docs/plans/corpus-stitch-and-dreaming-seam.md`.
