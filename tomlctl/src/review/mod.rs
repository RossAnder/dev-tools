//! `tomlctl review` — the v2 review store over `.claude/flows/<slug>/review-v2.toml`
//! or `.claude/review-v2/<scope>.toml`, one module per substrate or verb.

pub(crate) mod catalogue;
pub(crate) mod dispatch;
mod fields;
mod matching;
mod store;

mod add;
mod apply;
mod catalogue_cmd;
mod cells;
mod cover;
mod digest;
mod export;
mod query;
mod resolve;
mod runs;
mod schema_out;
mod stats;
mod survey;
mod sync;
mod target;
mod tier;
mod triage;
mod verdict;

pub(crate) use tier::Tier;

/// A `validation`-kind error, for input a review verb refuses.
pub(super) fn invalid(msg: impl Into<String>) -> anyhow::Error {
    crate::errors::tagged_err(crate::errors::ErrorKind::Validation, None, msg)
}
