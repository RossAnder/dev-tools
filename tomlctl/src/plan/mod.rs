//! The `plan` store: a flow's decisions, contracts, approvals, runs and
//! handovers in `.claude/flows/<slug>/plan.toml`, written only through the
//! `tomlctl plan` verbs.

mod approve;
pub(crate) mod check;
mod drift;
mod emit;
mod milestone;
mod open;
mod probes;
mod records;
mod stage;
pub(crate) mod target;
mod view;

pub(crate) mod dispatch;
pub(crate) mod schema;
pub(crate) mod store;

pub(crate) use view::VIEW_ROLES;

// `review apply` writes a finding's edit to a decision, contract or milestone
// through the verbs' own write paths, with `expect` checked inside the lock.
pub(crate) use milestone::{
    MilestoneEdit, apply_review_edit as apply_milestone_edit,
    validate_review_edit as validate_milestone_edit,
};
pub(crate) use records::{
    RecordEdit, apply_review_edit as apply_record_edit,
    validate_review_edit as validate_record_edit,
};
