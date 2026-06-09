//! Transport seam for PTY-style interactive sessions.
//!
//! `Transport` is a pluggable seam — `PtyTransport` (T4) is the only
//! implementation in this plan; ACP / remote slots are reserved but
//! unimplemented. The supervisor (T8) drives one `Transport` per session and
//! hands the resulting `TransportHandle` to per-session bookkeeping.

use std::path::PathBuf;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use lumina_core::error::AppError;
use lumina_core::protocol::{InputFrame, SessionId, TypedMessage};

/// Spawn-time configuration for a transport session. Fields cover everything a
/// `claude` REPL spawn needs to know — working directory, CLI args, optional
/// agent/model/settings JSON, and OTEL env passthrough. The legacy
/// `prompt_pattern` field was removed when the vt100 parser was retired in
/// favour of JSONL-tail message extraction (`pty/jsonl_tail.rs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SpawnConfig {
    pub cwd: PathBuf,
    pub claude_args: Vec<String>,
    pub agent_json: Option<String>,
    pub model: Option<String>,
    pub env_passthrough_otel: bool,
    pub settings_json: Option<String>,
}

/// Terminal exit summary for a transport session. `code` is the process exit
/// status when available; `signal` is the terminating signal on Unix. `success`
/// is the canonical "did this end cleanly?" boolean the supervisor records on
/// the session row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SessionExit {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub success: bool,
}

/// Per-session handle returned by `Transport::spawn`. Deliberately NOT `Clone`:
/// `broadcast::Receiver` is not Clone, and consumers re-subscribe via the
/// `broadcast::Sender` stored elsewhere in the supervisor (T8).
pub struct TransportHandle {
    pub session_id: SessionId,
    pub outbound: broadcast::Receiver<TypedMessage>,
    pub inbound: mpsc::Sender<InputFrame>,
    pub shutdown: CancellationToken,
    pub completed: oneshot::Receiver<SessionExit>,
}

/// Pluggable transport for interactive sessions. The only implementation in
/// this plan is `PtyTransport` (T4); ACP / remote slots are reserved but
/// unimplemented.
#[async_trait]
pub trait Transport: Send + Sync {
    async fn spawn(&self, config: SpawnConfig) -> Result<TransportHandle, AppError>;
}
