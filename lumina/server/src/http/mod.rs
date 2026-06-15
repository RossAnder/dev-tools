//! HTTP `/api` sub-router — split into per-family modules for round-4.
//!
//! See `docs/plans/lumina-story-planning-round-4.md` (T1). The split lets
//! per-family route work (T2–T5) run in parallel without single-file
//! contention. Each per-family module exposes its own `pub fn router() ->
//! Router<AppState>`; this `mod.rs` composes them via `.merge(...)`. The
//! Phase-2 family files are pre-declared as empty-router stubs here so that
//! subsequent tasks only need to fill in their owned file body — `mod.rs` is
//! touched ONCE (this task) and not by any Phase-2 follow-up.
//!
//! The public symbol consumed by `app.rs` is exactly `crate::http::router()`,
//! unchanged from the pre-split layout.

use axum::Router;

use crate::app::AppState;

pub mod companion;
pub mod work_items;
pub mod repo_links;
pub mod structured_patches;
pub mod acceptance_criteria;
pub mod research_notes;
pub mod risks;
pub mod rejected_alternatives;
pub mod task_dependencies;
pub mod open_questions;
pub mod findings;
pub mod queries;
pub mod runs;
pub mod sprints;
pub mod activity;
pub mod context_blocks;
pub mod readiness;
pub mod pty_sessions;
pub mod export;
pub mod execution;
pub mod settings;
pub mod sessions;
pub mod stream;
pub mod worktrees;
pub mod files;
/// Shared WebSocket helpers (Origin allowlist) — a helper module, NOT a
/// route family; it exposes no `router()` and is deliberately absent from
/// the `.merge(...)` list below.
pub mod ws_common;

/// Build the composed `/api` sub-router by merging every per-family router.
/// The composition root in `app.rs` calls `.nest("/api", crate::http::router())`,
/// so paths declared inside each family module are relative to `/api`.
pub fn router() -> Router<AppState> {
    Router::new()
        .merge(companion::router())
        .merge(work_items::router())
        .merge(repo_links::router())
        .merge(structured_patches::router())
        .merge(acceptance_criteria::router())
        .merge(research_notes::router())
        .merge(risks::router())
        .merge(rejected_alternatives::router())
        .merge(task_dependencies::router())
        .merge(open_questions::router())
        .merge(findings::router())
        .merge(queries::router())
        .merge(runs::router())
        .merge(sprints::router())
        .merge(activity::router())
        .merge(context_blocks::router())
        .merge(readiness::router())
        .merge(pty_sessions::router())
        .merge(export::router())
        .merge(execution::router())
        .merge(settings::router())
        .merge(sessions::router())
        .merge(stream::router())
        .merge(worktrees::router())
        .merge(files::router())
}
