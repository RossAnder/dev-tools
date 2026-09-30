//! Incremental tail of a subagent's transcript file.
//!
//! A transcript is JSONL that Claude Code appends to while the agent runs.
//! [`TailState::refresh`] reads only the bytes past the last complete line it
//! consumed, so a poll costs the size of what was appended, not of the file.
//! Only `assistant` lines become entries: each `text` block and each
//! `tool_use` block is one; `thinking` blocks and every other line are skipped.
//!
//! **Observed against Claude Code 2.1.283, 2026-09-28.** The line shape read
//! here (`type`, `timestamp`, `message.content[]` blocks, `message.usage`) is
//! not a documented contract. A renamed field degrades to no entries rather
//! than an error, so a panel that goes quiet on a live agent starts here.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::claude_dir;

/// The most a single refresh reads. A first read, or a backlog larger than
/// this, starts at the first line boundary inside the file's last window.
pub(crate) const READ_MAX: u64 = 64 * 1024;
/// Entries kept; older ones are dropped as new ones arrive.
pub(crate) const KEEP_ENTRIES: usize = 64;
/// Characters kept of a tool call's detail and of a text block.
const DETAIL_MAX: usize = 80;
const TEXT_MAX: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntryKind {
    ToolUse { name: String, detail: String },
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The line's `timestamp`, verbatim; `""` when it has none.
    pub(crate) ts: String,
    pub(crate) kind: EntryKind,
    /// The block's prose with whitespace collapsed; `""` for a tool call.
    pub(crate) text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TailState {
    /// The transcript path as the agents store records it.
    pub(crate) path: String,
    /// Byte position just past the last complete line consumed.
    pub(crate) offset: u64,
    pub(crate) entries: Vec<Entry>,
    /// Context size from the newest assistant `usage` seen, which the agents
    /// store only fills in once the agent stops.
    pub(crate) tokens: Option<u64>,
    /// Set when the path does not canonicalise under the root; nothing is read.
    pub(crate) rejected: bool,
    /// Set once the path has canonicalised under the root.
    accepted: bool,
    /// The canonical directory the transcript must sit under; `None` rejects every path.
    root: Option<PathBuf>,
}

/// What the activity panel draws of a tail: its fields, without the means to read the
/// file, so the UI thread cannot do file I/O through it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TailView {
    pub(crate) path: String,
    pub(crate) entries: Vec<Entry>,
    pub(crate) tokens: Option<u64>,
    pub(crate) rejected: bool,
}

impl TailState {
    /// A copy of the fields the panel draws.
    pub(crate) fn view(&self) -> TailView {
        TailView {
            path: self.path.clone(),
            entries: self.entries.clone(),
            tokens: self.tokens,
            rejected: self.rejected,
        }
    }

    /// A tail confined to the Claude config directory.
    pub(crate) fn new(path: &str) -> TailState {
        TailState::within(path, claude_dir())
    }

    /// A tail confined to `root`, canonicalised once here; a root that cannot be
    /// canonicalised rejects every path.
    pub(crate) fn within(path: &str, root: Option<PathBuf>) -> TailState {
        TailState {
            path: path.to_string(),
            root: root.and_then(|root| std::fs::canonicalize(root).ok()),
            ..TailState::default()
        }
    }

    /// Points the tail at `path`, starting over when it differs from the
    /// current one. The root is kept.
    pub(crate) fn retarget(&mut self, path: &str) {
        if self.path != path {
            *self = TailState {
                path: path.to_string(),
                root: self.root.take(),
                ..TailState::default()
            };
        }
    }

    /// Reads what was appended since the last call. Returns whether anything
    /// visible changed. A file shorter than `offset` was replaced, so the tail
    /// starts over; a trailing line with no newline yet is left for next time.
    pub(crate) fn refresh(&mut self) -> bool {
        // A path already accepted whose length has not moved has nothing to read, and
        // costs one stat instead of canonicalising, opening and statting.
        if self.accepted
            && !self.rejected
            && std::fs::metadata(&self.path).is_ok_and(|meta| meta.len() == self.offset)
        {
            return false;
        }
        let Some(path) = self.resolve() else {
            return false;
        };
        let Ok(mut file) = File::open(&path) else {
            return false;
        };
        let Ok(len) = file.metadata().map(|meta| meta.len()) else {
            return false;
        };

        let mut changed = false;
        if len < self.offset {
            changed = !self.entries.is_empty() || self.tokens.is_some();
            self.offset = 0;
            self.entries.clear();
            self.tokens = None;
        }
        if len == self.offset {
            return changed;
        }

        // Past the cap the read starts one byte early, so a window that
        // happens to begin exactly on a line boundary is not cut short.
        let skip_partial = len - self.offset > READ_MAX;
        let seek = if skip_partial {
            len - READ_MAX - 1
        } else {
            self.offset
        };
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(seek)).is_err()
            || file.take(len - seek).read_to_end(&mut bytes).is_err()
        {
            return changed;
        }

        let mut base = seek;
        let mut body = bytes.as_slice();
        if skip_partial {
            let Some(cut) = body.iter().position(|b| *b == b'\n') else {
                // One line longer than the window: unreadable as JSON anyway.
                self.offset = seek + bytes.len() as u64;
                return changed;
            };
            base += cut as u64 + 1;
            body = &body[cut + 1..];
        }
        let Some(last) = body.iter().rposition(|b| *b == b'\n') else {
            if skip_partial {
                self.offset = base;
            }
            return changed;
        };
        self.offset = base + last as u64 + 1;

        for line in body[..last].split(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(line);
            let line = line.trim_end_matches('\r');
            let (entries, tokens) = parse_line(line);
            if let Some(tokens) = tokens {
                changed |= self.tokens != Some(tokens);
                self.tokens = Some(tokens);
            }
            changed |= !entries.is_empty();
            self.entries.extend(entries);
        }
        if self.entries.len() > KEEP_ENTRIES {
            let excess = self.entries.len() - KEEP_ENTRIES;
            self.entries.drain(..excess);
        }
        changed
    }

    /// The canonical transcript path when it lies under the root, else `None`
    /// with `rejected` set. A file that does not exist yet is not rejected.
    fn resolve(&mut self) -> Option<PathBuf> {
        if self.path.is_empty() {
            return None;
        }
        let path = Path::new(&self.path);
        let file = match std::fs::canonicalize(path) {
            Ok(file) => file,
            Err(_) => return None,
        };
        let inside = self
            .root
            .as_deref()
            .is_some_and(|root| file.starts_with(root));
        self.rejected = !inside;
        self.accepted |= inside;
        inside.then_some(file)
    }
}

/// The entries and the context size one transcript line carries.
fn parse_line(line: &str) -> (Vec<Entry>, Option<u64>) {
    // A cheap filter before the parse: most lines are tool results.
    if !line.contains("\"assistant\"") {
        return (Vec::new(), None);
    }
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return (Vec::new(), None);
    };
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return (Vec::new(), None);
    }
    let ts = value
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let Some(message) = value.get("message") else {
        return (Vec::new(), None);
    };
    let tokens = message.get("usage").map(|usage| {
        [
            "input_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
            "output_tokens",
        ]
        .iter()
        .filter_map(|key| usage.get(*key).and_then(Value::as_u64))
        .fold(0u64, u64::saturating_add)
    });

    let mut entries = Vec::new();
    match message.get("content") {
        Some(Value::String(text)) => push_text(&mut entries, &ts, text),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            push_text(&mut entries, &ts, text);
                        }
                    }
                    Some("tool_use") => entries.push(Entry {
                        ts: ts.clone(),
                        kind: EntryKind::ToolUse {
                            name: block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("?")
                                .to_string(),
                            detail: tool_detail(block.get("input")),
                        },
                        text: String::new(),
                    }),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    (entries, tokens)
}

fn push_text(entries: &mut Vec<Entry>, ts: &str, text: &str) {
    let text = truncate(&collapse(text), TEXT_MAX);
    if !text.is_empty() {
        entries.push(Entry {
            ts: ts.to_string(),
            kind: EntryKind::Text,
            text,
        });
    }
}

/// `input.description`, else `input.command`, else `input.file_path`.
fn tool_detail(input: Option<&Value>) -> String {
    let Some(input) = input else {
        return String::new();
    };
    ["description", "command", "file_path"]
        .iter()
        .filter_map(|key| input.get(*key).and_then(Value::as_str))
        .map(collapse)
        .find(|s| !s.is_empty())
        .map(|s| truncate(&s, DETAIL_MAX))
        .unwrap_or_default()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters, the last replaced by `…` when cut.
pub(crate) fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("glimpse-transcript-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn text_line(ts: &str, text: &str) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": ts,
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]}
        })
        .to_string()
            + "\n"
    }

    fn tool_line(name: &str, input: Value) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-09-28T11:05:00Z",
            "message": {"content": [
                {"type": "thinking", "thinking": "hidden"},
                {"type": "tool_use", "id": "t", "name": name, "input": input}
            ], "usage": {"input_tokens": 10, "cache_read_input_tokens": 1000, "output_tokens": 5}}
        })
        .to_string()
            + "\n"
    }

    fn append(path: &Path, text: &str) {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open")
            .write_all(text.as_bytes())
            .expect("append");
    }

    fn texts(tail: &TailState) -> Vec<&str> {
        tail.entries.iter().map(|e| e.text.as_str()).collect()
    }

    #[test]
    fn refresh_reads_only_the_bytes_past_the_offset() {
        let root = temp_dir("incremental");
        let path = root.join("agent.jsonl");
        let first = text_line("2026-09-28T11:04:30Z", "one");
        let second = text_line("2026-09-28T11:04:31Z", "two");
        append(&path, &first);
        append(&path, &second);

        let mut tail = TailState::within(path.to_str().unwrap(), Some(root.clone()));
        assert!(tail.refresh());
        assert_eq!(texts(&tail), ["one", "two"]);
        assert_eq!(tail.offset, (first.len() + second.len()) as u64);
        assert!(!tail.refresh(), "nothing new, nothing changed");

        // Rewrite the consumed prefix in place at the same length: a tail that
        // re-read from the start would pick up "ONE"/"TWO".
        let third = text_line("2026-09-28T11:04:32Z", "three");
        let rewritten = first.replace("one", "ONE") + &second.replace("two", "TWO") + &third;
        std::fs::write(&path, &rewritten).expect("rewrite");
        assert!(tail.refresh());
        assert_eq!(texts(&tail), ["one", "two", "three"]);
        assert_eq!(tail.offset, rewritten.len() as u64);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_partial_last_line_is_held_back_until_its_newline_lands() {
        let root = temp_dir("partial");
        let path = root.join("agent.jsonl");
        let first = text_line("2026-09-28T11:04:30Z", "whole");
        let second = text_line("2026-09-28T11:04:31Z", "split");
        let (head, rest) = second.split_at(second.len() / 2);
        append(&path, &first);
        append(&path, head);

        let mut tail = TailState::within(path.to_str().unwrap(), Some(root.clone()));
        tail.refresh();
        assert_eq!(texts(&tail), ["whole"]);
        assert_eq!(tail.offset, first.len() as u64);

        append(&path, rest);
        assert!(tail.refresh());
        assert_eq!(texts(&tail), ["whole", "split"]);
        assert_eq!(tail.offset, (first.len() + second.len()) as u64);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_path_outside_the_root_is_rejected_and_never_read() {
        let base = temp_dir("outside");
        let root = base.join("claude");
        let other = base.join("other");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&other).expect("other");
        let path = other.join("agent.jsonl");
        append(&path, &text_line("2026-09-28T11:04:30Z", "secret"));

        let mut tail = TailState::within(path.to_str().unwrap(), Some(root.clone()));
        assert!(!tail.refresh());
        assert!(tail.rejected);
        assert!(tail.entries.is_empty());
        assert_eq!(tail.offset, 0);

        // A traversal that starts inside the root is caught after canonicalising.
        let sneaky = root.join("..").join("other").join("agent.jsonl");
        let mut tail = TailState::within(sneaky.to_str().unwrap(), Some(root.clone()));
        assert!(!tail.refresh());
        assert!(tail.rejected);
        assert!(tail.entries.is_empty());

        let mut rootless = TailState::within(path.to_str().unwrap(), None);
        assert!(!rootless.refresh());
        assert!(rootless.rejected);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_first_read_takes_only_the_last_window_from_a_line_boundary() {
        let root = temp_dir("window");
        let path = root.join("agent.jsonl");
        let mut body = String::new();
        let mut n = 0;
        while body.len() as u64 <= 3 * READ_MAX {
            body += &text_line(
                "2026-09-28T11:04:30Z",
                &format!("line {n:05} padding padding"),
            );
            n += 1;
        }
        std::fs::write(&path, &body).expect("write");

        let mut tail = TailState::within(path.to_str().unwrap(), Some(root.clone()));
        tail.refresh();
        assert_eq!(tail.offset, body.len() as u64);
        let last = format!("line {:05} padding padding", n - 1);
        assert_eq!(
            tail.entries.last().map(|e| e.text.as_str()),
            Some(last.as_str())
        );
        assert!(tail.entries.len() <= KEEP_ENTRIES);
        let first_kept = format!("line {:05} padding padding", n - KEEP_ENTRIES);
        assert_eq!(tail.entries[0].text, first_kept);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_truncated_file_starts_the_tail_over() {
        let root = temp_dir("truncated");
        let path = root.join("agent.jsonl");
        append(&path, &text_line("2026-09-28T11:04:30Z", "old one"));
        append(&path, &text_line("2026-09-28T11:04:31Z", "old two"));
        let mut tail = TailState::within(path.to_str().unwrap(), Some(root.clone()));
        tail.refresh();

        std::fs::write(&path, text_line("2026-09-28T11:05:00Z", "new")).expect("replace");
        assert!(tail.refresh());
        assert_eq!(texts(&tail), ["new"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_detail_prefers_description_then_command_then_file_path() {
        let (entries, tokens) = parse_line(
            tool_line(
                "Bash",
                serde_json::json!({"command": "cargo test", "description": "Run the tests"}),
            )
            .trim_end(),
        );
        assert_eq!(tokens, Some(1015));
        assert_eq!(
            entries,
            [Entry {
                ts: "2026-09-28T11:05:00Z".to_string(),
                kind: EntryKind::ToolUse {
                    name: "Bash".to_string(),
                    detail: "Run the tests".to_string(),
                },
                text: String::new(),
            }]
        );

        let detail = |input: Value| match &parse_line(tool_line("T", input).trim_end()).0[0].kind {
            EntryKind::ToolUse { detail, .. } => detail.clone(),
            EntryKind::Text => panic!("a tool call"),
        };
        assert_eq!(
            detail(serde_json::json!({"command": "ls  -la\n"})),
            "ls -la"
        );
        assert_eq!(
            detail(serde_json::json!({"file_path": "src/a.rs"})),
            "src/a.rs"
        );
        assert_eq!(detail(serde_json::json!({"pattern": "x"})), "");
        let long = detail(serde_json::json!({"command": "x".repeat(200)}));
        assert_eq!(long.chars().count(), DETAIL_MAX);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn non_assistant_lines_give_no_entries() {
        let user =
            serde_json::json!({"type": "user", "message": {"content": "an \"assistant\" word"}})
                .to_string();
        assert!(parse_line(&user).0.is_empty());
        assert!(parse_line("{not json \"assistant\"").0.is_empty());
    }
}
