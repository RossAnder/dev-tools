//! Flow-aware subcommand cluster (T1+ from `docs/plans/flow-tracking-overhaul.md`).
//! Each leaf module is implemented in a dedicated task — this file exists in
//! T1 as a structural skeleton so Phase A leaf tasks (T2–T5) and Phase B
//! composite tasks (T7–T11) can each edit their own file without colliding on
//! `flow/mod.rs` or `flow/dispatch.rs`.

pub(crate) mod active;
pub(crate) mod doctor;
pub(crate) mod ensure_artifact;
pub(crate) mod find_plans;
pub(crate) mod init;
pub(crate) mod list;
pub(crate) mod resolve;
pub(crate) mod stale;

mod dispatch;

pub(crate) use dispatch::dispatch;
