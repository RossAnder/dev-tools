//! Flow-aware subcommand cluster (T1+ from `docs/plans/flow-tracking-overhaul.md`).
//! Each leaf module is implemented in a dedicated task — this file exists in
//! T1 as a structural skeleton so Phase A leaf tasks (T2–T5) and Phase B
//! composite tasks (T7–T11) can each edit their own file without colliding on
//! `flow/mod.rs` or `flow/dispatch.rs`.

mod active;
mod artifacts;
mod doctor;
mod ensure_artifact;
mod find_plans;
mod init;
mod list;
mod resolve;
mod schema;
mod stale;
mod time;

mod dispatch;

pub(crate) use dispatch::dispatch;
