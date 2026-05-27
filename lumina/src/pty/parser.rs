//! vt100-backed parser pipeline: PTY bytes → segmented [`TypedMessage`] blocks.
//!
//! The parser maintains an in-memory virtual screen (vt100 0.16.2) that we do
//! NOT render to a UI — we only use it to canonicalise ANSI / cursor-movement
//! control sequences down to plain text rows, which we then classify and emit
//! as typed message blocks for the supervisor (T8) to persist.
//!
//! Segmentation strategy (v1 baseline — deliberately heuristic; see plan
//! Risks §): after each `feed()` we read the cursor position to decide which
//! visible rows are "finalised" (the cursor has advanced past them) versus
//! "in progress" (the cursor is still on them). Finalised non-blank rows are
//! grouped into runs by classification and emitted as one TypedMessage per
//! run. The cursor row, when it matches the prompt matcher, is emitted as a
//! `Prompt` block AND drives the end-of-turn idle heuristic.
//!
//! ## Vt100 0.16.2 API surface used
//!
//! - `vt100::Parser::new(rows, cols, scrollback_len)` — constructor.
//! - `vt100::Parser::process(&mut self, bytes: &[u8])` — advance the FSM.
//! - `vt100::Parser::screen() -> &vt100::Screen` — snapshot accessor.
//! - `vt100::Screen::rows(start_col, width) -> impl Iterator<Item = String>`
//!   — per-row plain-text contents (ANSI already stripped by vt100; newlines
//!   NOT included; trailing spaces from cell padding are preserved).
//! - `vt100::Screen::cursor_position() -> (u16, u16)` — `(row, col)`.
//! - `vt100::Screen::size() -> (u16, u16)` — `(rows, cols)`.
//!
//! ## Constraints
//!
//! - The `regex` crate is intentionally NOT a Cargo dependency. The prompt
//!   matcher is hand-rolled (see [`matches_prompt`]). Custom user prompt
//!   patterns from `SpawnConfig::prompt_pattern` are NOT honoured by v1 —
//!   only the built-in heuristic runs. A future revision can promote the
//!   matcher to a small DSL once we have real fixture data driving it.
//! - The parser doesn't emit `UserInput` blocks; the supervisor (T8) emits
//!   those from its own send-side buffer, since the parser has no visibility
//!   into what we typed into the PTY.

use std::time::{Duration, Instant};

use crate::pty::protocol::{MessageKind, TypedMessage};

/// vt100 parser pipeline + end-of-turn heuristic.
///
/// Owns a [`vt100::Parser`] for ANSI / cursor-movement canonicalisation and
/// tracks a row cursor (`last_emitted_row`) so each call to [`feed`] only
/// emits NEW finalised rows since the previous call.
///
/// The `parse_strategy_version` field is exposed so the supervisor (T8) can
/// stamp it on the `pty_sessions.parse_strategy_version` column — a forward
/// hook for when a future version replaces the heuristics and we need to
/// distinguish persisted transcripts by parser generation.
pub struct Parser {
    vt: vt100::Parser,
    last_emitted_row: u16,
    idle_since: Option<Instant>,
    /// v1 generation marker; persisted by the supervisor on `pty_sessions`.
    pub parse_strategy_version: i64,
}

impl Parser {
    /// 80×24 screen with no scrollback (vt100's own default dimensions).
    pub fn new() -> Self {
        Self::new_with_size(24, 80)
    }

    /// Custom screen size — useful for tests that need a known grid.
    pub fn new_with_size(rows: u16, cols: u16) -> Self {
        Self {
            vt: vt100::Parser::new(rows, cols, 0),
            last_emitted_row: 0,
            idle_since: None,
            parse_strategy_version: 1,
        }
    }

    /// Feed raw PTY bytes; return any newly-finalised typed-message blocks.
    ///
    /// `sequence` on each returned block is left as `0` — the supervisor mints
    /// the monotone session-local sequence numbers when persisting.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<TypedMessage> {
        self.vt.process(bytes);

        let (_screen_rows, cols) = self.vt.screen().size();
        let (cursor_row, _cursor_col) = self.vt.screen().cursor_position();

        // Snapshot every visible row's plain-text content. `rows(0, cols)`
        // returns an iterator of `String`s without trailing newlines; vt100
        // has already canonicalised any ANSI escapes away.
        let all_rows: Vec<String> = self.vt.screen().rows(0, cols).collect();

        // Finalised rows: [last_emitted_row, cursor_row). The cursor row is
        // still "in progress" — we don't emit it as text but we DO probe it
        // for the prompt matcher below.
        let finalise_end = cursor_row;
        let finalised: Vec<(u16, String)> = if self.last_emitted_row < finalise_end {
            (self.last_emitted_row..finalise_end)
                .filter_map(|r| all_rows.get(r as usize).map(|s| (r, s.clone())))
                .collect()
        } else {
            Vec::new()
        };

        let mut emitted = Vec::new();
        // Group finalised rows into contiguous runs of the same classification,
        // skipping blank rows (whitespace-only). One TypedMessage per run.
        let mut current_kind: Option<MessageKind> = None;
        let mut current_buf: Vec<String> = Vec::new();
        for (_row_idx, line) in &finalised {
            let trimmed = line.trim_end();
            if trimmed.trim().is_empty() {
                // Blank row terminates any in-progress run.
                if let Some(kind) = current_kind.take() {
                    let text = current_buf.join("\n");
                    emitted.push(text_block(kind, text));
                    current_buf.clear();
                }
                continue;
            }
            let kind = classify_line(trimmed);
            match current_kind {
                Some(k) if k == kind => {
                    current_buf.push(trimmed.to_string());
                }
                Some(prev) => {
                    let text = current_buf.join("\n");
                    emitted.push(text_block(prev, text));
                    current_buf.clear();
                    current_buf.push(trimmed.to_string());
                    current_kind = Some(kind);
                }
                None => {
                    current_buf.push(trimmed.to_string());
                    current_kind = Some(kind);
                }
            }
        }
        if let Some(kind) = current_kind {
            let text = current_buf.join("\n");
            emitted.push(text_block(kind, text));
        }

        // Advance the row cursor past every row we just considered. The cursor
        // row itself is NOT marked emitted yet (it may grow more text).
        self.last_emitted_row = finalise_end;

        // Probe the cursor row for the prompt matcher. If it matches, emit a
        // Prompt block (and treat the prompt row as emitted) — and mark idle.
        let mut produced_prompt = false;
        if let Some(cur_line) = all_rows.get(cursor_row as usize) {
            let trimmed = cur_line.trim_end();
            if !trimmed.trim().is_empty() && matches_prompt(trimmed) {
                emitted.push(text_block(MessageKind::Prompt, trimmed.to_string()));
                self.last_emitted_row = cursor_row.saturating_add(1);
                produced_prompt = true;
            }
        }

        // Idle bookkeeping:
        // - If we sat on a prompt OR produced nothing this feed → mark idle.
        // - Otherwise (we emitted real content other than the prompt) → clear.
        if produced_prompt || emitted.is_empty() {
            if self.idle_since.is_none() {
                self.idle_since = Some(Instant::now());
            }
        } else {
            self.idle_since = None;
        }

        emitted
    }

    /// End-of-turn signal: true IFF the cursor row currently matches the
    /// prompt matcher AND `idle_since` is at least `threshold` old as of
    /// `now`. The supervisor polls this on its idle timer.
    pub fn check_idle(&mut self, now: Instant, threshold: Duration) -> bool {
        let Some(since) = self.idle_since else {
            return false;
        };
        if now.saturating_duration_since(since) < threshold {
            return false;
        }

        let (_rows, cols) = self.vt.screen().size();
        let (cursor_row, _cursor_col) = self.vt.screen().cursor_position();
        let all_rows: Vec<String> = self.vt.screen().rows(0, cols).collect();
        let Some(line) = all_rows.get(cursor_row as usize) else {
            return false;
        };
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() {
            return false;
        }
        matches_prompt(trimmed)
    }
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

/// Hand-rolled prompt matcher (no `regex` dep): the bottom line is a prompt
/// when its trimmed form is exactly one of the known sigils, OR when the line
/// begins with `Human:` (the conversation-prefix convention in `claude` REPL
/// transcripts).
fn matches_prompt(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    matches!(t, ">" | "›" | "❯") || t.starts_with("Human:")
}

/// Classification rule for a finalised non-blank line. Tool-call sigils win;
/// anything else is assistant text by default. The `ParserUnknown` kind is
/// reserved for residual cases that the supervisor or higher layers spot —
/// the line-level classifier here is total over the assistant/tool axis.
fn classify_line(line: &str) -> MessageKind {
    if line.starts_with("⏺") || line.starts_with("🔧") || line.starts_with("Tool:") {
        MessageKind::ToolCall
    } else {
        MessageKind::AssistantText
    }
}

/// Build a text-shaped TypedMessage. Both `content.text` and `raw_text` carry
/// the same canonicalised text; future revisions may diverge these (e.g.
/// `raw_text` keeping ANSI for replay) once a real consumer demands it.
fn text_block(kind: MessageKind, text: String) -> TypedMessage {
    TypedMessage {
        sequence: 0,
        kind,
        content: serde_json::json!({ "text": text }),
        raw_text: Some(text),
        created_at: jiff::Timestamp::now().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn feed_emits_assistant_text_block() {
        let mut p = Parser::new();
        let out = p.feed(b"hello world\n");
        // Exactly one block, assistant-text, content.text == "hello world".
        assert_eq!(out.len(), 1, "expected one block, got {out:?}");
        assert_eq!(out[0].kind, MessageKind::AssistantText);
        assert_eq!(
            out[0].content.get("text").and_then(|v| v.as_str()),
            Some("hello world")
        );
        assert_eq!(out[0].raw_text.as_deref(), Some("hello world"));
    }

    #[test]
    fn feed_detects_prompt_line() {
        let mut p = Parser::new();
        let _ = p.feed(b"some output\n> ");
        // Force idle_since back at least 100ms so the threshold passes.
        p.idle_since = Some(Instant::now() - Duration::from_millis(500));
        let now = Instant::now();
        assert!(
            p.check_idle(now, Duration::from_millis(100)),
            "expected check_idle to fire on '> ' prompt; idle_since={:?}",
            p.idle_since
        );
    }

    #[test]
    fn feed_treats_tool_call_sigil() {
        let mut p = Parser::new();
        // "⏺ ToolName(arg)\n  result\n\n" — sigil-led block, then result row,
        // then a blank separator. We expect exactly one ToolCall block whose
        // text covers the two non-blank rows.
        let out = p.feed("⏺ ToolName(arg)\n  result\n\n".as_bytes());
        let tool_calls: Vec<&TypedMessage> = out
            .iter()
            .filter(|m| m.kind == MessageKind::ToolCall)
            .collect();
        assert_eq!(
            tool_calls.len(),
            1,
            "expected exactly one ToolCall block; got: {out:?}"
        );
        let text = tool_calls[0]
            .content
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            text.contains("ToolName(arg)"),
            "ToolCall text should contain the sigil row: {text:?}"
        );
    }

    #[test]
    fn feed_parser_unknown_for_uncategorised() {
        let mut p = Parser::new();
        // Pure cursor movement (CSI H = move cursor to home) with no printable
        // payload should produce no emissions. The contract: empty vec out,
        // idle_since gets set.
        let out = p.feed(b"\x1b[H");
        assert!(out.is_empty(), "expected no emissions, got {out:?}");
        assert!(
            p.idle_since.is_some(),
            "idle_since should be set after a no-content feed"
        );
    }

    #[test]
    fn parse_strategy_version_is_one() {
        let p = Parser::new();
        assert_eq!(p.parse_strategy_version, 1);
    }
}
