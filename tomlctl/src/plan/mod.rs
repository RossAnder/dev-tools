//! The `plan` store: a flow's decisions, contracts, approvals, runs and
//! handovers in `.claude/flows/<slug>/plan.toml`, written only through the
//! `tomlctl plan` verbs.

mod approve;
mod check;
mod drift;
mod milestone;
mod open;
mod probes;
mod records;
mod stage;
mod view;

pub(crate) mod dispatch;
pub(crate) mod schema;
pub(crate) mod store;

pub(crate) use view::VIEW_ROLES;
