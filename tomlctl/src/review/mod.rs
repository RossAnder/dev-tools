//! `tomlctl review` — the v2 review store over `.claude/flows/<slug>/review-v2.toml`
//! or `.claude/review-v2/<scope>.toml`, one module per substrate or verb.

#[allow(
    dead_code,
    reason = "the review verbs are its callers; until they are wired only its tests reach it"
)]
pub(crate) mod catalogue;
pub(crate) mod dispatch;
#[allow(
    dead_code,
    reason = "the review verbs are its callers; until they are wired only its tests reach it"
)]
mod fields;
#[allow(
    dead_code,
    reason = "the review verbs are its callers; until they are wired only its tests reach it"
)]
mod matching;
#[allow(
    dead_code,
    reason = "the review verbs are its callers; until they are wired only its tests reach it"
)]
mod store;

mod add;
mod catalogue_cmd;
mod cells;
mod digest;
mod export;
mod query;
mod resolve;
mod runs;
mod schema_out;
mod stats;
mod survey;
mod sync;
mod triage;
mod verdict;
