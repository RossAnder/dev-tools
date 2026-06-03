//! JSONL record envelope, cwd→projects-dirname transform, and record→TypedMessage mapping.
//!
//! Carved out of `jsonl_tail/mod.rs` as the parsing/record-type cluster; the
//! runtime tail/bind cluster remains in `mod.rs`. Every item here stays
//! reachable at `crate::pty::jsonl_tail::*` via the `pub use parse::*;` in
//! `mod.rs`.

use std::path::{Path, PathBuf};

use crate::pty::protocol::{MessageKind, TypedMessage};

// ---------------------------------------------------------------------------
// JSONL record envelope (tolerant deserialise)
// ---------------------------------------------------------------------------

/// One row of a Claude Code session JSONL file.
///
/// Known variants are tagged on the top-level `"type"` field. An incoming
/// record whose `type` is unrecognised — or which is recognised but fails
/// to match the known shape — is captured by [`parse_line`] as
/// [`JsonlRecordParsed::UnknownRaw`] (preserving the raw line) rather than
/// trying to fit it here, so callers never lose data on a schema drift.
///
/// Per the JSONL schema research:
/// - `user.message.content` may be either a bare string OR a `Vec` of
///   content blocks (currently only `tool_result` is meaningful).
/// - `assistant.message.content` is always a `Vec` of blocks: `text`,
///   `tool_use`, `thinking`, or any future variant (absorbed via the
///   `Unknown` arm on [`AssistantContentBlock`]).
/// - `summary` records carry both `uuid` and `leafUuid`; either may be
///   absent on some emitter versions.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JsonlRecord {
    User {
        uuid: String,
        #[serde(rename = "parentUuid")]
        parent_uuid: Option<String>,
        message: UserMessage,
    },
    Assistant {
        uuid: String,
        #[serde(rename = "parentUuid")]
        parent_uuid: Option<String>,
        message: AssistantMessage,
    },
    Summary {
        uuid: Option<String>,
        #[serde(rename = "leafUuid")]
        leaf_uuid: Option<String>,
        summary: String,
    },
    /// Claude-internal metadata records (e.g. `{"type":"system","subtype":"turn_duration",...}`).
    ///
    /// We recognise the discriminator explicitly so they don't fall through
    /// the `UnknownRaw` path and get loud-rendered. All subtypes are dropped
    /// in [`map_record_to_typed`] today; if a user-facing subtype emerges,
    /// route it via the per-subtype filter table there.
    #[serde(rename = "system")]
    SystemMeta {
        #[serde(default)]
        subtype: Option<String>,
    },
}

/// Discriminator strings for records that are claude-internal and must NOT
/// be rendered in the conversation transcript. These reach
/// [`map_record_to_typed`] via the [`JsonlRecordParsed::UnknownRaw`] arm
/// (no matching [`JsonlRecord`] variant) and are silently dropped there.
///
/// Keep this list narrow: only types we have OBSERVED in the wild from
/// real `claude` sessions. Truly novel discriminators surface as a
/// compact `unknown_record_type` System row so maintainers spot drift.
const NOISY_INTERNAL_TYPES: &[&str] = &[
    "mode",
    "permission-mode",
    "file-history-snapshot",
    "attachment",
    "ai-title",
];

#[derive(Debug, Clone, serde::Deserialize)]
pub struct UserMessage {
    pub content: UserContent,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum UserContent {
    /// `"hello"` — first-turn user input is a bare string.
    Text(String),
    /// `[{type:"tool_result", ...}, ...]` — turn-2+ tool responses.
    Blocks(Vec<UserContentBlock>),
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContentBlock {
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: serde_json::Value,
        #[serde(default)]
        is_error: bool,
    },
    /// Any future user-content-block type — opaque to us.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<AssistantContentBlock>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: Option<String>,
    },
    /// Any future assistant-content-block type — opaque to us.
    #[serde(other)]
    Unknown,
}

/// Result of [`parse_line`].
///
/// We can't use `#[serde(other)]` directly on [`JsonlRecord`] because the
/// `other` arm cannot carry data — so a custom two-stage parse runs the
/// strict known-variant deserialise first, and falls back to raw-capture
/// on any failure (unknown `type` discriminator, missing required field,
/// malformed JSON).
#[derive(Debug, Clone)]
pub enum JsonlRecordParsed {
    /// Parsed into one of the known [`JsonlRecord`] variants.
    Known(JsonlRecord),
    /// Couldn't parse — raw line preserved, plus a best-effort attempt to
    /// pull the `type` discriminator out (None when the JSON itself is
    /// malformed).
    UnknownRaw {
        raw: String,
        parsed_type: Option<String>,
    },
}

/// Parse one JSONL line into a [`JsonlRecordParsed`].
///
/// On any deserialise failure (unknown `type`, missing field, etc.) returns
/// `UnknownRaw` carrying the original line so the supervisor can persist
/// it as a `system` row without losing the data.
pub fn parse_line(line: &str) -> JsonlRecordParsed {
    match serde_json::from_str::<JsonlRecord>(line) {
        Ok(rec) => JsonlRecordParsed::Known(rec),
        Err(_) => {
            // Best-effort: pull out the `type` field for diagnostics. If the
            // line isn't valid JSON at all, parsed_type stays None.
            let parsed_type = serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from));
            JsonlRecordParsed::UnknownRaw {
                raw: line.to_string(),
                parsed_type,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// cwd → projects-dirname transformation
// ---------------------------------------------------------------------------

/// Reproduce the Claude Code projects-dirname algorithm.
///
/// Replaces each byte that is NOT in `[A-Za-z0-9-]` with `-`. No
/// consecutive-`-` collapsing; no leading-dash special-case; no lowercasing.
/// On Windows, `C:\Users\rossa\dev\dev-tools` becomes
/// `C--Users-rossa-dev-dev-tools` — verified against the on-disk directory
/// in this repository.
///
/// Operates on the path's lossy-UTF8 string representation, which matches
/// how Claude Code itself behaves on non-UTF8 path bytes. The result is a
/// `String` rather than an `OsString` because it's used to compose paths
/// under `~/.claude/projects/` where the directory name is always ASCII
/// after the substitution.
pub fn sanitise_cwd(p: &Path) -> String {
    let raw = p.to_string_lossy();
    // Strip the Windows verbatim-path prefix `\\?\` if present. `std::fs::canonicalize`
    // returns this form on Windows for any path, but Claude Code sanitises the bare
    // user-visible cwd (`C:\Users\rossa\dev\dev-tools` → `C--Users-rossa-dev-dev-tools`),
    // NOT the canonicalised verbatim form (which would sanitise to a four-leading-dash
    // `----C--…`). Without this strip, lumina's bind_jsonl_path watches a directory
    // claude.exe never writes to, and the spawn 5s-timeouts.
    let s: &str = raw.strip_prefix(r"\\?\").unwrap_or(&raw);
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' {
            out.push(b as char);
        } else {
            out.push('-');
        }
    }
    out
}

/// Resolve the base directory that contains per-cwd projects subdirectories.
///
/// Precedence:
/// 1. `LUMINA_PTY_PROJECTS_ROOT` env var (used by tests; precedent:
///    `LUMINA_WORKTREE_ROOT` in [`crate::pty::spawn::resolve_and_validate_cwd`]).
/// 2. Platform default: `%USERPROFILE%\.claude\projects` on Windows;
///    `$HOME/.claude/projects` on Unix.
/// 3. If neither is set, fall back to the current working directory and
///    emit a diagnostic — this is a "should not happen in practice" path,
///    but we don't panic.
pub fn resolve_projects_root() -> PathBuf {
    if let Ok(v) = std::env::var("LUMINA_PTY_PROJECTS_ROOT") {
        return PathBuf::from(v);
    }

    #[cfg(target_os = "windows")]
    let home_var = "USERPROFILE";
    #[cfg(not(target_os = "windows"))]
    let home_var = "HOME";

    match std::env::var(home_var) {
        Ok(home) => PathBuf::from(home).join(".claude").join("projects"),
        Err(_) => {
            tracing::warn!(
                home_var = %home_var,
                "jsonl_tail: home env var unset; falling back to CWD for projects root"
            );
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
    }
}

// ---------------------------------------------------------------------------
// JSONL record → TypedMessage mapping
// ---------------------------------------------------------------------------

/// Map one parsed JSONL record to zero-or-more [`TypedMessage`] rows.
///
/// A single `assistant` record carrying `N` content blocks emits `N` rows
/// (one per block, preserving order). A `user` record whose content is a
/// `Vec` of `tool_result` blocks emits one row per result. Bare-string
/// `user` content emits one `UserInput` row; `summary` emits one `System`
/// row; unknowns of any flavour fall through to a `System` row that carries
/// enough metadata for the SPA / replay to reason about what was lost.
///
/// `sequence` is always `0` on the returned rows — the bridge task in
/// `pty::spawn` mints the per-session monotone sequence via
/// `Session::next_sequence()` immediately before persistence. `created_at`
/// is stamped here with the wall-clock `jiff::Timestamp::now().to_string()`
/// (matches the convention in `export.rs` and `domain.rs`).
pub fn map_record_to_typed(parsed: &JsonlRecordParsed) -> Vec<TypedMessage> {
    let now = jiff::Timestamp::now().to_string();
    match parsed {
        JsonlRecordParsed::Known(rec) => match rec {
            JsonlRecord::Assistant { message, .. } => message
                .content
                .iter()
                .filter_map(|block| match block {
                    // Skip empty / whitespace-only text blocks. claude commonly
                    // emits a blank text block alongside a `tool_use` block;
                    // mapping it would render an empty bubble in the transcript.
                    AssistantContentBlock::Text { text } if text.trim().is_empty() => None,
                    AssistantContentBlock::Text { text } => Some(TypedMessage {
                        sequence: 0,
                        kind: MessageKind::AssistantText,
                        content: serde_json::json!({ "text": text }),
                        raw_text: Some(text.clone()),
                        created_at: now.clone(),
                        tool_use_id: None,
                    }),
                    AssistantContentBlock::ToolUse { id, name, input } => Some(TypedMessage {
                        sequence: 0,
                        kind: MessageKind::ToolUse,
                        content: serde_json::json!({
                            "name": name,
                            "input": input,
                            "tool_use_id": id,
                        }),
                        raw_text: None,
                        created_at: now.clone(),
                        tool_use_id: Some(id.clone()),
                    }),
                    AssistantContentBlock::Thinking { thinking, signature } => Some(TypedMessage {
                        sequence: 0,
                        kind: MessageKind::System,
                        content: serde_json::json!({
                            "subtype": "thinking",
                            "text": thinking,
                            "signature": signature,
                        }),
                        raw_text: Some(thinking.clone()),
                        created_at: now.clone(),
                        tool_use_id: None,
                    }),
                    AssistantContentBlock::Unknown => Some(TypedMessage {
                        sequence: 0,
                        kind: MessageKind::System,
                        content: serde_json::json!({
                            "subtype": "unknown_assistant_block",
                        }),
                        raw_text: None,
                        created_at: now.clone(),
                        tool_use_id: None,
                    }),
                })
                .collect(),
            JsonlRecord::User { message, .. } => match &message.content {
                UserContent::Text(s) => vec![TypedMessage {
                    sequence: 0,
                    kind: MessageKind::UserInput,
                    content: serde_json::json!({ "text": s }),
                    raw_text: Some(s.clone()),
                    created_at: now,
                    tool_use_id: None,
                }],
                UserContent::Blocks(blocks) => blocks
                    .iter()
                    .map(|block| match block {
                        UserContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => TypedMessage {
                            sequence: 0,
                            kind: MessageKind::ToolResult,
                            content: serde_json::json!({
                                "tool_use_id": tool_use_id,
                                "output": content,
                                "is_error": is_error,
                            }),
                            raw_text: None,
                            created_at: now.clone(),
                            tool_use_id: Some(tool_use_id.clone()),
                        },
                        UserContentBlock::Unknown => TypedMessage {
                            sequence: 0,
                            kind: MessageKind::System,
                            content: serde_json::json!({
                                "subtype": "unknown_user_block",
                            }),
                            raw_text: None,
                            created_at: now.clone(),
                            tool_use_id: None,
                        },
                    })
                    .collect(),
            },
            JsonlRecord::Summary { summary, .. } => vec![TypedMessage {
                sequence: 0,
                kind: MessageKind::System,
                content: serde_json::json!({
                    "subtype": "summary",
                    "text": summary,
                }),
                raw_text: Some(summary.clone()),
                created_at: now,
                tool_use_id: None,
            }],
            JsonlRecord::SystemMeta { subtype: _ } => {
                // Claude-internal metadata (turn_duration, etc.). Drop all
                // subtypes today; if a user-facing subtype emerges, add a
                // match arm here that returns a curated TypedMessage row.
                Vec::new()
            }
        },
        JsonlRecordParsed::UnknownRaw { raw, parsed_type } => match parsed_type {
            Some(t) if NOISY_INTERNAL_TYPES.contains(&t.as_str()) => {
                // Known-noisy claude-internal record type — drop silently.
                Vec::new()
            }
            Some(t) => {
                // Previously-unseen discriminator. Emit a COMPACT marker so
                // maintainers spot it in the transcript without flooding it
                // with raw JSON. raw_text intentionally None.
                vec![TypedMessage {
                    sequence: 0,
                    kind: MessageKind::System,
                    content: serde_json::json!({
                        "subtype": "unknown_record_type",
                        "type": t,
                    }),
                    raw_text: None,
                    created_at: now,
                    tool_use_id: None,
                }]
            }
            None => {
                // Truly malformed JSON (no parseable `type` discriminator).
                // Preserve the raw line in raw_text for forensic debugging.
                vec![TypedMessage {
                    sequence: 0,
                    kind: MessageKind::System,
                    content: serde_json::json!({
                        "subtype": "malformed_jsonl",
                    }),
                    raw_text: Some(raw.clone()),
                    created_at: now,
                    tool_use_id: None,
                }]
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;
    use std::path::Path;

    // -----------------------------------------------------------------
    // sanitise_cwd — table-driven
    // -----------------------------------------------------------------
    #[rstest]
    #[case(r"C:\Users\rossa\dev\dev-tools", "C--Users-rossa-dev-dev-tools")]
    #[case("/home/ross/work/dev-tools", "-home-ross-work-dev-tools")]
    #[case("/var/log/app.test", "-var-log-app-test")]
    #[case("abc-def-123", "abc-def-123")]
    #[case("/tmp/with space/file", "-tmp-with-space-file")]
    // Windows verbatim-prefix forms (\\?\…) — std::fs::canonicalize emits these on
    // Windows, but Claude Code sanitises the bare user-visible cwd. The prefix MUST
    // be stripped to keep lumina's watched directory aligned with claude's writes.
    #[case(r"\\?\C:\Users\rossa\dev\dev-tools", "C--Users-rossa-dev-dev-tools")]
    #[case(r"\\?\C:\Users\rossa\dev\dev-tools\lumina", "C--Users-rossa-dev-dev-tools-lumina")]
    fn sanitise_cwd_cases(#[case] input: &str, #[case] expected: &str) {
        let got = sanitise_cwd(Path::new(input));
        assert_eq!(got, expected);
    }

    // -----------------------------------------------------------------
    // JsonlRecord deserialise — known + unknown
    // -----------------------------------------------------------------
    #[test]
    fn parse_assistant_text() {
        let line = r#"{"type":"assistant","uuid":"a","parentUuid":"b","message":{"content":[{"type":"text","text":"hi"}]}}"#;
        match parse_line(line) {
            JsonlRecordParsed::Known(JsonlRecord::Assistant { uuid, parent_uuid, message }) => {
                assert_eq!(uuid, "a");
                assert_eq!(parent_uuid.as_deref(), Some("b"));
                assert_eq!(message.content.len(), 1);
                match &message.content[0] {
                    AssistantContentBlock::Text { text } => assert_eq!(text, "hi"),
                    other => panic!("expected Text, got {other:?}"),
                }
            }
            other => panic!("expected Known(Assistant), got {other:?}"),
        }
    }

    #[test]
    fn parse_assistant_tool_use() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"id1","name":"Read","input":{"file_path":"x"}}]}}"#;
        match parse_line(line) {
            JsonlRecordParsed::Known(JsonlRecord::Assistant { message, .. }) => {
                match &message.content[0] {
                    AssistantContentBlock::ToolUse { id, name, input } => {
                        assert_eq!(id, "id1");
                        assert_eq!(name, "Read");
                        assert_eq!(input.get("file_path").and_then(|v| v.as_str()), Some("x"));
                    }
                    other => panic!("expected ToolUse, got {other:?}"),
                }
            }
            other => panic!("expected Known(Assistant), got {other:?}"),
        }
    }

    #[test]
    fn parse_user_string_content() {
        let line = r#"{"type":"user","uuid":"u","parentUuid":null,"message":{"content":"hello"}}"#;
        match parse_line(line) {
            JsonlRecordParsed::Known(JsonlRecord::User { uuid, parent_uuid, message }) => {
                assert_eq!(uuid, "u");
                assert_eq!(parent_uuid, None);
                match message.content {
                    UserContent::Text(s) => assert_eq!(s, "hello"),
                    other => panic!("expected Text, got {other:?}"),
                }
            }
            other => panic!("expected Known(User), got {other:?}"),
        }
    }

    #[test]
    fn parse_user_tool_result_block() {
        let line = r#"{"type":"user","uuid":"u","message":{"content":[{"type":"tool_result","tool_use_id":"id1","content":"ok","is_error":false}]}}"#;
        match parse_line(line) {
            JsonlRecordParsed::Known(JsonlRecord::User { message, .. }) => {
                match &message.content {
                    UserContent::Blocks(blocks) => {
                        assert_eq!(blocks.len(), 1);
                        match &blocks[0] {
                            UserContentBlock::ToolResult { tool_use_id, content, is_error } => {
                                assert_eq!(tool_use_id, "id1");
                                assert_eq!(content.as_str(), Some("ok"));
                                assert!(!*is_error);
                            }
                            other => panic!("expected ToolResult, got {other:?}"),
                        }
                    }
                    other => panic!("expected Blocks, got {other:?}"),
                }
            }
            other => panic!("expected Known(User), got {other:?}"),
        }
    }

    #[test]
    fn parse_summary() {
        let line = r#"{"type":"summary","summary":"compacted","leafUuid":"x"}"#;
        match parse_line(line) {
            JsonlRecordParsed::Known(JsonlRecord::Summary { summary, leaf_uuid, .. }) => {
                assert_eq!(summary, "compacted");
                assert_eq!(leaf_uuid.as_deref(), Some("x"));
            }
            other => panic!("expected Known(Summary), got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_type_captures_raw() {
        let line = r#"{"type":"file-history-snapshot","whatever":"data"}"#;
        match parse_line(line) {
            JsonlRecordParsed::UnknownRaw { raw, parsed_type } => {
                assert_eq!(parsed_type.as_deref(), Some("file-history-snapshot"));
                assert_eq!(raw, line);
            }
            other => panic!("expected UnknownRaw, got {other:?}"),
        }
    }

    #[test]
    fn parse_malformed_json_captures_raw_with_no_type() {
        let line = "not-json-at-all";
        match parse_line(line) {
            JsonlRecordParsed::UnknownRaw { raw, parsed_type } => {
                assert_eq!(parsed_type, None);
                assert_eq!(raw, line);
            }
            other => panic!("expected UnknownRaw, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------
    // map_record_to_typed
    // -----------------------------------------------------------------

    #[test]
    fn map_assistant_text_emits_one_assistant_text_row() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":"hello"}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MessageKind::AssistantText);
        assert_eq!(
            out[0].content.get("text").and_then(|v| v.as_str()),
            Some("hello")
        );
        assert_eq!(out[0].raw_text.as_deref(), Some("hello"));
        assert_eq!(out[0].tool_use_id, None);
    }

    #[test]
    fn map_assistant_tool_use_sets_tool_use_id() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"tool_use","id":"id1","name":"Read","input":{"file_path":"x"}}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MessageKind::ToolUse);
        assert_eq!(out[0].tool_use_id.as_deref(), Some("id1"));
        assert_eq!(
            out[0].content.get("name").and_then(|v| v.as_str()),
            Some("Read")
        );
        assert_eq!(
            out[0]
                .content
                .get("tool_use_id")
                .and_then(|v| v.as_str()),
            Some("id1")
        );
    }

    #[test]
    fn map_user_tool_result_sets_tool_use_id() {
        let line = r#"{"type":"user","uuid":"u","message":{"content":[{"type":"tool_result","tool_use_id":"id1","content":"ok","is_error":false}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MessageKind::ToolResult);
        assert_eq!(out[0].tool_use_id.as_deref(), Some("id1"));
        assert_eq!(
            out[0]
                .content
                .get("is_error")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
    }

    #[test]
    fn map_assistant_mixed_blocks_preserves_order() {
        // text block first, then a tool_use block — expect two rows in that order.
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":"thinking..."},{"type":"tool_use","id":"id7","name":"Bash","input":{"command":"ls"}}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, MessageKind::AssistantText);
        assert_eq!(out[1].kind, MessageKind::ToolUse);
        assert_eq!(out[1].tool_use_id.as_deref(), Some("id7"));
    }

    #[test]
    fn map_assistant_skips_empty_text_block_keeps_tool_use() {
        // claude often emits a blank text block right before a tool_use; only
        // the tool_use should produce a row (the blank text would otherwise
        // render as an empty bubble).
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":"  \n "},{"type":"tool_use","id":"id9","name":"Bash","input":{"command":"ls"}}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1, "empty text block must be dropped; got {out:?}");
        assert_eq!(out[0].kind, MessageKind::ToolUse);
        assert_eq!(out[0].tool_use_id.as_deref(), Some("id9"));
    }

    #[test]
    fn map_assistant_lone_empty_text_emits_nothing() {
        let line = r#"{"type":"assistant","uuid":"a","message":{"content":[{"type":"text","text":""}]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "a lone empty text block emits no rows; got {out:?}");
    }

    #[test]
    fn map_user_string_emits_user_input() {
        let line = r#"{"type":"user","uuid":"u","message":{"content":"hi there"}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MessageKind::UserInput);
        assert_eq!(out[0].raw_text.as_deref(), Some("hi there"));
        assert_eq!(out[0].tool_use_id, None);
    }

    // -----------------------------------------------------------------
    // Noise-filter: claude-internal record types must NOT reach the
    // SPA transcript. See NOISY_INTERNAL_TYPES + the SystemMeta variant.
    // -----------------------------------------------------------------

    #[test]
    fn map_drops_mode_record() {
        let line = r#"{"type":"mode","mode":"normal","sessionId":"s1"}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "mode records must be dropped; got {out:?}");
    }

    #[test]
    fn map_drops_permission_mode_record() {
        let line = r#"{"type":"permission-mode","permissionMode":"acceptEdits","sessionId":"s1"}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "permission-mode records must be dropped; got {out:?}");
    }

    #[test]
    fn map_drops_file_history_snapshot_record() {
        let line = r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"files":[]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "file-history-snapshot records must be dropped; got {out:?}");
    }

    #[test]
    fn map_drops_attachment_record() {
        // Representative `attachment` shape — skill_listing variant with a tiny payload.
        let line = r#"{"type":"attachment","attachment":{"type":"skill_listing","skills":[]}}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "attachment records must be dropped; got {out:?}");
    }

    #[test]
    fn map_drops_ai_title_record() {
        let line = r#"{"type":"ai-title","aiTitle":"My Convo","sessionId":"s1"}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "ai-title records must be dropped; got {out:?}");
    }

    #[test]
    fn map_drops_system_turn_duration_record() {
        let line = r#"{"type":"system","subtype":"turn_duration","durationMs":1234,"sessionId":"s1"}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert!(out.is_empty(), "system/turn_duration records must be dropped; got {out:?}");
    }

    #[test]
    fn map_unknown_type_emits_compact_system_row() {
        let line = r#"{"type":"some-future-type","data":"big-payload-that-must-not-leak"}"#;
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1, "expected exactly one row for an unknown type");
        assert_eq!(out[0].kind, MessageKind::System);
        assert_eq!(
            out[0].content.get("subtype").and_then(|v| v.as_str()),
            Some("unknown_record_type")
        );
        assert_eq!(
            out[0].content.get("type").and_then(|v| v.as_str()),
            Some("some-future-type")
        );
        // The raw payload must NOT leak into raw_text or content.
        assert!(out[0].raw_text.is_none(), "raw_text must be None for compact marker");
        assert!(
            out[0].content.get("raw").is_none(),
            "content must not carry the raw line"
        );
    }

    #[test]
    fn map_malformed_json_emits_one_forensic_system_row() {
        let line = "not-valid-json-at-all";
        let out = map_record_to_typed(&parse_line(line));
        assert_eq!(out.len(), 1, "expected exactly one forensic row");
        assert_eq!(out[0].kind, MessageKind::System);
        assert_eq!(
            out[0].content.get("subtype").and_then(|v| v.as_str()),
            Some("malformed_jsonl")
        );
        assert_eq!(out[0].raw_text.as_deref(), Some("not-valid-json-at-all"));
    }
}
