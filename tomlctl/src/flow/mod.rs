//! Flow-aware subcommand cluster. Each subcommand owns a leaf module; this
//! file and `flow/dispatch.rs` hold only wiring, so edits to two subcommands
//! never land in the same file.

mod active;
mod artifacts;
mod doctor;
mod ensure_artifact;
mod envelope;
mod find_plans;
mod init;
// Re-exported so the dispatch-layer byte-identity test
// (`seed_doc_for_matches_bootstrap_bytes`) can name a real bootstrap code path
// without widening the whole `init` module to `pub(crate)`. Test-only: `init`
// calls `execution_record_skeleton` directly, so that assertion is the only
// out-of-module consumer.
#[cfg(test)]
pub(crate) use init::execution_record_skeleton;
mod list;
pub(crate) mod render_progress_log;
mod resolve;
mod schema;
mod stale;

mod dispatch;

pub(crate) use dispatch::dispatch;
