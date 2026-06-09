//! Session-corpus correlation HARVEST — the pure, DB-free transcript-parsing half
//! of the session-corpus layer (migration 0015), carved out of `repo/sessions.rs`
//! (review R15). Scans parsed JSONL records for lumina's OWN MCP tool calls
//! (`mcp__lumina__*`) and recovers the `{sprint, agent, task}` correlation a
//! session touched.
//!
//! This module holds NO persistence: it has no `DbClient`/`DbTx` dependency and
//! never touches the database — conceptually it is a sibling of
//! `crate::jsonl_tail::parse` (record parsing), not of the corpus write
//! layer. Both corpus paths feed the ONE harvester here — the batch ingest
//! composer (`super::ingest_transcript`) via [`harvest_correlation`], and the live
//! spawned tail (`crate::pty::spawn::drain_and_persist_corpus`) via
//! [`CorrelationAccumulator::observe`] — so the two paths cannot drift on the
//! harvest rules. `super`'s `pub use harvest::*` re-exports the public surface
//! (`Correlation` / `CorrelationAccumulator` / `harvest_correlation`) at
//! `crate::repo::*`, so existing call sites are unchanged.

use crate::jsonl_tail::{
    AssistantContentBlock, JsonlRecord, JsonlRecordParsed, UserContent, UserContentBlock,
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
    // content is already a structured object/number/bool/null). The common
    // bare-string arm BORROWS the inner `&str` (no eager clone) — we only own
    // lazily in the rare reparse-failure fallback below where the owned
    // `Value::String` is actually needed. The array arm must build an owned
    // `String` (concatenation has no single source slice).
    let text: Option<std::borrow::Cow<'_, str>> = match content {
        serde_json::Value::String(s) => Some(std::borrow::Cow::Borrowed(s)),
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
                Some(std::borrow::Cow::Owned(buf))
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
                serde_json::Value::String(s.into_owned())
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
    // Allocation-free equivalent of `name == short || name == format!("{PREFIX}{short}")`:
    // the bare form, OR the prefix stripped leaving exactly `short`.
    name == short || name.strip_prefix(LUMINA_TOOL_PREFIX) == Some(short)
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

/// Last-wins-by-ordinal slot update: replace `slot` whenever `ordinal` is `>=`
/// the stored ordinal, so the highest-ordinal value wins and a tie keeps the
/// later-visited one. Shared by every correlation tracker.
fn update_max(slot: &mut Option<(i64, String)>, ordinal: i64, value: impl FnOnce() -> String) {
    if slot.as_ref().is_none_or(|(o, _)| ordinal >= *o) {
        *slot = Some((ordinal, value()));
    }
}

/// Incremental correlation harvester — the SINGLE implementation of the harvest
/// rules, shared by both the batch ingest path ([`harvest_correlation`]) and the
/// live spawned-session tail (`crate::pty::spawn::drain_and_persist_corpus`, R3),
/// so the two paths can never drift on what correlation they recover.
///
/// The harvest is naturally two-phase: a record's `tool_use` blocks set
/// `has_lumina`, register the claim/session-context producers, and supply the
/// claim-INPUT sprint/agent; its `tool_result` blocks then attribute task/sprint
/// off the RESULT, but only once the producing `tool_use_id` is known. The two
/// entry points feed those phases differently:
///
///   * [`harvest_correlation`] folds [`Self::observe`] over the slice in a SINGLE
///     in-order pass and, ONLY if a successful result referenced a producer
///     registered LATER (a re-ordered transcript — tracked via
///     `unattributed_result_ids`), runs a second [`Self::observe_tool_results`]
///     pass to attribute that orphan. The common in-order transcript needs just
///     the one pass.
///   * [`Self::observe`] folds BOTH halves of one record in arrival order — the
///     streaming entry point for the live tail, where records arrive in file
///     (ordinal) order so a claim/ctx result's `tool_use` was always registered
///     by an earlier record. It retains nothing per record beyond the small
///     producer map + last-wins slots (no transcript buffering), so a long
///     spawned session costs O(distinct claim/ctx tool_use_ids), not O(lines).
#[derive(Default)]
pub struct CorrelationAccumulator {
    has_lumina: bool,
    sprint_at: Option<(i64, String)>,
    agent_at: Option<(i64, String)>,
    task_at: Option<(i64, String)>,
    /// `get_session_context` sprint fallback — lower priority than a claim input.
    ctx_sprint_at: Option<(i64, String)>,
    /// `tool_use_id` → which correlation-relevant tool produced it.
    producer: std::collections::HashMap<String, ResultProducer>,
    /// `tool_use_id`s of successful (non-error) `tool_result`s that were visited
    /// BEFORE their producer was registered. Recorded by
    /// [`Self::observe_tool_results`] ONLY when called with `track_orphans=true`
    /// (the batch [`harvest_correlation`] pass); consulted ONLY by
    /// [`harvest_correlation`] AFTER its single in-order pass — if any recorded id
    /// has since become a claim/ctx producer (its `tool_use` appeared LATER in the
    /// slice, a re-ordered transcript), a second result-only pass is run to
    /// attribute it. Most recorded ids are UNRELATED tools (a `complete_task`/
    /// `Read`/`Bash` result whose id never becomes a producer) and are correctly
    /// ignored — so the common in-order transcript still takes ONE pass. The
    /// strictly-in-order streaming `observe` path passes `track_orphans=false` and
    /// never records here, so the live-tail accumulator stays O(distinct claim/ctx
    /// ids), never O(lines).
    unattributed_result_ids: Vec<String>,
}

impl CorrelationAccumulator {
    /// Fold one record's `tool_use` blocks (the registration phase): set
    /// `has_lumina`, register claim/session-context producers, and harvest the
    /// claim-INPUT sprint/agent (last-wins by `ordinal`). No-op on a record that
    /// is not a `Known` assistant message.
    fn observe_tool_uses(&mut self, ordinal: i64, parsed: &JsonlRecordParsed) {
        let JsonlRecordParsed::Known {
            record: JsonlRecord::Assistant { message, .. },
            ..
        } = parsed
        else {
            return;
        };
        for block in &message.content {
            let AssistantContentBlock::ToolUse { id, name, input } = block else {
                continue;
            };
            if name.starts_with(LUMINA_TOOL_PREFIX) {
                self.has_lumina = true;
            }
            if is_lumina_tool(name, CLAIM_TOOL) {
                self.producer.insert(id.clone(), ResultProducer::Claim);
                if let Some(s) = input.get("sprint_id").and_then(|v| v.as_str()) {
                    update_max(&mut self.sprint_at, ordinal, || s.to_owned());
                }
                if let Some(a) = input.get("agent_id").and_then(|v| v.as_str()) {
                    update_max(&mut self.agent_at, ordinal, || a.to_owned());
                }
            } else if is_lumina_tool(name, SESSION_CONTEXT_TOOL) {
                self.producer.insert(id.clone(), ResultProducer::SessionContext);
            }
        }
    }

    /// Fold one record's `tool_result` blocks (the attribution phase): attribute
    /// task_id (claim) / sprint fallback (session-context) for a successful
    /// (`is_error = false`) result whose producing `tool_use_id` is ALREADY
    /// registered. A result whose producer is unknown is skipped — in the
    /// two-pass batch entry every producer was registered first, and in the
    /// in-order streaming fold an unknown producer means an unrelated tool (a
    /// `complete_task` result never registers, so it can't hijack attribution).
    /// No-op on a record that is not a `Known` user message with block content.
    fn observe_tool_results(&mut self, ordinal: i64, parsed: &JsonlRecordParsed, track_orphans: bool) {
        let JsonlRecordParsed::Known {
            record: JsonlRecord::User { message, .. },
            ..
        } = parsed
        else {
            return;
        };
        let UserContent::Blocks(blocks) = &message.content else {
            return;
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
            let Some(kind) = self.producer.get(tool_use_id).copied() else {
                // A successful result whose producing `tool_use` is not (yet)
                // registered. Usually an UNRELATED tool (a `complete_task` /
                // `Read` / `Bash` result whose id never becomes a claim/ctx
                // producer) — genuinely unattributable, skip. But it MIGHT be a
                // re-ordered transcript whose claim/ctx `tool_use` appears LATER:
                // record the id so `harvest_correlation` can, after its single
                // pass, check whether it became a producer and (only then) run a
                // second result-only pass to attribute the orphan. ONLY the batch
                // `harvest_correlation` pass records (track_orphans=true); the
                // strictly-in-order streaming `observe` passes false — a result's
                // producer always precedes it there, so an orphan is genuinely
                // unattributable and recording it would grow the live-tail
                // accumulator O(lines) instead of O(distinct claim/ctx ids).
                if track_orphans {
                    self.unattributed_result_ids.push(tool_use_id.clone());
                }
                continue;
            };
            let flattened = flatten_tool_result_content(content);
            match kind {
                ResultProducer::Claim => {
                    if let Some(tid) = extract_claim_task_id(&flattened) {
                        update_max(&mut self.task_at, ordinal, || tid);
                    }
                }
                ResultProducer::SessionContext => {
                    if let Some(s) = flattened.get("sprint_id").and_then(|v| v.as_str()) {
                        update_max(&mut self.ctx_sprint_at, ordinal, || s.to_owned());
                    }
                }
            }
        }
    }

    /// Fold ONE record in arrival order — the streaming entry point for the live
    /// spawned tail. A single JSONL record is either an assistant (`tool_use`s)
    /// or a user (`tool_result`s), never both, so exactly one half does work; the
    /// in-order guarantee means a result's producer was registered by an earlier
    /// record.
    pub fn observe(&mut self, ordinal: i64, parsed: &JsonlRecordParsed) {
        self.observe_tool_uses(ordinal, parsed);
        // `track_orphans=false`: the streaming path is strictly in-order, so a
        // result's producer always precedes it — there are no recoverable orphans,
        // and recording them would grow this accumulator O(lines) over a long
        // spawned session (it must stay O(distinct claim/ctx ids)).
        self.observe_tool_results(ordinal, parsed, false);
    }

    /// Resolve the accumulated [`Correlation`]: a claim-derived sprint wins, else
    /// the `get_session_context` fallback fills it.
    pub fn finish(self) -> Correlation {
        let sprint_id = self
            .sprint_at
            .map(|(_, v)| v)
            .or_else(|| self.ctx_sprint_at.map(|(_, v)| v));
        Correlation {
            has_lumina: self.has_lumina,
            sprint_id,
            agent_id: self.agent_at.map(|(_, v)| v),
            task_id: self.task_at.map(|(_, v)| v),
        }
    }
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
/// Records may appear in any order, so correctness must survive a `tool_result`
/// that lexically PRECEDES its producing `tool_use`. We fold the SINGLE-pass
/// streaming [`Self::observe`] (uses + results per record, in slice order),
/// recording the `tool_use_id` of any successful result whose producer was not
/// yet registered. For the common IN-ORDER transcript every claim/ctx result's
/// producer is already registered, so the single pass is the whole job —
/// identical to the streaming `observe` fold, which is exactly what the
/// single-source guarantee requires. We then run a SECOND result-only pass over
/// the whole slice ONLY IF one of those recorded ids turns out to be a claim/ctx
/// producer registered LATER (a genuinely re-ordered transcript) — recovering
/// precisely the old two-pass behaviour for that case. Recorded ids that never
/// became producers (a `complete_task`/`Read`/`Bash` result, the overwhelming
/// majority) do NOT trigger the second pass, so an ordinary transcript stays
/// single-pass. The second pass is idempotent for already-attributed results
/// because [`update_max`] is `>=` (re-applying the same `(ordinal, value)`
/// replaces it with itself).
///
/// All records are scanned regardless of `isSidechain` (harvest-all).
pub fn harvest_correlation(records: &[(i64, JsonlRecordParsed)]) -> Correlation {
    let mut acc = CorrelationAccumulator::default();
    // Single in-order pass: register producers + harvest inputs + attribute
    // results per record — identical slot updates to the streaming `observe` fold
    // (the single-source guarantee), but with `track_orphans=true` so a re-ordered
    // transcript's orphan result ids are recorded for the conditional pass below.
    for (ordinal, parsed) in records {
        acc.observe_tool_uses(*ordinal, parsed);
        acc.observe_tool_results(*ordinal, parsed, true);
    }
    // Orphan fallback: run the second result-only pass ONLY if some successful
    // result was visited before its producer was registered AND that producer
    // is a claim/ctx tool that appeared LATER in the slice (a re-ordered
    // transcript). An ordinary transcript records only unrelated-tool ids here
    // (never producers) and skips the second pass entirely.
    let needs_orphan_pass = acc
        .unattributed_result_ids
        .iter()
        .any(|id| acc.producer.contains_key(id));
    if needs_orphan_pass {
        for (ordinal, parsed) in records {
            acc.observe_tool_results(*ordinal, parsed, false);
        }
    }
    acc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonl_tail::parse_line;

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

    /// The streaming `CorrelationAccumulator::observe` fold (the spawned-tail
    /// entry point, R3) yields the SAME `Correlation` as the two-pass
    /// `harvest_correlation` (the ingest entry point) for an in-order transcript —
    /// locking the single-source guarantee so the two paths cannot drift.
    #[test]
    fn streaming_observe_matches_batch_harvest() {
        let lines = vec![
            (1, parse_line(&claim_tool_use_line("a1", "tu-1", "sprint-7", "agent-x"))),
            (2, parse_line(&claim_result_line("u1", "tu-1", "task-42"))),
        ];
        let batch = harvest_correlation(&lines);

        let mut acc = CorrelationAccumulator::default();
        for (ordinal, parsed) in &lines {
            acc.observe(*ordinal, parsed);
        }
        let streamed = acc.finish();

        assert_eq!(streamed, batch, "streaming observe == two-pass harvest");
        assert_eq!(streamed.sprint_id.as_deref(), Some("sprint-7"));
        assert_eq!(streamed.agent_id.as_deref(), Some("agent-x"));
        assert_eq!(streamed.task_id.as_deref(), Some("task-42"));
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
}
