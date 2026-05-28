//! PTY wire types — typed messages, session identifiers, and input frames.
//!
//! These types are the protocol surface between the PTY supervisor and its
//! clients (the MCP/HTTP layers and the SPA). Snake_case is the wire
//! convention for every enum; owned strings throughout (no `Cow`).
//!
//! `created_at` is a `String` (RFC3339 / `CURRENT_TIMESTAMP` rendering) to
//! match the existing `domain.rs` convention — every row struct in this
//! crate stores timestamps as `String`. The plan named `jiff::Timestamp` as
//! the preferred type, but the workspace's `jiff = "0.2"` dependency does
//! not enable the `serde` feature (T1 owns Cargo.toml, so widening it from
//! T2 is out of scope), and `domain.rs` is unambiguous on the convention.
//! The supervisor (T8) mints timestamps via `jiff::Timestamp::now().to_string()`
//! exactly as `export.rs` already does.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identifier for a PTY-backed `claude` session. UUIDv7 so the wire form is
/// sortable by mint-time. `serde(transparent)` makes the JSON form a bare
/// uuid string (no `{ "0": "…" }` wrapper).
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Mint a fresh UUIDv7-backed session id.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Underlying uuid.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Hyphenated form (the uuid crate's default Display).
        fmt::Display::fmt(&self.0, f)
    }
}

/// Lifecycle status of a session.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Spawning,
    Active,
    Idle,
    Awaiting,
    Completed,
    Failed,
    Cancelled,
}

impl SessionStatus {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Spawning => "spawning",
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Awaiting => "awaiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

impl FromStr for SessionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "spawning" => Ok(Self::Spawning),
            "active" => Ok(Self::Active),
            "idle" => Ok(Self::Idle),
            "awaiting" => Ok(Self::Awaiting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(format!("unknown session status: {other}")),
        }
    }
}

/// Classification of a [`TypedMessage`] — drives per-kind rendering and the
/// per-kind `content` payload shape on the SPA. Six variants matching the
/// JSONL-tail taxonomy (post lumina-pty-jsonl-tail, T5): the prior eight-
/// variant set carrying `ToolCall|Prompt|ParserUnknown` was removed when the
/// vt100 parser was deleted in favour of reading the canonical structured
/// transcript from Claude Code's session JSONL.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    UserInput,
    AssistantText,
    ToolUse,
    ToolResult,
    System,
    Error,
}

impl MessageKind {
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::UserInput => "user_input",
            Self::AssistantText => "assistant_text",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
            Self::System => "system",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for MessageKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

impl FromStr for MessageKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user_input" => Ok(Self::UserInput),
            "assistant_text" => Ok(Self::AssistantText),
            "tool_use" => Ok(Self::ToolUse),
            "tool_result" => Ok(Self::ToolResult),
            "system" => Ok(Self::System),
            "error" => Ok(Self::Error),
            other => Err(format!("unknown message kind: {other}")),
        }
    }
}

/// One ordered message in a session's transcript. `sequence` is monotone
/// within a session (assigned by the supervisor); `content` is a per-kind
/// JSON payload (no enum proliferation — the kind discriminator says how to
/// read it). `raw_text` carries the original PTY chunk when useful for
/// debugging or replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TypedMessage {
    pub sequence: i64,
    pub kind: MessageKind,
    pub content: serde_json::Value,
    pub raw_text: Option<String>,
    pub created_at: String,
    /// Tool-use correlation id. Populated on `ToolUse` (the id from the JSONL
    /// `assistant.content.tool_use` block) AND on `ToolResult` (the id from
    /// the JSONL `user.content.tool_result.tool_use_id` field). The web SPA
    /// pairs `ToolResult` rows back to their `ToolUse` parent via this field.
    /// `None` on every other kind.
    pub tool_use_id: Option<String>,
}

/// Classification of an [`InputFrame`] coming from a client.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Prompt,
    Cancel,
    Control,
    Keystroke,
}

impl InputKind {
    fn as_wire(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Cancel => "cancel",
            Self::Control => "control",
            Self::Keystroke => "keystroke",
        }
    }
}

impl fmt::Display for InputKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// One client→supervisor input frame. `payload` semantics depend on `kind`
/// (prompt text, control sequence, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct InputFrame {
    pub kind: InputKind,
    pub payload: String,
}
