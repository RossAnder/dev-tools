//! `tomlctl review` — the v2 review store over `.claude/flows/<slug>/review-v2.toml`
//! or `.claude/review-v2/<scope>.toml`, one module per substrate or verb.

pub(crate) mod catalogue;
pub(crate) mod dispatch;
mod fields;
mod matching;
mod store;

mod add;
mod anchor;
mod apply;
mod catalogue_cmd;
mod cells;
mod cover;
mod digest;
mod export;
mod query;
mod recheck;
mod reconcile;
mod reproduce;
mod resolve;
mod run_show;
mod runs;
mod schedule;
mod schema_out;
mod select;
mod settle;
mod stats;
mod survey;
mod sync;
mod target;
mod tier;
mod triage;
mod verdict;

// The vocabularies review verbs check their flags against after parsing, for
// `capabilities` to publish.
pub(crate) use cover::SEAL_STATUSES;
pub(crate) use fields::{
    COMMANDS, Depth, FINDING_STATUSES, Family, Kind, ReproduceVerdict, Role, Route, SETTLE_TIERS,
    Severity, VERDICT_BY, VERDICTS,
};
pub(crate) use query::BLOCKED_ON_FILTERS;
pub(crate) use schema_out::TARGETS as SCHEMA_TARGETS;
pub(crate) use stats::AXES as STATS_AXES;
pub(crate) use triage::CONVERT_KINDS;
// The entry points for readers outside the review group, so that no field
// name of the store is spelled outside it.
pub(crate) use store::{implementer_findings, plan_family, reproduce_state};
pub(crate) use tier::Tier;

/// A `validation`-kind error, for input a review verb refuses.
pub(super) fn invalid(msg: impl Into<String>) -> anyhow::Error {
    crate::errors::tagged_err(crate::errors::ErrorKind::Validation, None, msg)
}
