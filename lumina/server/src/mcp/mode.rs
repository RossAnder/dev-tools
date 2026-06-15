//! MCP execution-mode read tool — the focus-1C.1 corroborated-resolver
//! CONSUMER. The single `read_only_hint = true` tool here lets a caller
//! (primarily a Task-spawned TEAMMATE, which inherits the [`LUMINA_AUTONOMOUS`]
//! token via the propagated `settings.json` `env` block and has `/mcp` through
//! the project MCP config) CORROBORATE its execution mode SERVER-SIDE — the
//! production consumer that makes the corroborated resolver in
//! [`crate::pty::mode`] live.
//!
//! `get_execution_mode` and its `*Params`/result structs live here; the tool
//! registers via the `tool_router_mode` sub-router, summed into the combined
//! field by [`LuminaTools::with_state`].
//!
//! ## Why server-side, and the fail-safe semantics
//!
//! The [`LUMINA_AUTONOMOUS`] env var is plain process state and therefore
//! SPOOFABLE — a stray shell export could falsely read as autonomous and
//! suppress the human-decision gating the interactive path exists for. The
//! caller cannot trust the bare presence of the variable; instead it PRESENTS
//! its env value to this tool, which VERIFIES it against THIS server process's
//! per-process secret ([`crate::pty::mode::verify_token`]) and resolves the mode
//! through the pure single-source rule ([`crate::pty::mode::resolve_mode`]). A
//! valid token (one minted by, and injected by, this process) resolves to
//! `"autonomous"`; a present-but-invalid, empty, or absent token resolves to
//! `"interactive"` (the FAIL-SAFE default).
//!
//! ## Composition (read-only, no DB, no migration, no write)
//!
//! This tool touches NO database, issues NO SQL, and records NO write/event. It
//! is a pure call into the [`crate::pty::mode`] resolver — the secret-verify +
//! mode-resolve chain — and maps the resulting [`Mode`] to its lowercase wire
//! string.

use super::*;

use crate::pty::mode::{Mode, resolve_mode, verify_token};

/// Arguments for the `get_execution_mode` read tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetExecutionModeParams {
    /// The token the caller presents for corroboration — the value of its
    /// `LUMINA_AUTONOMOUS` env var. It is VERIFIED against this server process's
    /// per-process secret; only a token this process minted + injected resolves
    /// to `"autonomous"`. An empty / absent / mismatched value fails safe to
    /// `"interactive"`.
    pub token: String,
}

/// The resolved execution mode, as a lowercase wire string.
///
/// `mode` is always exactly `"autonomous"` or `"interactive"` — the lowercase
/// rendering of [`Mode::Autonomous`] / [`Mode::Interactive`]. A
/// present-but-invalid or empty token yields `"interactive"` (fail-safe).
#[derive(Debug, serde::Serialize)]
pub struct ExecutionMode {
    /// `"autonomous"` iff the presented token VERIFIED against this process's
    /// secret; otherwise `"interactive"` (the fail-safe default).
    pub mode: &'static str,
}

/// Map a resolved [`Mode`] to its lowercase wire string.
fn mode_wire(mode: Mode) -> &'static str {
    match mode {
        Mode::Autonomous => "autonomous",
        Mode::Interactive => "interactive",
    }
}

#[tool_router(router = tool_router_mode, vis = "pub(crate)")]
impl LuminaTools {
    // ---- Execution-mode read tool (read_only_hint = true) ----------------

    /// Corroborate the caller's execution mode SERVER-SIDE: verify the presented
    /// `token` against this process's per-process autonomous secret
    /// ([`crate::pty::mode::verify_token`]) and resolve the mode through the pure
    /// single-source rule ([`crate::pty::mode::resolve_mode`]). Read-only: no DB,
    /// no SQL, no write, no event. Returns `{ "mode": "autonomous" | "interactive" }`.
    ///
    /// A valid token (one minted + injected by this process — carried into a
    /// spawned orchestrator's env and propagated to its teammates via
    /// `settings.json`) resolves to `"autonomous"`; a present-but-invalid, empty,
    /// or absent token resolves to `"interactive"` (the FAIL-SAFE default).
    #[tool(
        description = "Corroborate this caller's execution mode server-side: present the LUMINA_AUTONOMOUS token; a token this process minted resolves to \"autonomous\", anything else (empty/invalid) fails safe to \"interactive\". Read-only; no DB.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_execution_mode(
        &self,
        Parameters(GetExecutionModeParams { token }): Parameters<GetExecutionModeParams>,
    ) -> Result<CallToolResult, ErrorData> {
        tracing::debug!(tool = "get_execution_mode", "mcp tool invoked");

        // Pure resolver chain — NO DB, NO SQL, NO write. `verify_token` compares
        // the presented value against this process's per-process secret;
        // `resolve_mode` is the single-source rule (autonomous ⟺ verified token,
        // else the fail-safe interactive default).
        let mode = resolve_mode(verify_token(&token));

        // Return a STRUCTURED result: `CallToolResult::structured` populates both
        // `structured_content` (the consumer reads `mode` directly) and a
        // JSON-text content mirror. `serde_json::to_value` on this owned
        // `&'static str` struct is effectively infallible, but is mapped to
        // `internal_error` rather than unwrapped (matching the module convention).
        let value = serde_json::to_value(ExecutionMode {
            mode: mode_wire(mode),
        })
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        structured_result(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::mode::autonomous_secret;
    use lumina_core::db::connect_in_memory;

    /// Drive the `get_execution_mode` tool directly and read the `mode` string
    /// out of the structured payload.
    async fn resolved_mode(tools: &LuminaTools, token: &str) -> String {
        let result = tools
            .get_execution_mode(Parameters(GetExecutionModeParams {
                token: token.to_owned(),
            }))
            .await
            .expect("get_execution_mode succeeds");
        assert_eq!(result.is_error, Some(false), "read tool is not an error");
        result
            .structured_content
            .expect("structured execution-mode payload")
            .get("mode")
            .and_then(|v| v.as_str())
            .expect("mode string")
            .to_owned()
    }

    /// The live per-process secret VERIFIES → `"autonomous"`; an empty token and
    /// a garbage token both fail safe → `"interactive"`. Mirrors the
    /// `pty::mode::verify_token` truth table at the MCP boundary.
    #[tokio::test]
    async fn resolves_autonomous_for_the_live_secret_and_fails_safe_otherwise() {
        let pool = Arc::new(AnyPool::from(connect_in_memory().await.expect("pool")));
        let tools = LuminaTools::new(pool);

        // A valid token (this process's minted secret) resolves to autonomous.
        assert_eq!(
            resolved_mode(&tools, autonomous_secret()).await,
            "autonomous",
            "the live per-process secret verifies ⇒ autonomous"
        );

        // An empty token fails safe to interactive.
        assert_eq!(
            resolved_mode(&tools, "").await,
            "interactive",
            "an empty token carries no valid secret ⇒ interactive (fail-safe)"
        );

        // A garbage (non-token) value — the human-shell-export attack — fails
        // safe to interactive.
        assert_eq!(
            resolved_mode(&tools, "not-the-secret").await,
            "interactive",
            "a garbage token never verifies ⇒ interactive (fail-safe)"
        );
    }
}
