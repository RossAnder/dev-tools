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
mod record;
mod record_path;
mod record_schema;
pub(crate) mod render_progress_log;
mod resolve;
mod schema;
mod stale;

mod dispatch;

pub(crate) use dispatch::dispatch;
#[cfg(test)]
pub(crate) use envelope::{VALID_ARTIFACTS, VALID_COMMANDS};
pub(crate) use init::validate_slug;
pub(crate) use list::list_all;
pub(crate) use record_path::execution_record_path;
pub(crate) use record_schema::RecordType;
// The skill-parity gate in `cli::dispatch::tests` reads the enforced contract.
#[cfg(test)]
pub(crate) use record_schema::{FAILED_IDS_CAP, TEXT_CAPS, TYPES, type_enums, type_required};
pub(crate) use schema::FlowProjection;
