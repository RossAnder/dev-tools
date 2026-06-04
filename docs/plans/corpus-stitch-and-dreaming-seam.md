# Plan: Corpus stitch/retrieval API & dreaming trigger seam (layer 3)

**Plan path**: docs/plans/corpus-stitch-and-dreaming-seam.md
**Created**: 2026-06-02
**Status**: skeleton (seed — resolve the Open Design Questions and flesh the tasks before `/review-plan` → `/implement`)
**Architecture**: layer 3 of [ADR-0004](../adr/0004-harness-session-corpus.md). Builds on layer 2 (`docs/plans/harness-session-corpus.md`). The dreaming **engine** (the analysis intelligence) is a SEPARATE later plan — this layer builds only the retrieval surface + the trigger seam it will plug into.
> Last revised: 2026-06-04
> Paths updated 2026-06-04 for the joyful-singing-crane refactor: `repo.rs` / `mcp.rs` are now submodule directories. The stitch query + redaction land in a NEW `repo/corpus.rs` submodule (re-exported from `repo/mod.rs`); the corpus/dream HTTP routes in a new `http/corpus.rs` (mounted in `http/mod.rs`); optional read tools in a new `mcp/corpus.rs` family (constructor router-sum + count-invariant in `mcp/mod.rs`). The `export::export_pending` / `POST /export` precedent it mirrors is unsplit (`export.rs`, `http/export.rs`).

## Objective

Expose the **Stitch / retrieval API** that composes captured **Sessions** into sprint/story-level transcripts for analysis, plus an operator/external-cron-callable **dreaming trigger seam** — without building the dreaming engine.

## Constraints

- **Read-mostly** — stitch is a query surface; the dream trigger is a thin seam.
- **Redact on egress** — any transcript content that leaves the box (a dreaming prompt, an adhoc git-export) passes through a redaction pass; the at-rest store stays lossless (layer 2).
- **No reintroduced internal background loop** — the trigger mirrors the operator-triggered export precedent (`export::export_pending` / `POST /export`). "Scheduled" = an EXTERNAL scheduler (OS cron / `/schedule` / CI) hitting the endpoint.
- **Build no engine** — provide the seam (and the redacted extract it would consume); the SQL-metrics and/or LLM pattern pass is deferred.

## Scope

- **In**: a stitch query (`GET /api/corpus?...`) keyed by sprint/story/task/agent/project/time-window/global, returning BOTH a session-bundle (structure preserved) AND a timestamp-interleaved merged transcript; a `form` selector (`raw` | `curated` | `redacted`); the egress redaction pass; the dream trigger seam (`POST /dream` + `lumina dream`) that materialises a redacted stitched extract for the (deferred) engine; SPA view of a stitched sprint/story transcript.
- **Out**: the dreaming engine (pattern analysis, metrics, doc-draft output); auto-committing any documentation; an internal scheduler.
- **Affected areas**: `lumina/src/http/corpus.rs` (new corpus + dream routes, mounted in `http/mod.rs`), `lumina/src/repo/corpus.rs` (new submodule — stitch query + redaction; re-exported from `repo/mod.rs`), `lumina/src/mcp/corpus.rs` (optional read-tool family; constructor router-sum + count-invariant in `mcp/mod.rs`), `lumina/src/cli.rs` / `lumina/src/main.rs` (CLI `lumina dream`, unsplit), `lumina/web/`, `lumina/CLAUDE.md`.

## Resolved decisions (grilling 2026-06-02)

- Stitch returns BOTH bundle + interleaved, queryable by ANY correlation axis (incl. cross-project/global), form-selectable (raw | curated | redacted). Dreaming = substrate + seam now, engine deferred. Trigger = operator/external-cron (`POST /dream` + `lumina dream`), no internal loop. Redaction concentrated on egress.

## Open Design Questions (resolve before fleshing tasks)

1. **Redaction pass** — regex/entropy secret-scanner vs an allowlist of safe record kinds vs a dedicated tool. False-positive tolerance? (Egress-only, so lossy redaction is acceptable here.)
2. **`POST /dream` v1 behaviour** — pure no-op trigger that emits a "ready" event, or does it materialise + return a redacted stitched extract (the engine's future input) even though no engine consumes it yet? (Lean: materialise the redacted extract so the seam is exercised end-to-end.)
3. **Interleave semantics** — strict per-record timestamp merge across sessions, or grouped-by-session-with-time-markers? Tie-breaking on equal timestamps across processes.
4. **Stitch output size** — pagination / streaming for large sprints; caps + a `log`-style "truncated N records" signal.
5. **Dream record** — does a dream pass leave a durable record (table/run) now, or is that deferred with the engine?

## Tasks (skeleton)

- **T1**: stitch query in a new `repo/corpus.rs` submodule (declare `mod corpus; pub use corpus::*;` in `repo/mod.rs`) — resolve a correlation key → matching sessions + records (from layer 2's `session_records`); build bundle + interleaved views.
- **T2**: egress redaction pass (per Q1) — in `repo/corpus.rs`.
- **T3**: `GET /api/corpus` route — axis params + `form` selector; pagination (Q4). New `http/corpus.rs`, mounted via `.merge(corpus::router())` in `http/mod.rs`.
- **T4**: dream trigger seam — `POST /dream` (in `http/corpus.rs`) + `lumina dream` CLI (in `cli.rs`) per Q2; external-cron documented. Mirror `export.rs`'s `export_pending` + `http/export.rs` precedent (both unsplit).
- **T5**: SPA — stitched sprint/story transcript view.
- **T6**: tests — stitch by each axis; interleave order; redaction strips known secret fixtures; dream seam materialises a redacted extract. Co-locate unit tests in `repo/corpus.rs`; the end-to-end thread in `lumina/tests/`.
- **T7**: docs — `lumina/CLAUDE.md` (corpus/dream surface, tool count), note the engine is deferred.

## Verification

- `cargo build` / `cargo nextest run` / `cargo clippy`; `rg` macro gate = 0.
- Stitch returns correct bundle + interleaved for a seeded multi-session sprint; redacted form strips secret fixtures; `POST /dream` produces a redacted extract without an engine.

## Risks

- **Redaction gaps** — egress is the one place secrets can leak (esp. into an LLM dreaming prompt later); a miss leaks. Conservative defaults; test against secret fixtures.
- **Scheduler temptation** — do NOT reintroduce an internal loop; keep the trigger operator/external (ADR-0004).
- **Engine pre-emption** — resist building analysis here; the seam must stay thin so the deferred engine isn't a stub.
- **Shared-shell serialisation (post-refactor)** — this layer adds one new submodule per surface (`repo/corpus.rs`, `http/corpus.rs`, optional `mcp/corpus.rs`), so it edits each of `repo/mod.rs`, `http/mod.rs`, and `mcp/mod.rs` once. T1/T2 both live in `repo/corpus.rs` ⇒ sequential; the HTTP (T3/T4) and mcp tasks touch different shells and can run alongside T1/T2's body once the submodule exists.
