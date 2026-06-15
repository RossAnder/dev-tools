//! Autonomous-vs-interactive MODE-SIGNAL contract + the single-source mode
//! resolver every skill/consumer reads (focus 1C.1 foundation).
//!
//! Two execution modes are selected by a SERVER-ISSUED SECRET TOKEN that proves a
//! session (or one of its teammates) was launched by THIS lumina process:
//!
//!   * [`Mode::Interactive`] — a human-typed terminal invocation: live
//!     AskUserQuestion, the agent grills and defers freely. The FAIL-SAFE
//!     default.
//!   * [`Mode::Autonomous`] — a lumina-spawned / scheduler-driven run: durable-
//!     primitive comms only (live AUQ is structurally dead here — it
//!     auto-resolves empty in a no-TTY context and is buffered out of the JSONL
//!     tail), the agent takes more decisions and surfaces only HARD ones.
//!
//! ## Why a token, not a session-source corroboration
//!
//! The mode signal is carried by the [`LUMINA_AUTONOMOUS`] env var, which is plain
//! process state and therefore SPOOFABLE: anything in a human's shell profile /
//! tmux / direnv could set it and falsely read as autonomous, suppressing the
//! human-decision gating the interactive path exists for. So the env var must NOT
//! be trusted bare. An earlier design corroborated it against a server-verifiable
//! provenance fact (`pty_sessions.source = 'spawned'`), but that has a fatal gap:
//! a Task-spawned TEAMMATE of an autonomous orchestrator has NO `pty_sessions`
//! row of its own (only the orchestrator was spawned through the PTY supervisor),
//! so source-based corroboration can never confirm a teammate as autonomous.
//!
//! Instead the env var now carries a per-process SECRET TOKEN that lumina mints
//! once at startup ([`autonomous_secret`]) and injects — both into the spawned
//! orchestrator's environment AND, via the propagated `settings.json` `env`, into
//! its teammates. A server-side consumer VERIFIES the presented token against the
//! live secret ([`verify_token`]): a match proves the value came from this
//! process's injection, not a stray shell export. This works UNIFORMLY for the
//! orchestrator and its teammates — they all carry the same minted token — which
//! the session-source model could not. A stray `LUMINA_AUTONOMOUS=…` in a human
//! shell holds no valid token, so [`verify_token`] rejects it and the resolver
//! fails SAFE to [`Mode::Interactive`].
//!
//! ## The single source of truth
//!
//! [`resolve_mode`] is the PURE rule keyed on a verified-token boolean — the one
//! place the decision lives, exhaustively unit-testable without env mutation.
//! [`resolve_mode_from_env`] is the convenience entry point that reads the current
//! process's [`LUMINA_AUTONOMOUS`] env value, verifies it, and resolves. Every
//! consumer reads mode through these — there is no second place the rule lives.

use std::sync::OnceLock;

/// The environment-variable NAME carrying the autonomous mode signal.
///
/// The value of this variable is the SECRET TOKEN (the [`autonomous_secret`]
/// string), NOT a `1`/`true` flag: at the PTY spawn seam lumina injects
/// `LUMINA_AUTONOMOUS=<autonomous_secret()>` into the spawned orchestrator's
/// environment, and the same token propagates to teammates via the static
/// `settings.json` `env` block. A consumer proves autonomy by VERIFYING the
/// presented value with [`verify_token`] — the bare presence of the variable is
/// never sufficient. This module owns only the name; it never sets the variable
/// itself (the spawn seam does).
pub const LUMINA_AUTONOMOUS: &str = "LUMINA_AUTONOMOUS";

/// The per-process autonomous secret, minted once on first read.
///
/// Trade-off (deliberate, documented): the secret is minted from
/// [`uuid::Uuid::now_v7`] — ~74 random bits, NOT a cryptographic RNG — and is
/// PER-PROCESS and NEVER PERSISTED (regenerated on every server restart, held only
/// in this `OnceLock`). That is sufficient for an in-process, ephemeral
/// corroboration check whose sole job is to distinguish lumina-injected env from a
/// stray human-shell export: an attacker would have to read this process's memory
/// or env to forge it, at which point they already have host access. It is NOT
/// suitable as a cross-process or persistent credential — do not reuse it as one.
static AUTONOMOUS_SECRET: OnceLock<String> = OnceLock::new();

/// The execution mode a skill/consumer runs under (focus 1C.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Human-typed terminal invocation: interactive, live AskUserQuestion, the
    /// agent grills and defers freely. The FAIL-SAFE default — chosen whenever the
    /// presented token is absent, empty, or does not match this process's secret.
    Interactive,
    /// Lumina-spawned / scheduler-driven autonomous run: durable-primitive comms
    /// only, the agent takes more decisions and surfaces only HARD ones. Selected
    /// ONLY when the presented [`LUMINA_AUTONOMOUS`] token VERIFIES against this
    /// process's [`autonomous_secret`].
    Autonomous,
}

impl Mode {
    /// `true` for [`Mode::Autonomous`] — the convenience predicate consumers use
    /// to branch their comms behaviour.
    pub fn is_autonomous(self) -> bool {
        matches!(self, Mode::Autonomous)
    }
}

/// Return this process's autonomous secret, minting it once on first call.
///
/// The spawn seam reads this to inject `LUMINA_AUTONOMOUS=<secret>` into spawned
/// sessions and propagate it to their teammates; a server-side consumer compares
/// a presented token against it via [`verify_token`]. Repeated calls return the
/// SAME value for the life of the process (the `OnceLock` guarantees stability).
/// See [`AUTONOMOUS_SECRET`] for the per-process / non-persistent trade-off.
pub fn autonomous_secret() -> &'static str {
    AUTONOMOUS_SECRET.get_or_init(|| uuid::Uuid::now_v7().to_string())
}

/// Verify a PRESENTED token against this process's [`autonomous_secret`].
///
/// `true` iff `presented` is non-empty AND equals the live secret. An empty or
/// mismatched value is rejected (fail-safe). The comparison runs over the FULL
/// length of both strings — it deliberately avoids an early length-mismatch
/// return so a wrong-but-same-length and a wrong-and-different-length token take
/// the same path; the secret is not a long-lived credential (see
/// [`AUTONOMOUS_SECRET`]) so this is belt-and-braces, not a hard timing
/// guarantee.
pub fn verify_token(presented: &str) -> bool {
    if presented.is_empty() {
        return false;
    }
    let secret = autonomous_secret();
    // Fold a full-length comparison into one accumulator rather than returning
    // early on the first differing byte (or on a length mismatch). `zip` stops at
    // the shorter input, so a length difference is captured by the explicit
    // length check folded into the same accumulator.
    let mut diff = (presented.len() != secret.len()) as u8;
    for (a, b) in presented.bytes().zip(secret.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// The PURE mode-resolution rule — the single source of truth for the
/// autonomous-mode decision.
///
/// Autonomous ⟺ the presented token VERIFIED (`token_valid == true`). Every other
/// case — no token, an empty token, or a mismatched token — fails SAFE to
/// [`Mode::Interactive`]. Kept free of any env / secret read so the whole truth
/// table is exhaustively unit-testable without process-global env mutation; the
/// env-reading + verification composition lives in [`resolve_mode_from_env`].
pub fn resolve_mode(token_valid: bool) -> Mode {
    if token_valid {
        Mode::Autonomous
    } else {
        Mode::Interactive
    }
}

/// Resolve the mode for the CURRENT process: read the [`LUMINA_AUTONOMOUS`] env
/// value, [`verify_token`] it against this process's secret, and feed the result
/// through the pure [`resolve_mode`] rule. The convenience a server-side consumer
/// calls when it wants the mode of the env it is itself running under.
///
/// An unset variable yields an empty borrow, which `verify_token` rejects, so an
/// absent signal fails SAFE to [`Mode::Interactive`].
pub fn resolve_mode_from_env() -> Mode {
    let value = std::env::var(LUMINA_AUTONOMOUS).unwrap_or_default();
    resolve_mode(verify_token(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fail-safe truth table — the security-critical core of 1C.1. The
    /// decision now keys on a VERIFIED-token boolean: only a valid token yields
    /// [`Mode::Autonomous`]; anything else fails safe.
    #[test]
    fn resolve_mode_truth_table() {
        assert_eq!(
            resolve_mode(true),
            Mode::Autonomous,
            "a verified token ⇒ autonomous"
        );
        assert_eq!(
            resolve_mode(false),
            Mode::Interactive,
            "no / invalid token ⇒ fail safe to interactive"
        );
    }

    #[test]
    fn mode_is_autonomous_predicate() {
        assert!(Mode::Autonomous.is_autonomous());
        assert!(!Mode::Interactive.is_autonomous());
    }

    #[test]
    fn verify_token_accepts_only_the_live_secret() {
        let secret = autonomous_secret();
        assert!(
            verify_token(secret),
            "the live secret must verify against itself"
        );
    }

    #[test]
    fn verify_token_rejects_wrong_and_empty() {
        let secret = autonomous_secret();

        assert!(!verify_token(""), "an empty token must never verify");
        assert!(
            !verify_token("not-the-secret"),
            "a wrong token must never verify"
        );
        // A same-length-but-wrong token (mutate one byte of the real secret) must
        // also fail — proving the check compares content, not just length.
        let mut wrong = secret.to_string();
        let last = wrong.pop().expect("secret is non-empty");
        wrong.push(if last == 'a' { 'b' } else { 'a' });
        assert_eq!(wrong.len(), secret.len(), "mutation preserved the length");
        assert!(
            !verify_token(&wrong),
            "a same-length but differing token must never verify"
        );
    }

    #[test]
    fn autonomous_secret_is_stable_across_calls() {
        // The OnceLock guarantees one minted value for the life of the process.
        assert_eq!(
            autonomous_secret(),
            autonomous_secret(),
            "two reads of the per-process secret must be identical"
        );
    }

    #[test]
    fn resolve_mode_from_env_fails_safe_on_a_garbage_value() {
        // A stray, NON-token value in the env (the human-shell-export attack)
        // must NOT read as autonomous. Use a value that cannot equal a uuid
        // secret so the assertion holds regardless of the minted secret.
        // SAFETY: single-threaded unit test; no other thread reads the env here.
        unsafe {
            std::env::set_var(LUMINA_AUTONOMOUS, "1");
        }
        assert_eq!(
            resolve_mode_from_env(),
            Mode::Interactive,
            "a bare '1' (legacy flag / stray export) carries no valid token ⇒ interactive"
        );
        unsafe {
            std::env::remove_var(LUMINA_AUTONOMOUS);
        }
        assert_eq!(
            resolve_mode_from_env(),
            Mode::Interactive,
            "an unset variable ⇒ interactive"
        );
    }
}
