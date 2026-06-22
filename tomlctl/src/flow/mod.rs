//! Flow-aware subcommand cluster (T1+ from `docs/plans/flow-tracking-overhaul.md`).
//! Each leaf module is implemented in a dedicated task — this file exists in
//! T1 as a structural skeleton so Phase A leaf tasks (T2–T5) and Phase B
//! composite tasks (T7–T11) can each edit their own file without colliding on
//! `flow/mod.rs` or `flow/dispatch.rs`.

mod active;
mod artifacts;
mod doctor;
mod ensure_artifact;
mod envelope;
mod find_plans;
mod init;
// R5: re-export just the pure execution-record skeleton builder so the
// dispatch-layer byte-identity test (`cli::dispatch`'s
// `seed_doc_for_matches_bootstrap_bytes`) can name a REAL bootstrap code path
// without widening the whole `init` module to `pub(crate)`. Mirrors the
// minimal-surface `time` re-export below. Test-only: `bootstrap_execution_record`
// calls `execution_record_skeleton` directly within `init`, so the re-export's
// only out-of-module consumer is that `#[cfg(test)]` assertion.
#[cfg(test)]
pub(crate) use init::execution_record_skeleton;
mod list;
// T3: `flow render-progress-log` — deterministic markdown render of a flow's
// PROGRESS-LOG.md from its execution-record.toml. Owns its own leaf file so it
// never collides with sibling leaf tasks on this or flow/dispatch.rs.
pub(crate) mod render_progress_log;
mod resolve;
mod schema;
mod stale;
// R17: the `time` module stays PRIVATE — only sibling `flow::*` modules (its
// descendants) reference the full helper set, and a private `mod` is visible
// to them. The sole non-`flow` consumer is the dispatch layer, which needs
// exactly ONE helper (`today_toml_date`, for `cli::seed_doc_for`'s
// schema-aware seed). We re-export just that one symbol below rather than
// widening the whole module to `pub(crate)`, keeping the cross-layer
// visibility surface minimal.
mod time;
pub(crate) use time::today_toml_date;

mod dispatch;

pub(crate) use dispatch::dispatch;
