//! Harness session-corpus persistence helpers (migration 0015, ADR-0004 layer 2).
//!
//! Two write helpers + the centralised inert-event call for the session-ingest
//! path. Like the `pty_*` CRUD in [`super::pty`], the corpus tables are NOT
//! part of the single-mutation-path `+1 work_items / +1 events` invariant —
//! sessions are export-INERT. Each ingest records exactly ONE coarse,
//! export-inert `events` row (`aggregate_type="session"`) via
//! [`super::events::record_inert_event`]; the git-export drain (`export.rs`)
//! materialises ONLY `aggregate_type="work_item"` events, so a `"session"`
//! event is recorded-but-never-rendered.
//!
//! The per-row write helpers take an in-flight `&mut dyn DbTx` (NOT a
//! `DbClient`) so the T6 ingest composer (`ingest_transcript`) can compose
//! them. The composer is NOT one atomic transaction: it writes in CHUNKED
//! short txns (each `INGEST_CHUNK_ROWS` records committed independently, every
//! insert `ON CONFLICT DO NOTHING` so partial progress is safe and re-ingest is
//! idempotent), with the `pty_sessions` upsert riding the FIRST chunk's txn and
//! the single coarse `session.ingested` event emitted in its OWN final small
//! txn AFTER the chunk loop — and only when net-new corpus rows actually
//! landed. Mirrors the `record_event`/`record_inert_event` contract: the
//! borrowed `&str` params are `.to_owned()`'d before binding so the bound args
//! are owned/`'static`.
//!
//! `pub use sessions::*` in `repo/mod.rs` exposes these at `crate::repo::*`.

use uuid::Uuid;

use crate::args;
use crate::db::DbClient;
use crate::error::AppError;
use crate::pty::jsonl_tail::{
    parse_line, record_index_fields, AssistantContentBlock, JsonlRecord, JsonlRecordParsed,
    SessionRecordIndex, UserContent, UserContentBlock,
};

/// The MCP tool-name prefix that marks lumina's OWN work-item server calls. A
/// transcript carrying ANY `tool_use` whose `name` starts with this prefix is a
/// lumina-correlatable session (`has_lumina = true`).
///
/// The single-hyphen `mcp__lumina-ask__*` ask-server calls deliberately DO NOT
/// match (the ask server is the per-session AUQ mount, not the 73-tool work-item
/// surface): `"mcp__lumina-ask__".starts_with("mcp__lumina__")` is `false`, so a
/// plain `starts_with` is the exact discriminator (see `lumina/CLAUDE.md`).
const LUMINA_TOOL_PREFIX: &str = "mcp__lumina__";

/// The bare `claim_next_task` tool short-name (the MCP wire form is
/// `mcp__lumina__claim_next_task`). Correlation reads sprint/agent off this
/// tool's INPUT and task off its successful RESULT.
const CLAIM_TOOL: &str = "claim_next_task";

/// The bare `get_session_context` tool short-name — an ADDITIONAL correlation
/// signal whose result can FILL fields a claim record didn't (fallback only,
/// never overriding a claim-derived value).
const SESSION_CONTEXT_TOOL: &str = "get_session_context";

/// Correlation recovered from a parsed transcript by [`harvest_correlation`].
///
/// `has_lumina` is the GATE: a transcript with no `mcp__lumina__*` tool_use is
/// dropped by `ingest_transcript` (nothing persists). The three id fields are
/// best-effort and may be `None` even when `has_lumina` is true (a session that
/// called some lumina tool but never `claim_next_task` / `get_session_context`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Correlation {
    /// True iff ANY `tool_use.name` starts with `mcp__lumina__` (excludes the
    /// single-hyphen `mcp__lumina-ask__*` ask-server tools).
    pub has_lumina: bool,
    /// Sprint id, last-wins from the highest-ordinal `claim_next_task` tool_use
    /// INPUT, with a `get_session_context` result as fallback.
    pub sprint_id: Option<String>,
    /// Agent id, last-wins from the highest-ordinal `claim_next_task` tool_use
    /// INPUT (no `get_session_context` fallback — that result carries no agent).
    pub agent_id: Option<String>,
    /// Task id from the LAST (highest-ordinal) SUCCESSFUL `claim_next_task`
    /// tool_result (`is_error = false`). A later `complete_task` does NOT change
    /// this attribution.
    pub task_id: Option<String>,
}

/// Outcome of [`ingest_transcript`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestOutcome {
    /// The transcript carried no `mcp__lumina__*` tool_use — nothing was
    /// persisted (no `pty_sessions` row, no `session_records`, no event).
    Dropped,
    /// The transcript was lumina-correlatable and was ingested.
    Ingested {
        /// The number of NET-NEW `session_records` rows this ingest actually
        /// inserted — summed from each insert's `rows_affected`, so an
        /// idempotent re-ingest (every per-row insert collapsing on the
        /// `ON CONFLICT` no-op) reports `0`. This is also the count carried in
        /// the coarse `session.ingested` event, which is emitted ONLY when this
        /// value is `> 0`.
        records_inserted: usize,
        /// The harvested correlation.
        correlation: Correlation,
    },
}

/// Batch size for the per-line `session_records` inserts. A multi-thousand-line
/// transcript is chunked into short txns of this many rows EACH (each insert is
/// `ON CONFLICT DO NOTHING`, so partial progress is safe and re-ingest is
/// idempotent) rather than one unbounded `BEGIN IMMEDIATE` that would hold the
/// RESERVED writer lock past the 5s busy_timeout (P12 merged finding). The
/// pty_sessions upsert + the single coarse inert event ride the FIRST chunk's
/// txn; the inert event stays COARSE — exactly one per ingest.
const INGEST_CHUNK_ROWS: usize = 500;

/// Maximum transcript size (bytes) `ingest_transcript` will read into memory.
///
/// 64 MiB is FAR above any real `claude` session JSONL (the largest observed
/// transcripts are single-digit MiB), so this never rejects a legitimate
/// ingest — it is purely a DoS/OOM ceiling on a hostile or corrupt file placed
/// under the confined projects root. The composer stats the file FIRST and
/// refuses (without reading) when the length exceeds this cap.
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

/// Derive the per-line corpus `dedup_key` from the best-effort record index,
/// the SINGLE source of truth shared by the ingest path
/// ([`ingest_transcript`]) and the live-tail spawned consumer
/// (`crate::pty::spawn`), so the two paths can never drift on the key scheme.
///
/// The two key namespaces are PREFIXED so they cannot collide: a record's own
/// `record_uuid` (when present) yields `u:<uuid>`, while a record with no uuid
/// falls back to the synthetic `o:<ordinal>` (the lossless-at-rest contract: a
/// record with no uuid still gets a stable per-line key). Without the `u:` /
/// `o:` prefixes a record whose uuid was literally e.g. `o:5` could collide
/// with line-5's synthetic key.
pub fn corpus_dedup_key(index: &SessionRecordIndex, ordinal: i64) -> String {
    match &index.record_uuid {
        Some(uuid) => format!("u:{uuid}"),
        None => format!("o:{ordinal}"),
    }
}

/// The VERBATIM raw JSONL line carried on either parsed variant — that string
/// IS the lossless-at-rest corpus payload. Shared by the ingest path and the
/// spawned consumer so the raw-extraction match lives in exactly one place.
pub(crate) fn corpus_raw(parsed: &JsonlRecordParsed) -> &str {
    match parsed {
        JsonlRecordParsed::Known { raw, .. } | JsonlRecordParsed::UnknownRaw { raw, .. } => {
            raw.as_str()
        }
    }
}

/// Render a Rust-side ISO-8601 timestamp string for the corpus TEXT timestamp
/// columns. Matches the convention used across the `pty_*` write paths
/// (`jiff::Timestamp::now().to_string()`, see `repo/pty.rs::now_string` and
/// `export.rs`); the `session_records.created_at` / `pty_sessions` timestamp
/// columns are NOT NULL with no SQL default, so the Rust side supplies them.
fn now_string() -> String {
    jiff::Timestamp::now().to_string()
}

/// Insert one verbatim `session_records` row (migration 0015) for a single
/// ingested JSONL line, idempotent on re-harvest: `ON CONFLICT(session_id,
/// dedup_key) DO NOTHING`, so re-ingesting the same content-keyed line is a
/// no-op (the second insert affects zero rows and is NOT an error).
///
/// Column mapping:
///   * `id` — a fresh UUIDv7 TEXT (the repo's PK convention).
///   * `session_id` — the owning `pty_sessions.id` (FK).
///   * `line_ordinal` — the 0-based position of this line within the session's
///     JSONL (the caller's running counter).
///   * `record_type`/`record_uuid`/`parent_uuid`/`ts` — the best-effort index
///     fields off `index`; each `Option` binds to NULL when absent.
///   * `is_sidechain` — `index.is_sidechain` is a `bool`, but the SQL column is
///     INTEGER (and the read-struct field is `i64`), so it is bound as `0`/`1`
///     via `index.is_sidechain as i64`.
///   * `raw` — the VERBATIM JSONL line, stored unmodified (lossless-at-rest).
///   * `dedup_key` — the content-derived idempotency key (the caller computes
///     it; this fn never derives it).
///   * `created_at` — the ingest timestamp ([`now_string`]).
///
/// This helper records NO event — the single coarse `session.ingested` event
/// for an ingest batch is recorded once by [`record_session_ingested_event`],
/// NOT per row, so a multi-thousand-line transcript does not emit a matching
/// flood of outbox rows.
///
/// Returns the number of rows actually inserted: `1` on a fresh row, `0` when
/// the `ON CONFLICT(session_id, dedup_key) DO NOTHING` collapsed a duplicate.
/// The ingest composer sums these to derive the NET-NEW row count (and to gate
/// the coarse event on net-new `> 0`).
pub async fn insert_session_record(
    tx: &mut dyn crate::db::DbTx,
    session_id: &str,
    ordinal: i64,
    raw: &str,
    index: &SessionRecordIndex,
    dedup_key: &str,
) -> Result<u64, AppError> {
    let id = Uuid::now_v7().to_string();
    let created_at = now_string();

    let rows_affected = tx.execute(
        r#"
        INSERT INTO session_records (
            id, session_id, line_ordinal, record_type, record_uuid,
            parent_uuid, ts, is_sidechain, raw, dedup_key, created_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        ON CONFLICT(session_id, dedup_key) DO NOTHING
        "#,
        args![
            id,
            session_id.to_owned(),
            ordinal,
            index.record_type.clone(),
            index.record_uuid.clone(),
            index.parent_uuid.clone(),
            index.ts.clone(),
            // `is_sidechain: bool` on the index → 0/1 for the INTEGER column.
            index.is_sidechain as i64,
            raw.to_owned(),
            dedup_key.to_owned(),
            created_at
        ],
    )
    .await?;

    Ok(rows_affected)
}

/// Upsert the `pty_sessions` row for an ingested (or pre-existing spawned)
/// session, idempotent on the PK: `ON CONFLICT(id) DO NOTHING`.
///
/// For an `source='ingested'` row the sentinel shape is fixed:
/// `config_json='{}'` (the ingest carries no spawn config), `status='completed'`
/// (a harvested transcript is a finished session), and `parse_strategy_version=1`.
/// `started_at` is supplied by the caller (harvested from the transcript);
/// `updated_at` is set equal to `started_at`; `ended_at` is the caller's
/// optional terminal timestamp.
///
/// The `ON CONFLICT(id) DO NOTHING` is deliberate: it NEVER clobbers an existing
/// row. If lumina already spawned this session (a live `source='spawned'` row),
/// the upsert is a no-op and the spawned row's lifecycle columns are preserved —
/// but the caller (T6) may still backfill that session's `session_records` via
/// [`insert_session_record`], since the corpus rows are keyed on
/// `(session_id, dedup_key)` independently of this row.
///
/// IMPORTANT — `project_id` validation is the CALLER's responsibility. The
/// `pty_sessions_project_kind_check_insert` trigger (migration 0008) `RAISE(ABORT)`s
/// the WHOLE transaction if `project_id` is set but does not reference a live
/// `kind='project'` work_item. This helper does NOT resolve or validate
/// `project_id` (T6's `harvest_correlation` does the cwd→project resolution);
/// pass only an already-validated `project_id` or `None`. `sprint_id`/`agent_id`
/// are harvested correlation columns and bind as NULL when absent.
///
/// `cwd` is stored LEXICALLY (raw) — do NOT route it through
/// `resolve_and_validate_cwd`; an ingested session's cwd is a historical fact
/// from the transcript, not a directory to validate-and-create here.
///
/// Records NO event itself — see [`record_session_ingested_event`].
#[allow(clippy::too_many_arguments)]
pub async fn upsert_session_row(
    tx: &mut dyn crate::db::DbTx,
    id: &str,
    source: &str,
    cwd: &str,
    project_id: Option<&str>,
    sprint_id: Option<&str>,
    agent_id: Option<&str>,
    started_at: &str,
    ended_at: Option<&str>,
) -> Result<(), AppError> {
    tx.execute(
        r#"
        INSERT INTO pty_sessions (
            id, cwd, config_json, parse_strategy_version, status,
            started_at, updated_at, ended_at, source, sprint_id, agent_id, project_id
        )
        VALUES ($1, $2, '{}', 1, 'completed', $3, $3, $4, $5, $6, $7, $8)
        ON CONFLICT(id) DO NOTHING
        "#,
        args![
            id.to_owned(),
            cwd.to_owned(),
            started_at.to_owned(),
            ended_at.map(|s| s.to_owned()),
            source.to_owned(),
            sprint_id.map(|s| s.to_owned()),
            agent_id.map(|s| s.to_owned()),
            project_id.map(|s| s.to_owned())
        ],
    )
    .await?;

    Ok(())
}

/// Record the ONE coarse, export-inert `session.ingested` event for an ingest,
/// centralising the inert-event contract for the session path. Delegates to
/// [`super::events::record_inert_event`] with `aggregate_type="session"` (the
/// guard there rejects only `"work_item"`, and the export drain never renders a
/// `"session"` aggregate). The `aggregate_id` is the session id; `payload` is a
/// small JSON summary the caller (T6) supplies.
pub async fn record_session_ingested_event(
    tx: &mut dyn crate::db::DbTx,
    session_id: &str,
    payload: serde_json::Value,
) -> Result<(), AppError> {
    super::events::record_inert_event(tx, "session", session_id, "session.ingested", payload).await
}

// ---------------------------------------------------------------------------
// Correlation harvest (T6) — scan parsed records for lumina's own MCP tool
// records and recover {sprint, agent, task}.
// ---------------------------------------------------------------------------

/// Flatten a `tool_result` `content` JSON `Value` into a single owned payload
/// `Value`, peeling the empirically-observed layers (research note + plan §4):
///
///   * `content` may be a BARE JSON STRING (`"…"`) — the common shape.
///   * OR an ARRAY of content blocks `[{type:"text", text:"…"}, …]` — concatenate
///     every block's `text`.
///   * OR already a JSON object/other Value — taken as-is.
///
/// The extracted text is then re-parsed ONCE MORE (`from_str`) because the MCP
/// tool return is itself a JSON-ENCODED STRING (the tool serialises its result
/// object to a string, which Claude Code stores as the `tool_result` content).
/// If that re-parse fails, the raw string is wrapped as a JSON string Value so
/// the caller still gets a `Value` to probe (and simply finds no `task_id`).
///
/// Defensive throughout: any layer that doesn't match falls through to a
/// best-effort Value — this never panics and never errors.
fn flatten_tool_result_content(content: &serde_json::Value) -> serde_json::Value {
    // Step 1: reduce to a single text string (or fall straight through if the
    // content is already a structured object/number/bool/null).
    let text: Option<String> = match content {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Array(items) => {
            // Concatenate the `text` of every `{type:"text", text:"…"}` block.
            // NOTE this is a best-effort recovery: if the result was split
            // across multiple text blocks in a way that does not reconstruct
            // the original JSON when naively concatenated, the downstream
            // `from_str` reparse simply fails and the task_id is not recovered
            // (the decode-fail branch below logs that gap).
            let mut buf = String::new();
            for item in items {
                if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                    buf.push_str(t);
                }
            }
            if buf.is_empty() {
                None
            } else {
                Some(buf)
            }
        }
        // Already a non-string Value (object/number/etc.) — probe it directly.
        other => return other.clone(),
    };

    match text {
        // Step 2: the inner text is itself a JSON-encoded string — parse once
        // more to recover the result object. On failure, keep the raw string as
        // a Value (the caller simply won't find a `task_id` in it) and emit a
        // debug diagnostic so an operator can distinguish "no claim at all"
        // from "claim result shape changed and the reparse silently failed".
        Some(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "session harvest: tool_result inner text did not reparse as JSON — \
                     correlation may be missed for this record"
                );
                serde_json::Value::String(s)
            }
        },
        None => serde_json::Value::Null,
    }
}

/// Pull a `task_id` out of a flattened `claim_next_task` result Value, being
/// defensive about where the claim object sits: the MCP surface wraps the
/// `ClaimedTask` as `{ "claimed": { "task_id": "…", … } }`, but a bare
/// `{ "task_id": "…" }` is also accepted. A `claimed: null` (no candidate) or a
/// missing `task_id` yields `None`.
fn extract_claim_task_id(flattened: &serde_json::Value) -> Option<String> {
    // Prefer the nested `claimed.task_id` (the MCP `ClaimedTask` wrapper shape).
    if let Some(tid) = flattened
        .get("claimed")
        .and_then(|c| c.get("task_id"))
        .and_then(|t| t.as_str())
    {
        return Some(tid.to_owned());
    }
    // Fall back to a top-level `task_id`.
    flattened
        .get("task_id")
        .and_then(|t| t.as_str())
        .map(str::to_owned)
}

/// True iff `name` is the bare or `mcp__lumina__`-prefixed form of `short`.
fn is_lumina_tool(name: &str, short: &str) -> bool {
    name == short || name == format!("{LUMINA_TOOL_PREFIX}{short}")
}

/// Which lumina tool a given `tool_use_id` belongs to, recorded from the
/// `tool_use` so its later `tool_result` can be attributed correctly. We only
/// track the two tools whose RESULTS matter to correlation; every other
/// `tool_use_id` is absent from the map and its result is ignored for
/// task/sprint attribution (this is what keeps a `complete_task` result from
/// hijacking the claim-derived task_id — plan §4).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultProducer {
    Claim,
    SessionContext,
}

/// Scan a slice of `(ordinal, parsed-record)` for lumina's own MCP tool records
/// and recover the correlation tuple. See [`Correlation`] for the field
/// contract; the precise harvest rules:
///
///   * `has_lumina` — ANY `tool_use.name` that `starts_with("mcp__lumina__")`.
///   * `sprint_id` / `agent_id` — last-wins by ordinal from the `claim_next_task`
///     tool_use INPUT (the highest-ordinal claim's input fields win). Read from
///     the input directly.
///   * `task_id` — from the LAST (highest-ordinal) SUCCESSFUL `claim_next_task`
///     tool_result (`is_error = false`). A result is attributed to a claim ONLY
///     when its `tool_use_id` was registered by a `claim_next_task` `tool_use`
///     (the name→id pairing) — so a `complete_task` result (which also carries a
///     `task_id`) does NOT change attribution.
///   * `get_session_context` results FILL `sprint_id` (fallback only — never
///     override a claim-derived value), again gated by the `tool_use_id` pairing.
///
/// Records may appear in any order: we do a FIRST pass to register every
/// `tool_use_id`→producer pairing (and harvest the claim inputs), then a SECOND
/// pass to attribute the results — so a result that lexically precedes its
/// `tool_use` (a malformed/re-ordered transcript) is still paired correctly.
///
/// All records are scanned regardless of `isSidechain` (harvest-all).
pub fn harvest_correlation(records: &[(i64, JsonlRecordParsed)]) -> Correlation {
    let mut has_lumina = false;

    // Last-wins-by-ordinal trackers. We keep the (ordinal, value) so a record
    // visited out of order still resolves to the highest ordinal.
    let mut sprint_at: Option<(i64, String)> = None;
    let mut agent_at: Option<(i64, String)> = None;
    let mut task_at: Option<(i64, String)> = None;
    // get_session_context fallbacks (lower priority than the claim-derived ones).
    let mut ctx_sprint_at: Option<(i64, String)> = None;

    // tool_use_id → which correlation-relevant tool produced it (claim / ctx).
    let mut producer: std::collections::HashMap<String, ResultProducer> =
        std::collections::HashMap::new();

    let update_max = |slot: &mut Option<(i64, String)>, ordinal: i64, value: String| {
        if slot.as_ref().is_none_or(|(o, _)| ordinal >= *o) {
            *slot = Some((ordinal, value));
        }
    };

    // ---- Pass 1: tool_use blocks → has_lumina, claim inputs, id→producer map.
    for (ordinal, parsed) in records {
        let JsonlRecordParsed::Known {
            record: JsonlRecord::Assistant { message, .. },
            ..
        } = parsed
        else {
            continue;
        };
        for block in &message.content {
            let AssistantContentBlock::ToolUse { id, name, input } = block else {
                continue;
            };
            if name.starts_with(LUMINA_TOOL_PREFIX) {
                has_lumina = true;
            }
            if is_lumina_tool(name, CLAIM_TOOL) {
                producer.insert(id.clone(), ResultProducer::Claim);
                if let Some(s) = input.get("sprint_id").and_then(|v| v.as_str()) {
                    update_max(&mut sprint_at, *ordinal, s.to_owned());
                }
                if let Some(a) = input.get("agent_id").and_then(|v| v.as_str()) {
                    update_max(&mut agent_at, *ordinal, a.to_owned());
                }
            } else if is_lumina_tool(name, SESSION_CONTEXT_TOOL) {
                producer.insert(id.clone(), ResultProducer::SessionContext);
            }
        }
    }

    // ---- Pass 2: tool_result blocks → attribute task_id (claim) + sprint
    // fallback (session-context), gated by the id→producer pairing.
    for (ordinal, parsed) in records {
        let JsonlRecordParsed::Known {
            record: JsonlRecord::User { message, .. },
            ..
        } = parsed
        else {
            continue;
        };
        let UserContent::Blocks(blocks) = &message.content else {
            continue;
        };
        for block in blocks {
            let UserContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } = block
            else {
                continue;
            };
            if *is_error {
                continue;
            }
            let Some(kind) = producer.get(tool_use_id).copied() else {
                // Not a claim/session-context result — irrelevant to correlation.
                continue;
            };
            let flattened = flatten_tool_result_content(content);
            match kind {
                ResultProducer::Claim => {
                    if let Some(tid) = extract_claim_task_id(&flattened) {
                        update_max(&mut task_at, *ordinal, tid);
                    }
                }
                ResultProducer::SessionContext => {
                    if let Some(s) = flattened.get("sprint_id").and_then(|v| v.as_str()) {
                        update_max(&mut ctx_sprint_at, *ordinal, s.to_owned());
                    }
                }
            }
        }
    }

    // sprint_id: claim-derived wins; fall back to the get_session_context signal.
    let sprint_id = sprint_at
        .map(|(_, v)| v)
        .or_else(|| ctx_sprint_at.map(|(_, v)| v));

    Correlation {
        has_lumina,
        sprint_id,
        agent_id: agent_at.map(|(_, v)| v),
        task_id: task_at.map(|(_, v)| v),
    }
}

// ---------------------------------------------------------------------------
// Ingest composer (T6) — read a transcript, harvest, and persist (chunked).
// ---------------------------------------------------------------------------

/// Ingest one harness-session JSONL transcript into the corpus.
///
/// Pipeline:
///   1. Stat `transcript_path` and refuse (without reading) when it exceeds
///      [`MAX_TRANSCRIPT_BYTES`]; otherwise read it (UTF-8) and split into
///      NON-EMPTY lines via the SHARED [`crate::pty::jsonl_tail::is_corpus_blank`]
///      predicate, enumerated 1-BASED (the T4 ordinal contract — identical to
///      the live tail, so dedup keys match between live-tail and ingest).
///   2. Parse each line via [`parse_line`] and [`harvest_correlation`] over the
///      `(ordinal, parsed)` slice.
///   3. If `!correlation.has_lumina` → return [`IngestOutcome::Dropped`] and
///      PERSIST NOTHING (no `pty_sessions` row, no records, no event).
///   4. Else resolve the project floor via [`super::resolve_cwd_to_project`]
///      (may be `None` — binds NULL), then WRITE in CHUNKED short txns: the first
///      chunk's txn carries the `pty_sessions` upsert (the `session_records` FK
///      needs it) + that chunk's per-line record inserts; subsequent chunks each
///      carry only their per-line record inserts. Every insert is
///      `ON CONFLICT DO NOTHING`, so re-calling this fn inserts ZERO new
///      `session_records` (idempotent).
///   5. AFTER the chunk loop, in its OWN final small txn, record the ONE coarse
///      export-inert `session.ingested` event — but ONLY when net-new corpus
///      rows actually landed (summed from each insert's `rows_affected`). A
///      re-ingest that inserts zero new rows therefore writes NO event, so
///      repeated (re)ingests cannot accumulate never-drained export-inert rows.
///
/// `dedup_key` is derived by the shared [`corpus_dedup_key`] helper (the
/// record's `record_uuid` namespaced `u:<uuid>`, else the synthetic
/// `o:<ordinal>`), identical to the spawned consumer in `crate::pty::spawn`.
///
/// SECURITY CONTRACT: the CALLER MUST confine `transcript_path` to a trusted
/// root before calling — this fn does NOT itself sandbox the path. The HTTP
/// caller (`http/sessions.rs`) does so via `confine_transcript_path` (canonicalise
/// + `starts_with` the `~/.claude/projects` root, rejecting `..`/symlink escape).
/// A FUTURE caller MUST uphold the same confinement, or it bypasses the only
/// gate keeping ingest reads inside the projects corpus. (The
/// [`MAX_TRANSCRIPT_BYTES`] cap here is a DoS ceiling, NOT a substitute for that
/// confinement.)
pub async fn ingest_transcript(
    db: &impl DbClient,
    session_id: &str,
    transcript_path: &str,
    cwd: &str,
) -> Result<IngestOutcome, AppError> {
    // Step 1: stat-then-cap BEFORE reading — refuse an oversized file without
    // pulling it into memory (DoS/OOM ceiling on a hostile file under the
    // confined root; see MAX_TRANSCRIPT_BYTES).
    let meta = tokio::fs::metadata(transcript_path).await.map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "stat transcript '{transcript_path}': {e}"
        ))
    })?;
    if meta.len() > MAX_TRANSCRIPT_BYTES {
        return Err(AppError::Validation(format!(
            "transcript '{transcript_path}' is {} bytes, exceeding the {MAX_TRANSCRIPT_BYTES}-byte ingest cap",
            meta.len()
        )));
    }

    let body = tokio::fs::read_to_string(transcript_path)
        .await
        .map_err(|e| {
            AppError::Other(anyhow::anyhow!(
                "reading transcript '{transcript_path}': {e}"
            ))
        })?;

    // Parse only the NON-EMPTY lines; the 1-based ordinal counts ONLY those
    // lines. The filter uses the SHARED `is_corpus_blank` predicate so this
    // path and the live tail agree on exactly which lines advance the ordinal
    // (a whitespace-only line counts on BOTH paths).
    let parsed: Vec<(i64, JsonlRecordParsed)> = body
        .lines()
        .filter(|l| !crate::pty::jsonl_tail::is_corpus_blank(l))
        .enumerate()
        .map(|(i, line)| ((i as i64) + 1, parse_line(line)))
        .collect();

    // Step 2/3: harvest; drop if no lumina tool call.
    let correlation = harvest_correlation(&parsed);
    if !correlation.has_lumina {
        return Ok(IngestOutcome::Dropped);
    }

    // Step 4: resolve the project floor (may be None → NULL).
    let project_id = super::resolve_cwd_to_project(db, cwd).await?;

    let started_at = now_string();

    // CHUNKED writes. The FIRST chunk's txn also carries the pty_sessions upsert
    // (the session_records FK needs it); later chunks carry only record inserts.
    // Each insert is ON CONFLICT DO NOTHING, so partial progress is safe and the
    // whole ingest is idempotent on re-call. We sum each insert's rows_affected
    // to derive the NET-NEW count: a re-ingest collapses every insert and lands
    // zero new rows, so it must NOT re-emit the coarse event (R5/R6).
    let mut net_new: u64 = 0;
    let mut first_chunk = true;
    for chunk in parsed.chunks(INGEST_CHUNK_ROWS) {
        let mut tx = db.begin().await?;

        if first_chunk {
            upsert_session_row(
                tx.as_mut(),
                session_id,
                "ingested",
                cwd,
                project_id.as_deref(),
                correlation.sprint_id.as_deref(),
                correlation.agent_id.as_deref(),
                &started_at,
                None,
            )
            .await?;
            first_chunk = false;
        }

        for (ordinal, p) in chunk {
            let index = record_index_fields(p);
            let dedup_key = corpus_dedup_key(&index, *ordinal);
            let raw = corpus_raw(p);
            net_new +=
                insert_session_record(tx.as_mut(), session_id, *ordinal, raw, &index, &dedup_key)
                    .await?;
        }

        tx.commit().await?;
    }

    // The ONE coarse, export-inert `session.ingested` event — emitted AFTER the
    // chunk loop in its own final small txn, and ONLY when net-new corpus rows
    // actually landed. A re-ingest (net_new == 0) writes no event, so repeated
    // (re)ingests can never accumulate never-drained export-inert outbox rows.
    if net_new > 0 {
        let mut tx = db.begin().await?;
        record_session_ingested_event(
            tx.as_mut(),
            session_id,
            serde_json::json!({
                "records": net_new,
                "has_lumina": true,
            }),
        )
        .await?;
        tx.commit().await?;
    }

    // Note: `parsed` is always non-empty here (has_lumina requires a parsed
    // tool_use, so the chunk loop ran and the pty_sessions row was upserted);
    // the `!has_lumina` early-return above handles the empty-transcript case.

    Ok(IngestOutcome::Ingested {
        records_inserted: net_new as usize,
        correlation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{connect_in_memory, scalar_one, AnyPool, DbClient};

    /// Seed a bare `pty_sessions` row directly so `session_records`'s FK
    /// (`session_id REFERENCES pty_sessions(id)`) is satisfiable, returning the
    /// session id. Uses [`upsert_session_row`] itself (project_id=None, so the
    /// kind-check trigger is a vacuous no-op).
    async fn seed_session(db: &impl DbClient, id: &str) {
        let mut tx = db.begin().await.expect("begin");
        upsert_session_row(
            tx.as_mut(),
            id,
            "ingested",
            "/dev/proj",
            None,
            None,
            None,
            "2026-06-05T00:00:00Z",
            None,
        )
        .await
        .expect("seed session row");
        tx.commit().await.expect("commit seed");
    }

    /// A duplicate `(session_id, dedup_key)` insert is a no-op: the second
    /// `insert_session_record` with the same key leaves exactly ONE row (the
    /// `ON CONFLICT(session_id, dedup_key) DO NOTHING` collapse).
    #[tokio::test]
    async fn duplicate_session_record_is_noop() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_session(&db, "sess-1").await;

        let index = SessionRecordIndex {
            record_type: Some("user".to_string()),
            record_uuid: Some("u1".to_string()),
            parent_uuid: None,
            ts: Some("2026-06-05T00:00:01Z".to_string()),
            is_sidechain: false,
        };

        // First insert writes the row.
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-1", 0, "{\"x\":1}", &index, "dk-1")
                .await
                .expect("first insert");
            tx.commit().await.expect("commit");
        }

        // Second insert with the SAME (session_id, dedup_key) collapses (no-op).
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-1", 1, "{\"x\":2}", &index, "dk-1")
                .await
                .expect("second insert (conflict no-op)");
            tx.commit().await.expect("commit");
        }

        let count: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1 AND dedup_key = $2",
            args!["sess-1".to_owned(), "dk-1".to_owned()],
        )
        .await
        .expect("count records");
        assert_eq!(count, 1, "duplicate (session_id, dedup_key) collapses to one row");
    }

    /// `is_sidechain: bool` on the index binds as `1`/`0` on the INTEGER column.
    #[tokio::test]
    async fn session_record_binds_is_sidechain_as_int() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        seed_session(&db, "sess-side").await;

        let index = SessionRecordIndex {
            record_type: Some("assistant".to_string()),
            record_uuid: Some("a1".to_string()),
            parent_uuid: Some("u1".to_string()),
            ts: None,
            is_sidechain: true,
        };
        {
            let mut tx = db.begin().await.expect("begin");
            insert_session_record(tx.as_mut(), "sess-side", 0, "{}", &index, "dk-side")
                .await
                .expect("insert");
            tx.commit().await.expect("commit");
        }

        let is_sidechain: i64 = scalar_one(
            &db,
            "SELECT is_sidechain FROM session_records WHERE session_id = $1 AND dedup_key = $2",
            args!["sess-side".to_owned(), "dk-side".to_owned()],
        )
        .await
        .expect("read is_sidechain");
        assert_eq!(is_sidechain, 1, "is_sidechain bool true binds as INTEGER 1");
    }

    /// `upsert_session_row` writes exactly ONE `session`-typed events row when
    /// composed with the centralised inert-event call (the `aggregate_type` is
    /// `"session"`, which the inert guard accepts and the export drain skips).
    #[tokio::test]
    async fn upsert_session_row_writes_one_session_event() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();

        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-ev",
                "ingested",
                "/dev/proj",
                None,
                None,
                None,
                "2026-06-05T00:00:00Z",
                None,
            )
            .await
            .expect("upsert session row");
            // The centralised coarse inert event for the ingest.
            record_session_ingested_event(
                tx.as_mut(),
                "sess-ev",
                serde_json::json!({ "lines": 0 }),
            )
            .await
            .expect("record inert event");
            tx.commit().await.expect("commit");
        }

        // Exactly one `session`-typed event row was written.
        let session_events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = $1",
            args!["session".to_owned()],
        )
        .await
        .expect("count session events");
        assert_eq!(session_events, 1, "exactly one session-typed events row");

        // And it is the `session.ingested` event on the session aggregate.
        let typed: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events \
             WHERE aggregate_type = 'session' AND aggregate_id = $1 \
               AND event_type = 'session.ingested'",
            args!["sess-ev".to_owned()],
        )
        .await
        .expect("count typed event");
        assert_eq!(typed, 1, "the event is session.ingested on the session aggregate");
    }

    /// `ON CONFLICT(id) DO NOTHING` never clobbers an existing row: a second
    /// upsert of the same id with a DIFFERENT source leaves the first source.
    #[tokio::test]
    async fn upsert_session_row_does_not_clobber_existing() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();

        // First upsert: a spawned-shaped row.
        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-keep",
                "spawned",
                "/dev/a",
                None,
                None,
                None,
                "2026-06-05T00:00:00Z",
                None,
            )
            .await
            .expect("first upsert");
            tx.commit().await.expect("commit");
        }

        // Second upsert of the SAME id with a different source/cwd: no-op.
        {
            let mut tx = db.begin().await.expect("begin");
            upsert_session_row(
                tx.as_mut(),
                "sess-keep",
                "ingested",
                "/dev/b",
                None,
                None,
                None,
                "2026-06-05T01:00:00Z",
                None,
            )
            .await
            .expect("second upsert (conflict no-op)");
            tx.commit().await.expect("commit");
        }

        let source: String = scalar_one(
            &db,
            "SELECT source FROM pty_sessions WHERE id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("read source");
        let cwd: String = scalar_one(
            &db,
            "SELECT cwd FROM pty_sessions WHERE id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("read cwd");
        assert_eq!(source, "spawned", "ON CONFLICT(id) DO NOTHING preserved the first source");
        assert_eq!(cwd, "/dev/a", "the original cwd is preserved");
    }

    // =====================================================================
    // T6 — harvest_correlation + ingest_transcript
    // =====================================================================

    /// Build a one-block `assistant` JSONL line carrying a `tool_use` for
    /// `mcp__lumina__claim_next_task` with the given input fields.
    fn claim_tool_use_line(uuid: &str, tool_use_id: &str, sprint: &str, agent: &str) -> String {
        format!(
            r#"{{"type":"assistant","uuid":"{uuid}","message":{{"content":[{{"type":"tool_use","id":"{tool_use_id}","name":"mcp__lumina__claim_next_task","input":{{"sprint_id":"{sprint}","agent_id":"{agent}","lane":"implement"}}}}]}}}}"#
        )
    }

    /// Build a `user` JSONL line carrying a SUCCESSFUL `tool_result` whose
    /// `content` is a bare JSON STRING encoding `{"claimed":{"task_id":...}}` —
    /// the double-encoded shape the harvest must peel (plan §4).
    fn claim_result_line(uuid: &str, tool_use_id: &str, task_id: &str) -> String {
        // The result content is itself a JSON-ENCODED string; embed it as a
        // JSON string value (serde handles the inner escaping for us).
        let inner = serde_json::json!({ "claimed": { "task_id": task_id } }).to_string();
        let content_value = serde_json::Value::String(inner);
        format!(
            r#"{{"type":"user","uuid":"{uuid}","message":{{"content":[{{"type":"tool_result","tool_use_id":"{tool_use_id}","content":{content_value},"is_error":false}}]}}}}"#
        )
    }

    /// (1) A transcript with a claim_next_task pair yields {has_lumina, sprint,
    /// agent, task}.
    #[test]
    fn harvest_yields_full_correlation_from_a_claim_pair() {
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "sprint-7", "agent-x"))),
            (2, parse_line(&claim_result_line("u1", "tu-1", "task-42"))),
        ];
        let c = harvest_correlation(&lines);
        assert!(c.has_lumina, "a mcp__lumina__ tool_use sets has_lumina");
        assert_eq!(c.sprint_id.as_deref(), Some("sprint-7"));
        assert_eq!(c.agent_id.as_deref(), Some("agent-x"));
        assert_eq!(c.task_id.as_deref(), Some("task-42"));
    }

    /// The single-hyphen `mcp__lumina-ask__*` ask-server tool does NOT set
    /// has_lumina (the exact-prefix discriminator).
    #[test]
    fn harvest_excludes_lumina_ask_server() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"t","name":"mcp__lumina-ask__ask_user_question","input":{}}]}}"#;
        let lines = vec![(1, parse_line(line))];
        let c = harvest_correlation(&lines);
        assert!(!c.has_lumina, "mcp__lumina-ask__ must NOT match mcp__lumina__");
    }

    /// (2) TWO successful claim_next_task results at different ordinals → the
    /// HIGHER-ordinal task_id wins (last-wins tie-break), and so do the
    /// higher-ordinal sprint/agent inputs.
    #[test]
    fn harvest_last_wins_by_ordinal() {
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "sprint-A", "agent-1"))),
            (2, parse_line(&claim_result_line("u1", "tu-1", "task-early"))),
            (3, parse_line(&claim_tool_use_line("a2", "tu-2", "sprint-B", "agent-2"))),
            (4, parse_line(&claim_result_line("u2", "tu-2", "task-late"))),
        ];
        let c = harvest_correlation(&lines);
        assert_eq!(c.task_id.as_deref(), Some("task-late"), "highest-ordinal claim wins");
        assert_eq!(c.sprint_id.as_deref(), Some("sprint-B"));
        assert_eq!(c.agent_id.as_deref(), Some("agent-2"));
    }

    /// A later `complete_task` does NOT change task attribution (only a
    /// successful claim_next_task result sets task_id).
    #[test]
    fn harvest_complete_task_does_not_change_attribution() {
        let complete_use = r#"{"type":"assistant","uuid":"a3","message":{"content":[{"type":"tool_use","id":"tu-c","name":"mcp__lumina__complete_task","input":{"task_id":"task-OTHER","agent_id":"agent-1"}}]}}"#;
        let complete_res_inner =
            serde_json::json!({ "task_id": "task-OTHER" }).to_string();
        let complete_res = format!(
            r#"{{"type":"user","uuid":"u3","message":{{"content":[{{"type":"tool_result","tool_use_id":"tu-c","content":{},"is_error":false}}]}}}}"#,
            serde_json::Value::String(complete_res_inner)
        );
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "sprint-1", "agent-1"))),
            (2, parse_line(&claim_result_line("u1", "tu-1", "task-claimed"))),
            (3, parse_line(complete_use)),
            (4, parse_line(&complete_res)),
        ];
        let c = harvest_correlation(&lines);
        // The complete_task result (ordinal 4, tool_use_id "tu-c") carries a
        // top-level `task_id`, but "tu-c" was NOT registered as a claim producer
        // (only claim_next_task tool_uses register their id), so the result is
        // ignored for task attribution. The task_id stays the claim-derived
        // `task-claimed` (ordinal 2) — exactly the plan's "complete does not
        // change attribution" rule.
        assert_eq!(
            c.task_id.as_deref(),
            Some("task-claimed"),
            "complete_task must NOT change the claim-derived task attribution"
        );
    }

    /// A claim_next_task result with `is_error=true` is NOT attributed.
    #[test]
    fn harvest_skips_errored_claim_result() {
        let errored = r#"{"type":"user","uuid":"u1","message":{"content":[{"type":"tool_result","tool_use_id":"tu-1","content":"boom","is_error":true}]}}"#;
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "sprint-1", "agent-1"))),
            (2, parse_line(errored)),
        ];
        let c = harvest_correlation(&lines);
        assert!(c.has_lumina);
        assert_eq!(c.task_id, None, "an errored claim result yields no task_id");
        // sprint/agent still come off the input.
        assert_eq!(c.sprint_id.as_deref(), Some("sprint-1"));
    }

    /// The tool_result content may be an ARRAY of `{type:"text", text}` blocks
    /// whose text is the JSON-encoded result — harvest concatenates + reparses.
    #[test]
    fn harvest_parses_array_text_block_content() {
        let inner = serde_json::json!({ "claimed": { "task_id": "task-arr" } }).to_string();
        // content is an array of one text block carrying the JSON-encoded string.
        let content = serde_json::json!([{ "type": "text", "text": inner }]).to_string();
        let result_line = format!(
            r#"{{"type":"user","uuid":"u1","message":{{"content":[{{"type":"tool_result","tool_use_id":"tu-1","content":{content},"is_error":false}}]}}}}"#
        );
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "s", "ag"))),
            (2, parse_line(&result_line)),
        ];
        let c = harvest_correlation(&lines);
        assert_eq!(c.task_id.as_deref(), Some("task-arr"));
    }

    /// (3) A transcript with no mcp__lumina__ call → has_lumina=false.
    #[test]
    fn harvest_no_lumina_call_is_false() {
        let read_use = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"x"}}]}}"#;
        let user = r#"{"type":"user","uuid":"u","message":{"content":"hi"}}"#;
        let lines = vec![(1, parse_line(read_use)), (2, parse_line(user))];
        let c = harvest_correlation(&lines);
        assert!(!c.has_lumina, "no mcp__lumina__ tool_use ⇒ has_lumina=false");
        assert_eq!(c, Correlation::default());
    }

    /// (3) ingest_transcript on a no-lumina transcript → Dropped, zero rows.
    #[tokio::test]
    async fn ingest_no_lumina_transcript_is_dropped() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        let body = concat!(
            r#"{"type":"user","uuid":"u","message":{"content":"hi"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"t","name":"Read","input":{"file_path":"x"}}]}}"#,
            "\n",
        );
        std::fs::write(&path, body).expect("write transcript");

        let outcome = ingest_transcript(&db, "sess-drop", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("ingest");
        assert_eq!(outcome, IngestOutcome::Dropped, "no-lumina transcript is Dropped");

        // Persisted nothing: no pty_sessions row, no records, no session event.
        let sessions: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM pty_sessions WHERE id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count sessions");
        assert_eq!(sessions, 0, "Dropped persists no pty_sessions row");

        let records: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count records");
        assert_eq!(records, 0, "Dropped persists no session_records");

        let events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-drop".to_owned()],
        )
        .await
        .expect("count events");
        assert_eq!(events, 0, "Dropped persists no session event");
    }

    /// (4) ingest_transcript persists the corpus + correlation, and re-calling it
    /// inserts ZERO new session_records (idempotent).
    #[tokio::test]
    async fn ingest_lumina_transcript_persists_and_is_idempotent() {
        let db: AnyPool = connect_in_memory().await.expect("pool").into();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("session.jsonl");
        // A blank line is interleaved to prove non-empty-line filtering + the
        // 1-based ordinal contract (the blank line does NOT consume an ordinal).
        let body = format!(
            "{}\n\n{}\n",
            claim_tool_use_line("a1", "tu-1", "sprint-9", "agent-z"),
            claim_result_line("u1", "tu-1", "task-99"),
        );
        std::fs::write(&path, &body).expect("write transcript");

        let outcome = ingest_transcript(&db, "sess-keep", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("first ingest");
        let IngestOutcome::Ingested { records_inserted, correlation } = outcome else {
            panic!("expected Ingested, got {outcome:?}");
        };
        assert_eq!(records_inserted, 2, "two non-empty lines ingested (blank skipped)");
        assert_eq!(correlation.sprint_id.as_deref(), Some("sprint-9"));
        assert_eq!(correlation.agent_id.as_deref(), Some("agent-z"));
        assert_eq!(correlation.task_id.as_deref(), Some("task-99"));

        // The pty_sessions row carries the harvested correlation.
        let (src, sprint): (String, Option<String>) = db
            .query_one(
                "SELECT source, sprint_id FROM pty_sessions WHERE id = $1",
                args!["sess-keep".to_owned()],
            )
            .await
            .expect("read session row");
        assert_eq!(src, "ingested");
        assert_eq!(sprint.as_deref(), Some("sprint-9"));

        // Exactly one coarse session.ingested event.
        let events: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("count events");
        assert_eq!(events, 1, "exactly one coarse inert event per ingest");

        let count_records = || async {
            scalar_one::<i64>(
                &db,
                "SELECT COUNT(*) FROM session_records WHERE session_id = $1",
                args!["sess-keep".to_owned()],
            )
            .await
            .expect("count records")
        };
        assert_eq!(count_records().await, 2, "two records persisted");

        // Re-call: the per-line inserts are idempotent — ON CONFLICT(session_id,
        // dedup_key) DO NOTHING + the pty_sessions ON CONFLICT(id) DO NOTHING ⇒
        // ZERO new session_records and no clobber of the existing row. The coarse
        // inert event is now gated on net-new > 0 (R5/R6): a re-ingest that lands
        // zero new corpus rows writes NO new event, so the event count stays at 1.
        let outcome2 = ingest_transcript(&db, "sess-keep", path.to_str().unwrap(), "/dev/proj")
            .await
            .expect("second ingest");
        let IngestOutcome::Ingested { records_inserted: net_new2, .. } = outcome2 else {
            panic!("expected Ingested, got {outcome2:?}");
        };
        assert_eq!(net_new2, 0, "re-ingest reports ZERO net-new rows");
        assert_eq!(count_records().await, 2, "re-ingest inserts ZERO new session_records");

        // And it did NOT re-emit the coarse inert event (still exactly one).
        let events_after_reingest: i64 = scalar_one(
            &db,
            "SELECT COUNT(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = $1",
            args!["sess-keep".to_owned()],
        )
        .await
        .expect("count events after re-ingest");
        assert_eq!(
            events_after_reingest, 1,
            "a zero-net-new re-ingest writes no new session.ingested event"
        );
    }
}
