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
mod list;
pub(crate) mod render_progress_log;
mod resolve;
mod schema;
mod stale;

mod dispatch;

pub(crate) use dispatch::dispatch;
