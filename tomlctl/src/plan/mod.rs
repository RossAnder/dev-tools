//! The `plan` store: a flow's decisions, contracts, approvals, runs and
//! handovers in `.claude/flows/<slug>/plan.toml`, written only through the
//! `tomlctl plan` verbs.

mod approve;
pub(crate) mod authored;
pub(crate) mod check;
mod drift;
mod emit;
mod lane;
mod milestone;
mod note_sources;
mod open;
pub(crate) mod probes;
mod records;
mod run_gate;
mod run_note;
mod run_notes;
mod run_report;
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

/// The milestone the oldest open `implement:<id>` run is executing, `""` with
/// no such run.
pub(crate) fn running_milestone(plan: &schema::PlanStore) -> String {
    store::open_runs(plan)
        .into_iter()
        .find_map(|run| run.step.strip_prefix("implement:"))
        .unwrap_or_default()
        .to_string()
}
