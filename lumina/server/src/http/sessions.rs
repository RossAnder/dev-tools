//! Harness session-corpus HTTP ingest (migration 0015, ADR-0004 layer 2).
//!
//! Exposes a SINGLE route — `POST /api/sessions/ingest` — that the Claude Code
//! `SessionEnd` http-hook posts to when a session ends. The hook
//! fires-and-forgets: this handler validates+confines the supplied
//! `transcript_path`, returns `202 Accepted` (empty body) IMMEDIATELY, and
//! `tokio::spawn`s the DB-bound ingest behind a [`Semaphore`](tokio::sync::Semaphore)
//! permit (`state.session_ingest_sem`; 4 by default, injectable via
//! [`AppState::with_ingest_permits`] for back-pressure tests). The ingest is best-effort —
//! any failure is logged via `tracing::warn!` and swallowed (never a 500), and
//! re-ingest is idempotent, so a dropped/abandoned spawn is safe (the hook
//! tolerates loss by design).
//!
//! ## SECURITY — unauthenticated arbitrary-file-read boundary (P2 critical)
//!
//! This endpoint is **UNAUTHENTICATED**: it inherits lumina's loopback-only
//! deployment rule (the `/api/*` surface has no auth and no Host-header check —
//! see `app::DEFAULT_HOST`). **Never expose lumina non-loopback** (do not bind
//! `0.0.0.0` / sit behind a reverse proxy reachable from a hostile network)
//! without a permission wrapper.
//!
//! The caller supplies `transcript_path`, and the ingest reads that file and
//! stores it VERBATIM in `session_records` (redaction is a later layer). An
//! un-validated path is therefore an arbitrary-file-read primitive: a local
//! caller could name `/etc/passwd`, `~/.aws/credentials`, or `~/.claude.json`
//! and read it back out of the corpus. The harvest drop-gate (no
//! `mcp__lumina__*` tool_use ⇒ Dropped) is only a PARTIAL accidental mitigation
//! — one synthetic `mcp__lumina__` line in the target file defeats it — so it is
//! NOT the security control.
//!
//! The control is [`confine_transcript_path`]: BEFORE the 202, and BEFORE any
//! spawn, the handler canonicalises both the allowed transcript root
//! (`~/.claude/projects`, via [`lumina_core::jsonl_tail::resolve_projects_root`])
//! and the caller's path, then requires the canonical path to live INSIDE the
//! canonical root. `std::fs::canonicalize` resolves `..` and symlinks and
//! requires the target to exist, so a symlink whose target is outside the root
//! canonicalises to an outside path and fails the `starts_with` check. On
//! Windows both canonical forms carry the same `\\?\` verbatim prefix, so the
//! prefix-match composes correctly because BOTH sides are canonicalised. Any
//! validation failure returns a 4xx and spawns NOTHING.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use serde::Deserialize;

use crate::app::AppState;
use lumina_core::repo;

/// Body of `POST /api/sessions/ingest`, matching the Claude Code `SessionEnd`
/// http-hook payload. `hook_event_name` and `reason` are accepted-and-IGNORED
/// (kept on the struct so deserialise does not fail on the hook's full payload).
#[derive(Debug, Deserialize)]
struct IngestBody {
    /// The owning session id (becomes `pty_sessions.id` on ingest).
    session_id: String,
    /// The absolute path to the session JSONL transcript. UNTRUSTED — confined
    /// to the `~/.claude/projects` root before use (see module docs).
    transcript_path: String,
    /// The session's working directory (stored lexically; resolves the project
    /// floor inside `repo::ingest_transcript`).
    cwd: String,
    /// Accepted and ignored (e.g. `"SessionEnd"`).
    #[serde(default)]
    #[allow(dead_code)]
    hook_event_name: Option<String>,
    /// Accepted and ignored (e.g. `"clear"`, `"logout"`, `"exit"`).
    #[serde(default)]
    #[allow(dead_code)]
    reason: Option<String>,
}

/// Build the sessions sub-router. Returned as `Router<AppState>` so
/// `http::router` can `.merge` it. The path is RELATIVE to the `/api` nest
/// (`app.rs` nests this under `/api`), so `/sessions/ingest` here resolves to
/// `/api/sessions/ingest`.
pub fn router() -> Router<AppState> {
    Router::new().route("/sessions/ingest", post(ingest_session))
}

/// Validate and confine an untrusted `transcript_path` to the `~/.claude/projects`
/// transcript root, returning the VALIDATED CANONICAL path on success.
///
/// Returns `Err(StatusCode)` on ANY of:
///   * the raw path contains a `..` (`ParentDir`) component (defence-in-depth,
///     rejected before touching the filesystem) → `400 BAD_REQUEST`;
///   * the path fails to canonicalise — non-existent, unreadable, or a broken
///     symlink (`std::fs::canonicalize` requires the target to exist) →
///     `400 BAD_REQUEST`;
///   * the confinement ROOT cannot be RESOLVED at all — the STRICT resolver
///     (`resolve_projects_root_strict`) returns `None` because neither
///     `LUMINA_PTY_PROJECTS_ROOT` nor HOME/USERPROFILE is set →
///     `500 INTERNAL_SERVER_ERROR`. This deliberately does NOT fall back to the
///     process CWD (R8): a CWD confinement root would expose the repo tree to the
///     arbitrary-file-read primitive, so an unresolvable root is a server
///     misconfiguration we reject, not silently widen;
///   * the resolved root is RESOLVABLE but does not yet exist on disk (a fresh
///     machine that has never run `claude`, so `~/.claude/projects` is absent)
///     and therefore fails to canonicalise → `500 INTERNAL_SERVER_ERROR` (still a
///     server-side condition, not the caller's fault — once `claude` has run at
///     least once on this machine the dir exists and ingest proceeds);
///   * the canonical path is NOT inside the canonical root → `403 FORBIDDEN`.
///
/// Both `500` paths are logged once via `tracing::warn!` (the caller's 202 has
/// NOT been sent — the confinement runs synchronously before any spawn).
///
/// The check runs on the CANONICAL forms of both sides, so symlink-escape is
/// defeated: a link pointing outside the root canonicalises to its outside
/// target and fails `Path::starts_with`. Canonicalising BOTH sides keeps the
/// Windows `\\?\` verbatim prefix consistent across the comparison.
fn confine_transcript_path(transcript_path: &str) -> Result<PathBuf, StatusCode> {
    let raw = Path::new(transcript_path);

    // Defence-in-depth: reject a literal `..` traversal component up-front,
    // before any filesystem syscall. (canonicalize would also resolve it, but
    // an early reject avoids resolving an attacker-controlled `..` chain at all.)
    if raw.components().any(|c| matches!(c, Component::ParentDir)) {
        tracing::warn!(
            transcript_path = %transcript_path,
            "session ingest rejected: transcript_path contains a `..` component"
        );
        return Err(StatusCode::BAD_REQUEST);
    }

    // Resolve the confinement ROOT via the STRICT resolver (R8): unlike the
    // best-effort live-tail resolver, this NEVER falls back to the process CWD
    // when HOME/USERPROFILE is unset. An unset-HOME CWD fallback would silently
    // collapse the security boundary onto the repo tree (exposing `.git` / in-tree
    // secrets to the arbitrary-file-read primitive), so a `None` here is a server
    // misconfiguration we REJECT rather than confine against — 500, never CWD.
    let Some(root) = lumina_core::jsonl_tail::resolve_projects_root_strict() else {
        tracing::warn!(
            "session ingest rejected: cannot resolve a confinement root \
             (HOME/USERPROFILE unset and LUMINA_PTY_PROJECTS_ROOT not set)"
        );
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    // Canonicalise the transcript root once. Resolving `..`/symlinks here and on
    // the candidate makes the prefix check a faithful confinement test. If the
    // root is resolvable but the dir does not exist on disk yet (a fresh machine
    // that has never run `claude`), canonicalize fails → 500 (a server-side
    // misconfiguration, not caller input).
    let canonical_root = std::fs::canonicalize(&root).map_err(|e| {
        tracing::warn!(
            root = %root.display(),
            error = %e,
            "session ingest: transcript root failed to canonicalise"
        );
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    // Canonicalise the caller's path. This resolves symlinks AND `..` and
    // REQUIRES the file to exist — a non-existent/unreadable path or a broken
    // symlink is an Err and is rejected (we never spawn on an unresolvable path).
    let canonical_path = std::fs::canonicalize(raw).map_err(|e| {
        tracing::warn!(
            transcript_path = %transcript_path,
            error = %e,
            "session ingest rejected: transcript_path failed to canonicalise"
        );
        StatusCode::BAD_REQUEST
    })?;

    // Confinement: the canonical candidate must live inside the canonical root.
    // Both sides are canonical, so a symlink whose target escapes the root has
    // already been resolved to that outside target and fails this check.
    if !canonical_path.starts_with(&canonical_root) {
        tracing::warn!(
            transcript_path = %transcript_path,
            "session ingest rejected: transcript_path resolves outside the ~/.claude/projects root"
        );
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(canonical_path)
}

/// `POST /api/sessions/ingest` — the `SessionEnd` http-hook entry point.
///
/// SECURITY: validates+confines `transcript_path` SYNCHRONOUSLY (see
/// [`confine_transcript_path`]) before doing anything else. On a rejection it
/// returns the 4xx and spawns NOTHING. On success it returns `202 Accepted`
/// (empty body) and `tokio::spawn`s the best-effort ingest, which acquires a
/// `session_ingest_sem` permit before any DB work and logs (never 500s) on
/// failure. The VALIDATED CANONICAL path — not the raw caller string — is what
/// the spawned ingest reads, so there is no TOCTOU re-resolution gap.
async fn ingest_session(
    State(state): State<AppState>,
    Json(body): Json<IngestBody>,
) -> Result<StatusCode, StatusCode> {
    let canonical_path = confine_transcript_path(&body.transcript_path)?;

    // Move owned data into the 'static spawned future: a cloned pool Arc, the
    // semaphore Arc, the validated canonical path, and the two string fields.
    let pool: Arc<lumina_core::db::AnyPool> = state.pool.clone();
    let sem = state.session_ingest_sem.clone();
    let session_id = body.session_id;
    let cwd = body.cwd;

    tokio::spawn(async move {
        // Backpressure: bound concurrent ingests at the semaphore's permit count.
        // `acquire_owned` errors only when the semaphore has been CLOSED (it
        // never is in lumina's lifetime) — on Err just drop the task silently.
        let Ok(_permit) = sem.acquire_owned().await else {
            return;
        };

        // R17: pass the canonical path as a faithful `&str`, NOT a lossy string.
        // A non-UTF-8 path mangled to U+FFFD would never re-read, wasting the
        // confinement work — so a non-UTF-8 canonical path is logged and the
        // best-effort task returns WITHOUT calling ingest (the 202 already went
        // out; the hook fires-and-forgets, so dropping this one is safe).
        let Some(path) = canonical_path.to_str() else {
            tracing::warn!(
                session_id = %session_id,
                "session ingest skipped: canonical transcript path is not valid UTF-8"
            );
            return;
        };
        if let Err(e) = repo::ingest_transcript(pool.as_ref(), &session_id, path, &cwd).await {
            // Best-effort: a down/garbage/unreadable transcript or a DB error is
            // logged and swallowed — the 202 already went out, and re-ingest is
            // idempotent, so loss is tolerated (the hook fires-and-forgets).
            tracing::warn!(session_id = %session_id, error = %e, "session ingest failed");
        }
    });

    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `transcript_path` containing a `..` component is rejected up-front with
    /// `400 BAD_REQUEST`, before any filesystem syscall.
    #[test]
    fn parent_dir_component_is_rejected() {
        let err = confine_transcript_path("/some/root/../../../etc/passwd")
            .expect_err("a `..` path must be rejected");
        assert_eq!(err, StatusCode::BAD_REQUEST);
    }

    /// A path that lives OUTSIDE the canonical transcript root is rejected
    /// (`403 FORBIDDEN` once it canonicalises, or `400` if it cannot resolve).
    /// We point the root at a tempdir via `LUMINA_PTY_PROJECTS_ROOT` and target
    /// a real file that exists but sits outside that root — exercising the
    /// `starts_with` confinement on canonical forms. nextest runs
    /// process-per-test, so mutating this process-global env var is isolated.
    #[test]
    fn out_of_root_path_is_rejected() {
        let root = tempfile::tempdir().expect("root tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, "secret").expect("write outside file");

        // SAFETY: process-per-test isolation under nextest.
        unsafe {
            std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", root.path());
        }
        let err = confine_transcript_path(outside_file.to_str().unwrap())
            .expect_err("an out-of-root path must be rejected");
        assert_eq!(err, StatusCode::FORBIDDEN);
        unsafe {
            std::env::remove_var("LUMINA_PTY_PROJECTS_ROOT");
        }
    }

    /// A real file INSIDE the canonical transcript root passes confinement and
    /// returns the canonical path (which starts with the canonical root).
    #[test]
    fn in_root_path_is_accepted() {
        let root = tempfile::tempdir().expect("root tempdir");
        let inside = root.path().join("session.jsonl");
        std::fs::write(&inside, "{}\n").expect("write inside file");

        // SAFETY: process-per-test isolation under nextest.
        unsafe {
            std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", root.path());
        }
        let canonical = confine_transcript_path(inside.to_str().unwrap())
            .expect("an in-root path must be accepted");
        let canonical_root =
            std::fs::canonicalize(root.path()).expect("canonicalise root in test");
        assert!(
            canonical.starts_with(&canonical_root),
            "the returned canonical path must live inside the canonical root"
        );
        unsafe {
            std::env::remove_var("LUMINA_PTY_PROJECTS_ROOT");
        }
    }

    /// A non-existent path inside the root cannot canonicalise → `400`.
    #[test]
    fn nonexistent_path_is_rejected() {
        let root = tempfile::tempdir().expect("root tempdir");
        let ghost = root.path().join("does-not-exist.jsonl");

        // SAFETY: process-per-test isolation under nextest.
        unsafe {
            std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", root.path());
        }
        let err = confine_transcript_path(ghost.to_str().unwrap())
            .expect_err("a non-existent path must be rejected");
        assert_eq!(err, StatusCode::BAD_REQUEST);
        unsafe {
            std::env::remove_var("LUMINA_PTY_PROJECTS_ROOT");
        }
    }

    /// THE load-bearing confinement guarantee: a symlink that lives INSIDE the
    /// root but POINTS OUTSIDE it must be rejected. `std::fs::canonicalize`
    /// resolves the link to its outside target, so the canonical path fails the
    /// `starts_with` check and we return `403 FORBIDDEN` — proving the symlink
    /// cannot be used to escape the `~/.claude/projects` confinement boundary.
    #[cfg(unix)]
    #[test]
    fn symlink_escaping_root_is_rejected() {
        let root = tempfile::tempdir().expect("root tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");

        // A real secret file OUTSIDE the confinement root.
        let outside_file = outside.path().join("secret.txt");
        std::fs::write(&outside_file, "secret").expect("write outside file");

        // A symlink INSIDE the root whose target is the outside secret. The link
        // path itself starts_with the root lexically, but canonicalize follows it
        // to the outside target — which is exactly the escape we must block.
        let link_inside = root.path().join("escape.jsonl");
        std::os::unix::fs::symlink(&outside_file, &link_inside).expect("create escaping symlink");

        // SAFETY: process-per-test isolation under nextest.
        unsafe {
            std::env::set_var("LUMINA_PTY_PROJECTS_ROOT", root.path());
        }
        let err = confine_transcript_path(link_inside.to_str().unwrap())
            .expect_err("a symlink whose target escapes the root must be rejected");
        assert_eq!(
            err,
            StatusCode::FORBIDDEN,
            "an in-root symlink pointing outside the root canonicalises out and is 403"
        );
        unsafe {
            std::env::remove_var("LUMINA_PTY_PROJECTS_ROOT");
        }
    }
}
