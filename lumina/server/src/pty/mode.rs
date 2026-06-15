//! Autonomous-vs-interactive MODE-SIGNAL contract + the single-source mode
//! resolver every skill/consumer reads (focus 1C.1 foundation).
//!
//! Two execution modes are selected by a BROAD "lumina-spawned / scheduler
//! context" signal (NOT "is-a-PTY-session"), so an orchestrator AND its non-PTY
//! subagent teammates resolve to the same mode:
//!
//!   * [`Mode::Interactive`] — a human-typed terminal invocation: live
//!     AskUserQuestion, the agent grills and defers freely. The FAIL-SAFE
//!     default.
//!   * [`Mode::Autonomous`] — a lumina-spawned / scheduler-driven run: durable-
//!     primitive comms only (live AUQ is structurally dead here — it
//!     auto-resolves empty in a no-TTY context and is buffered out of the JSONL
//!     tail), the agent takes more decisions and surfaces only HARD ones.
//!
//! The signal is carried by the [`LUMINA_AUTONOMOUS`] env var (injected at the
//! PTY spawn seam for the orchestrator and via static `settings.json` `env` for
//! independently-bootstrapped teammates — both are DOWNSTREAM tasks; this module
//! owns only the NAME + the resolver). The env var is plain process state and
//! therefore SPOOFABLE: anything in a human's shell profile / tmux / direnv could
//! set it and falsely read as autonomous, suppressing the human-decision gating
//! the interactive path exists for (research note seq15). So the signal is NOT
//! trusted bare — it is CORROBORATED against a server-verifiable provenance fact
//! (`pty_sessions.source = 'spawned'`, read via
//! [`lumina_core::repo::session_source`]). On any CONFLICT (env says autonomous
//! but the session was `ingested`) or ABSENCE (no session row yet — a brand-new
//! autonomous run has no spawned-correlation until its row lands), the resolver
//! fails SAFE to [`Mode::Interactive`] (seq30).
//!
//! [`resolve_mode`] is the PURE single source of truth for the decision;
//! [`resolve_mode_for_session`] is its DB-backed entry point that composes the
//! process-env signal with the provenance fact. Every consumer reads mode
//! through these — there is no second place the rule lives.

use lumina_core::db::DbClient;
use lumina_core::domain::SessionSource;
use lumina_core::error::AppError;
use lumina_core::repo;

/// The environment-variable NAME carrying the autonomous mode signal.
///
/// lumina injects `LUMINA_AUTONOMOUS=1` at the PTY spawn seam (the orchestrator)
/// and via static `settings.json` `env` (so agent-team teammates inherit it) —
/// see the downstream injection/propagation tasks. This module owns only the
/// name; it never sets the variable itself.
pub const LUMINA_AUTONOMOUS: &str = "LUMINA_AUTONOMOUS";

/// The execution mode a skill/consumer runs under (focus 1C.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Human-typed terminal invocation: interactive, live AskUserQuestion, the
    /// agent grills and defers freely. The FAIL-SAFE default — chosen whenever
    /// the autonomous signal is absent, unverified, or conflicts with the
    /// server-recorded session provenance.
    Interactive,
    /// Lumina-spawned / scheduler-driven autonomous run: durable-primitive comms
    /// only, the agent takes more decisions and surfaces only HARD ones. Selected
    /// ONLY when the env signal is present AND corroborated by
    /// `pty_sessions.source = 'spawned'`.
    Autonomous,
}

impl Mode {
    /// `true` for [`Mode::Autonomous`] — the convenience predicate consumers use
    /// to branch their comms behaviour.
    pub fn is_autonomous(self) -> bool {
        matches!(self, Mode::Autonomous)
    }
}

/// Parse a raw [`LUMINA_AUTONOMOUS`] value into the autonomous-signal boolean.
///
/// Truthy (case-insensitive, trimmed): `1` / `true` / `yes` / `on`. EVERYTHING
/// else — `None` (unset), empty / whitespace, `0` / `false`, or any other
/// string — is NOT a signal, so an ambiguous or malformed value can never push
/// the resolver toward autonomous (fail-safe). Pure (no env read) so the
/// truthiness rule is unit-testable without process-global env mutation.
pub fn parse_autonomous_flag(value: Option<&str>) -> bool {
    matches!(
        value.map(|v| v.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// Read the process-environment autonomous signal via [`parse_autonomous_flag`].
pub fn env_autonomous_flag() -> bool {
    parse_autonomous_flag(std::env::var(LUMINA_AUTONOMOUS).ok().as_deref())
}

/// The PURE mode-resolution rule — the single source of truth for the
/// autonomous-mode decision.
///
/// Autonomous ⟺ the env signal is present AND the server-verifiable provenance
/// is `Spawned`. EVERY other combination — signal absent, a conflicting
/// `Ingested` source, or no session row at all (`None`) — fails SAFE to
/// [`Mode::Interactive`] (research notes seq15 + seq30). Kept free of any env /
/// DB read so the whole truth table is exhaustively unit-testable.
pub fn resolve_mode(env_autonomous: bool, source: Option<SessionSource>) -> Mode {
    match (env_autonomous, source) {
        (true, Some(SessionSource::Spawned)) => Mode::Autonomous,
        _ => Mode::Interactive,
    }
}

/// Resolve the mode for a concrete session: compose the process-env signal
/// ([`env_autonomous_flag`]) with the server-verifiable provenance fact
/// ([`lumina_core::repo::session_source`]) through the pure [`resolve_mode`]
/// rule. The single DB-backed entry point a server-side consumer calls.
pub async fn resolve_mode_for_session(
    db: &impl DbClient,
    session_id: &str,
) -> Result<Mode, AppError> {
    let source = repo::session_source(db, session_id).await?;
    Ok(resolve_mode(env_autonomous_flag(), source))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fail-safe truth table — the security-critical core of 1C.1. Only one
    /// combination yields [`Mode::Autonomous`]; every other fails safe.
    #[test]
    fn resolve_mode_truth_table() {
        // The ONLY autonomous combination: signal present + spawned provenance.
        assert_eq!(
            resolve_mode(true, Some(SessionSource::Spawned)),
            Mode::Autonomous,
            "env signal corroborated by a spawned session ⇒ autonomous"
        );

        // Fail-safe to interactive on every other combination.
        assert_eq!(
            resolve_mode(false, Some(SessionSource::Spawned)),
            Mode::Interactive,
            "no env signal ⇒ interactive even on a spawned session"
        );
        assert_eq!(
            resolve_mode(true, Some(SessionSource::Ingested)),
            Mode::Interactive,
            "conflict (env autonomous but ingested) ⇒ fail safe to interactive"
        );
        assert_eq!(
            resolve_mode(true, None),
            Mode::Interactive,
            "absence (env autonomous but no session row) ⇒ fail safe to interactive"
        );
        assert_eq!(resolve_mode(false, None), Mode::Interactive);
        assert_eq!(
            resolve_mode(false, Some(SessionSource::Ingested)),
            Mode::Interactive
        );
    }

    #[test]
    fn mode_is_autonomous_predicate() {
        assert!(Mode::Autonomous.is_autonomous());
        assert!(!Mode::Interactive.is_autonomous());
    }

    #[test]
    fn parse_autonomous_flag_truthy_set() {
        for v in ["1", "true", "TRUE", "Yes", " on ", "On"] {
            assert!(
                parse_autonomous_flag(Some(v)),
                "{v:?} should signal autonomous"
            );
        }
        for v in ["0", "false", "", "  ", "no", "off", "enabled", "2"] {
            assert!(
                !parse_autonomous_flag(Some(v)),
                "{v:?} should NOT signal autonomous (fail-safe)"
            );
        }
        assert!(
            !parse_autonomous_flag(None),
            "unset ⇒ not autonomous (fail-safe)"
        );
    }
}
