//! `/mcp-ask` — a minimal, single-tool MCP server exposing `ask_user_question`,
//! the lumina-native replacement for claude's built-in AskUserQuestion (AUQ)
//! picker.
//!
//! ## Why this exists
//!
//! lumina drives `claude` interactively under a PTY and reads the transcript
//! from the session JSONL. claude buffers an OPEN native-AUQ `tool_use` out of
//! the JSONL until the question is answered (verified 2.1.156 — see
//! `lumina/CLAUDE.md` § "PTY interaction"), so a JSONL-tailing consumer can
//! never surface an open AUQ. Rather than screen-scrape the TUI picker out of
//! the PTY byte stream (fragile; and the PTY reveals multi-question AUQs one
//! screen at a time, so full fidelity is unreachable that way), lumina gives the
//! agent a STRUCTURED tool it calls instead of the native AUQ. The spawn appends
//! a system prompt steering claude here and registers this server via a
//! per-session `--mcp-config` (`crate::pty::pty_transport`).
//!
//! ## Why a separate mount (not the `/mcp` work-item surface)
//!
//! The 73-tool work-item MCP surface (`crate::mcp`) is NOT exposed to spawned
//! `claude` sessions — a spawned REPL is a general agent, not a planning client.
//! This mount carries ONLY `ask_user_question`, so a spawned session gains
//! exactly the one affordance it needs.
//!
//! ## Flow
//!
//! 1. The agent calls `ask_user_question` with the session id (from its system
//!    prompt) and one or more questions.
//! 2. The handler resolves the PTY [`Session`], registers a per-question
//!    `oneshot` in [`Session::pending_questions`], marks the session
//!    non-quiescent (adds the question id to `outstanding_tool_uses` so the
//!    supervisor won't flip it Idle while the human is deciding), broadcasts a
//!    synthetic `tool_use(AskUserQuestion)` `TypedMessage` (so the EXISTING
//!    `PtyAuqPicker` SPA renders it unchanged), and BLOCKS on the oneshot.
//! 3. The SPA POSTs the answer to `POST /api/pty/sessions/{id}/ask/{qid}/answer`
//!    (`crate::http::pty_sessions`), which fulfils the oneshot, clears the
//!    bookkeeping, and broadcasts a synthetic `tool_result` (closing the picker
//!    card with the answer).
//! 4. The tool returns the user's selections to the agent. On timeout it closes
//!    the picker itself and returns a "no answer" result.
//!
//! ## Session correlation (v1 trade-off)
//!
//! The session is identified by the agent-supplied `session_id` argument,
//! injected into the per-session system prompt. A buggy/hostile agent could
//! target another session's id; acceptable under lumina's localhost trust model
//! (same posture as the rest of `/api`). A future hardening can bind the session
//! out-of-band via a per-session `--mcp-config` header or URL path.

use std::sync::Arc;
use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ErrorData, ServerCapabilities, ServerInfo};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{ServerHandler, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::app::AppState;
use crate::pty::emit;
use crate::pty::protocol::{AskOutcome, AuqAnswer, MessageKind, SessionId, TypedMessage};
use crate::pty::session::Session;

/// How long the tool blocks awaiting an answer before giving up and closing the
/// picker. The per-session `--mcp-config` `timeout` (set by
/// `pty_transport`) is held slightly HIGHER so the server returns its own clean
/// "no answer" result before claude's MCP client kills the call.
const ASK_ANSWER_TIMEOUT: Duration = Duration::from_secs(1800); // 30 min

/// The synthetic-question id prefix. Distinguishes lumina-minted AUQ ids from
/// real claude `tool_use` ids (so they never collide in `outstanding_tool_uses`
/// or the SPA's pairing map).
const QUESTION_ID_PREFIX: &str = "lumina-ask-";

/// The literal label the SPA picker sends for the synthetic "Other" row; when
/// present in `selected_labels`, the real answer rides in `other_text`.
const OTHER_LABEL: &str = "Other";

// ---------------------------------------------------------------------------
// Tool parameter schema
// ---------------------------------------------------------------------------

/// One selectable option. Serialises to the `AuqOption` shape the SPA picker
/// consumes (`{label, description, preview?}`).
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct AskOption {
    /// Short option label (what the user picks).
    pub label: String,
    /// Optional one-line explanation of the option.
    #[serde(default)]
    pub description: String,
    /// Optional monospace preview block (e.g. a code/diff/config snippet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// One question. Serialises (camelCase) to the `AuqQuestion` shape the SPA
/// picker consumes (`{question, header, multiSelect, options}`).
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskQuestion {
    /// The full question text shown to the user.
    pub question: String,
    /// A short chip label (≤ ~12 chars), e.g. "Auth method".
    pub header: String,
    /// Allow multiple selections (checkbox) instead of one (radio).
    #[serde(default)]
    pub multi_select: bool,
    /// 2–4 distinct options. The SPA always offers an extra "Other" free-text
    /// row after these, so do NOT add an "Other" option yourself.
    pub options: Vec<AskOption>,
}

/// Arguments for the `ask_user_question` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AskUserQuestionParams {
    /// The lumina PTY session id to ask in. Use EXACTLY the id given in your
    /// system prompt.
    pub session_id: String,
    /// One or more questions to ask (rendered together in the lumina UI).
    pub questions: Vec<AskQuestion>,
}

// ---------------------------------------------------------------------------
// Tool handler
// ---------------------------------------------------------------------------

/// Single-tool MCP handler for the `/mcp-ask` mount. Holds an [`AppState`] clone
/// (for `pty_registry` + `pool`) plus the generated tool router.
#[derive(Clone)]
pub struct AskTools {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl AskTools {
    /// Construct the handler over a shared [`AppState`].
    pub fn with_state(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    /// Ask the human operator a multiple-choice question in the lumina UI and
    /// block until they answer.
    #[tool(
        description = "Ask the human operator one or more multiple-choice questions and BLOCK until they answer in the lumina UI. Use this INSTEAD of the built-in AskUserQuestion tool whenever you need the user to choose between options or decide between approaches. `session_id` must be exactly the lumina session id from your system prompt. Returns the user's selections (or a 'no answer' note on cancel/timeout).",
        annotations(open_world_hint = false)
    )]
    pub async fn ask_user_question(
        &self,
        Parameters(params): Parameters<AskUserQuestionParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if params.questions.is_empty() {
            return Err(ErrorData::invalid_params(
                "ask_user_question requires at least one question",
                None,
            ));
        }

        // Resolve the live session.
        let uuid = Uuid::parse_str(&params.session_id).map_err(|_| {
            ErrorData::invalid_params(
                format!("session_id {:?} is not a valid uuid", params.session_id),
                None,
            )
        })?;
        let session = self
            .state
            .pty_registry
            .get(&SessionId(uuid))
            .await
            .ok_or_else(|| {
                ErrorData::resource_not_found(
                    format!(
                        "pty session {} is not running (cannot ask a question)",
                        params.session_id
                    ),
                    None,
                )
            })?;

        let question_id = format!("{QUESTION_ID_PREFIX}{}", Uuid::now_v7());

        // Register the answer channel + mark the session non-quiescent BEFORE
        // broadcasting, so an instant answer can't race ahead of registration.
        let (tx, rx) = oneshot::channel::<AskOutcome>();
        session
            .pending_questions
            .lock()
            .await
            .insert(question_id.clone(), tx);
        session
            .outstanding_tool_uses
            .lock()
            .await
            .insert(question_id.clone());

        // Broadcast the synthetic tool_use → the SPA's PtyAuqPicker renders.
        let questions_value = serde_json::to_value(&params.questions).map_err(|e| {
            ErrorData::internal_error(format!("serialise questions: {e}"), None)
        })?;
        let tm = synthetic_tool_use(
            &question_id,
            questions_value,
            jiff::Timestamp::now().to_string(),
        );
        emit::persist_and_broadcast(self.state.pool.sqlite(), &session, tm).await;

        tracing::info!(
            session_id = %params.session_id,
            question_id = %question_id,
            questions = params.questions.len(),
            "ask_user_question: blocking on operator answer"
        );

        // Block until the answer endpoint fulfils the oneshot, or we time out.
        // `biased` polls the answer first so a just-delivered answer always
        // wins a tie with the timeout.
        let outcome = tokio::select! {
            biased;
            res = rx => res.ok(),
            _ = tokio::time::sleep(ASK_ANSWER_TIMEOUT) => None,
        };

        match outcome {
            // Answered / cancelled: the answer endpoint already cleared the
            // bookkeeping and broadcast the closing tool_result.
            Some(AskOutcome::Answered(answers)) => answer_result(&params.questions, &answers),
            Some(AskOutcome::Cancelled) => Ok(cancelled_result()),
            // Timeout or sender dropped (session teardown): close it ourselves.
            None => {
                self.close_unanswered(&session, &question_id).await;
                Ok(timeout_result())
            }
        }
    }
}

impl AskTools {
    /// Tear down a question that was never answered (timeout / session end):
    /// drop the pending entry, clear the quiescence marker, and broadcast a
    /// neutral closing `tool_result` so the SPA picker card resolves.
    async fn close_unanswered(&self, session: &Session, question_id: &str) {
        session.pending_questions.lock().await.remove(question_id);
        session.outstanding_tool_uses.lock().await.remove(question_id);
        let tm = synthetic_tool_result(
            question_id,
            "No answer received — the question timed out or the session ended.".to_string(),
            false,
            jiff::Timestamp::now().to_string(),
        );
        emit::persist_and_broadcast(self.state.pool.sqlite(), session, tm).await;
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for AskTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "lumina ask-user bridge. `ask_user_question` presents structured \
             multiple-choice questions to the human operator in the lumina UI and \
             blocks until they answer. Call it instead of the built-in \
             AskUserQuestion tool.",
        )
    }
}

// ---------------------------------------------------------------------------
// Synthetic message builders (shared with the answer endpoint)
// ---------------------------------------------------------------------------

/// Build the synthetic `tool_use(AskUserQuestion)` that opens the SPA picker.
/// Mirrors the shape `jsonl_tail::map_record_to_typed` produces for a real
/// `tool_use` block, so the SPA's `isAuqToolUse` / `pendingAuq` path renders it
/// unchanged. `questions` is the already-serialised `AuqQuestion[]` value.
pub fn synthetic_tool_use(question_id: &str, questions: Value, now: String) -> TypedMessage {
    TypedMessage {
        sequence: 0,
        kind: MessageKind::ToolUse,
        content: json!({
            "name": "AskUserQuestion",
            "input": { "questions": questions },
            "tool_use_id": question_id,
        }),
        raw_text: None,
        created_at: now,
        tool_use_id: Some(question_id.to_string()),
    }
}

/// Build the synthetic `tool_result` that CLOSES the SPA picker card. The SPA
/// pairs it to the open `tool_use` by `tool_use_id` (== `question_id`).
pub fn synthetic_tool_result(
    question_id: &str,
    output: String,
    is_error: bool,
    now: String,
) -> TypedMessage {
    TypedMessage {
        sequence: 0,
        kind: MessageKind::ToolResult,
        content: json!({
            "tool_use_id": question_id,
            "output": output,
            "is_error": is_error,
        }),
        raw_text: None,
        created_at: now,
        tool_use_id: Some(question_id.to_string()),
    }
}

/// True if a JSONL-derived `tool_use` TypedMessage is a call to THIS module's
/// `ask_user_question` MCP tool (name like `mcp__lumina-ask__ask_user_question`).
///
/// claude logs the raw MCP call to the session JSONL like any other tool. The
/// JSONL bridge (`crate::pty::spawn`) suppresses these tool_use rows AND their
/// matching tool_results, because the tool already broadcasts a synthetic AUQ
/// `tool_use`/`tool_result` pair that drives the SPA picker — surfacing the raw
/// MCP call too would double-render the question. The match is on the
/// distinctive tool name substring (no other tool is named `ask_user_question`),
/// which tolerates the `mcp__<server>__` prefixing without pinning the exact
/// server-name normalisation.
pub fn is_ask_user_question_tool_use(tm: &TypedMessage) -> bool {
    tm.kind == MessageKind::ToolUse
        && tm
            .content
            .get("name")
            .and_then(|n| n.as_str())
            .is_some_and(|n| n.contains("ask_user_question"))
}

/// One-line-per-question human summary of the user's selections, resolving the
/// `"Other"` row to its free text. Used as the closing `tool_result` `output`
/// shown in the SPA's completed picker card (the answer endpoint builds it).
pub fn brief_answer_summary(answers: &[AuqAnswer]) -> String {
    answers
        .iter()
        .map(|a| {
            let sel = resolve_selected(a);
            let body = if sel.is_empty() {
                "(no selection)".to_string()
            } else {
                sel.join(", ")
            };
            format!("Q{}: {body}", a.question_index)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Agent-facing result formatting
// ---------------------------------------------------------------------------

/// Resolve an answer's selected labels, substituting the user's free text for
/// the synthetic `"Other"` row.
fn resolve_selected(a: &AuqAnswer) -> Vec<String> {
    a.selected_labels
        .iter()
        .map(|l| {
            if l == OTHER_LABEL {
                a.other_text
                    .clone()
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| OTHER_LABEL.to_string())
            } else {
                l.clone()
            }
        })
        .collect()
}

/// Build the structured JSON the agent reads back from the tool. Pure (no I/O)
/// so it is unit-testable independent of `CallToolResult`.
fn build_answer_value(questions: &[AskQuestion], answers: &[AuqAnswer]) -> Value {
    let mut rendered = Vec::with_capacity(answers.len());
    let mut summary_parts = Vec::with_capacity(answers.len());
    for ans in answers {
        let q = questions.get(ans.question_index);
        let header = q.map(|q| q.header.clone()).unwrap_or_default();
        let question = q.map(|q| q.question.clone()).unwrap_or_default();
        let selected = resolve_selected(ans);
        let label = if header.is_empty() {
            format!("Q{}", ans.question_index)
        } else {
            header.clone()
        };
        summary_parts.push(format!("{label}={}", selected.join(", ")));
        rendered.push(json!({
            "questionIndex": ans.question_index,
            "header": header,
            "question": question,
            "selected": selected,
            "notes": ans.notes,
        }));
    }
    json!({
        "answers": rendered,
        "summary": summary_parts.join("; "),
    })
}

/// The successful tool result returned to the agent on an answer.
fn answer_result(
    questions: &[AskQuestion],
    answers: &[AuqAnswer],
) -> Result<CallToolResult, ErrorData> {
    let value = build_answer_value(questions, answers);
    let summary = value
        .get("summary")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    Ok(CallToolResult::success(vec![
        Content::text(format!("The user answered: {summary}")),
        Content::json(value)?,
    ]))
}

/// The tool result returned when the user dismissed the picker.
fn cancelled_result() -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        "The user dismissed the question without selecting an option. Proceed without their \
         input, or ask again if you still need a decision.",
    )])
}

/// The tool result returned when no answer arrived before the timeout.
fn timeout_result() -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        "The user did not answer within the time limit. Proceed without their input, or ask \
         again if you still need a decision.",
    )])
}

// ---------------------------------------------------------------------------
// Service builder
// ---------------------------------------------------------------------------

/// Build the single-tool MCP service mounted at `/mcp-ask` by
/// `app::build_router`. Mirrors `mcp::service_with_state`: a per-request factory
/// clones the `AppState` (cheap — all `Arc`s) and builds a fresh [`AskTools`].
/// `allowed_hosts` stays at the rmcp 1.7 loopback default.
pub fn service(state: AppState) -> StreamableHttpService<AskTools, LocalSessionManager> {
    StreamableHttpService::new(
        move || Ok(AskTools::with_state(state.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn q(header: &str, question: &str, labels: &[&str]) -> AskQuestion {
        AskQuestion {
            question: question.to_string(),
            header: header.to_string(),
            multi_select: false,
            options: labels
                .iter()
                .map(|l| AskOption {
                    label: l.to_string(),
                    description: String::new(),
                    preview: None,
                })
                .collect(),
        }
    }

    fn ans(idx: usize, labels: &[&str], other: Option<&str>) -> AuqAnswer {
        AuqAnswer {
            question_index: idx,
            selected_labels: labels.iter().map(|s| s.to_string()).collect(),
            other_text: other.map(String::from),
            notes: None,
        }
    }

    #[test]
    fn synthetic_tool_use_matches_auq_shape() {
        let questions = serde_json::to_value(vec![q("Pick", "Choose one", &["A", "B"])]).unwrap();
        let tm = synthetic_tool_use("lumina-ask-1", questions, "now".into());
        assert_eq!(tm.kind, MessageKind::ToolUse);
        assert_eq!(tm.tool_use_id.as_deref(), Some("lumina-ask-1"));
        assert_eq!(
            tm.content.get("name").and_then(|v| v.as_str()),
            Some("AskUserQuestion")
        );
        // input.questions[0] carries the camelCase multiSelect the SPA expects.
        let first = &tm.content["input"]["questions"][0];
        assert_eq!(first.get("multiSelect").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(first.get("header").and_then(|v| v.as_str()), Some("Pick"));
        assert_eq!(
            tm.content.get("tool_use_id").and_then(|v| v.as_str()),
            Some("lumina-ask-1")
        );
    }

    #[test]
    fn is_ask_tool_use_matches_mcp_prefixed_name_only() {
        let mcp = synthetic_tool_use("q1", serde_json::json!([]), "now".into());
        // synthetic_tool_use uses bare "AskUserQuestion" (the SPA-facing name),
        // which is NOT the raw MCP call name and must NOT be suppressed.
        assert!(!is_ask_user_question_tool_use(&mcp));

        let raw = TypedMessage {
            sequence: 0,
            kind: MessageKind::ToolUse,
            content: serde_json::json!({
                "name": "mcp__lumina-ask__ask_user_question",
                "input": {},
                "tool_use_id": "toolu_real",
            }),
            raw_text: None,
            created_at: "now".into(),
            tool_use_id: Some("toolu_real".into()),
        };
        assert!(is_ask_user_question_tool_use(&raw));

        // A different tool is never suppressed.
        let other = TypedMessage {
            sequence: 0,
            kind: MessageKind::ToolUse,
            content: serde_json::json!({ "name": "Read", "input": {}, "tool_use_id": "t" }),
            raw_text: None,
            created_at: "now".into(),
            tool_use_id: Some("t".into()),
        };
        assert!(!is_ask_user_question_tool_use(&other));
    }

    #[test]
    fn synthetic_tool_result_pairs_by_question_id() {
        let tm = synthetic_tool_result("lumina-ask-9", "done".into(), false, "now".into());
        assert_eq!(tm.kind, MessageKind::ToolResult);
        assert_eq!(tm.tool_use_id.as_deref(), Some("lumina-ask-9"));
        assert_eq!(
            tm.content.get("tool_use_id").and_then(|v| v.as_str()),
            Some("lumina-ask-9")
        );
        assert_eq!(
            tm.content.get("output").and_then(|v| v.as_str()),
            Some("done")
        );
        assert_eq!(
            tm.content.get("is_error").and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn brief_summary_resolves_other_free_text() {
        let answers = vec![
            ans(0, &["Beta"], None),
            ans(1, &["Other"], Some("a custom answer")),
        ];
        let summary = brief_answer_summary(&answers);
        assert_eq!(summary, "Q0: Beta\nQ1: a custom answer");
    }

    #[test]
    fn brief_summary_handles_empty_selection() {
        let answers = vec![ans(0, &[], None)];
        assert_eq!(brief_answer_summary(&answers), "Q0: (no selection)");
    }

    #[test]
    fn agent_value_maps_questions_and_resolves_other() {
        let questions = vec![
            q("Lib", "Which library?", &["serde", "Other"]),
            q("Mode", "Which mode?", &["fast", "safe"]),
        ];
        let answers = vec![ans(0, &["Other"], Some("miniserde")), ans(1, &["safe"], None)];
        let value = build_answer_value(&questions, &answers);

        assert_eq!(
            value["answers"][0]["selected"],
            serde_json::json!(["miniserde"])
        );
        assert_eq!(value["answers"][0]["header"], "Lib");
        assert_eq!(value["answers"][0]["question"], "Which library?");
        assert_eq!(value["answers"][1]["selected"], serde_json::json!(["safe"]));
        assert_eq!(value["summary"], "Lib=miniserde; Mode=safe");
    }

    #[test]
    fn agent_value_multiselect_joins_labels() {
        let questions = vec![q("Feat", "Which features?", &["a", "b", "c"])];
        let answers = vec![ans(0, &["a", "c"], None)];
        let value = build_answer_value(&questions, &answers);
        assert_eq!(value["answers"][0]["selected"], serde_json::json!(["a", "c"]));
        assert_eq!(value["summary"], "Feat=a, c");
    }

    /// End-to-end: the tool broadcasts a synthetic AUQ `tool_use`, registers a
    /// pending question + the quiescence marker, BLOCKS, and returns once an
    /// answer is delivered through the oneshot (as the answer endpoint would).
    #[tokio::test]
    async fn ask_tool_broadcasts_blocks_then_returns_on_answer() {
        use crate::db::{AnyPool, connect_in_memory};
        use crate::pty::session::Session;
        use std::sync::Arc;
        use std::time::Duration;
        use tokio::sync::{broadcast, mpsc};

        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let state = AppState::new(pool);

        let (bcast_tx, mut bcast_rx) = broadcast::channel(16);
        let (input_tx, _input_rx) = mpsc::channel(8);
        let session = Session::new(SessionId::new(), bcast_tx, input_tx);
        let sid = session.id;
        state.pty_registry.insert(session.clone()).await;

        let tools = AskTools::with_state(state.clone());
        let params = AskUserQuestionParams {
            session_id: sid.to_string(),
            questions: vec![q("Pick", "Choose one", &["Alpha", "Beta"])],
        };
        let task = tokio::spawn(async move { tools.ask_user_question(Parameters(params)).await });

        // A synthetic AUQ tool_use is broadcast to the SPA.
        let opened = bcast_rx.recv().await.expect("synthetic tool_use broadcast");
        assert_eq!(opened.kind, MessageKind::ToolUse);
        assert_eq!(
            opened.content.get("name").and_then(|v| v.as_str()),
            Some("AskUserQuestion")
        );
        let qid = opened.tool_use_id.clone().expect("question id");
        // The quiescence marker is set while the question is open.
        assert!(session.outstanding_tool_uses.lock().await.contains(&qid));

        // Deliver an answer through the oneshot (as the answer endpoint does).
        let tx = {
            let mut guard = session.pending_questions.lock().await;
            guard.remove(&qid).expect("pending question registered")
        };
        tx.send(AskOutcome::Answered(vec![AuqAnswer {
            question_index: 0,
            selected_labels: vec!["Beta".to_string()],
            other_text: None,
            notes: None,
        }]))
        .expect("oneshot send");

        // The blocked tool call resolves Ok once answered.
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("tool returned before the test timeout")
            .expect("task joined");
        assert!(result.is_ok(), "tool should return Ok once answered");
    }
}
