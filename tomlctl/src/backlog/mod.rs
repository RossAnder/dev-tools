//! `tomlctl backlog` — the repo-scoped capture log over `.claude/backlog.toml`.
//!
//! Leaf layout is one module per verb plus three shared substrates
//! (`schema`, `normalise`, `ids`), with `dispatch` fanning `BacklogOp` out to
//! them, so parallel tasks each own one file.
//!
//! The store's array is named `backlog`, never `items`: an array named
//! `items` in a file under `.claude/` is the default target of
//! `tomlctl items add|update|apply`, whose dedup stamping would overwrite the
//! content-derived `dedup_id` that every backlog id is built from.

mod add;
mod check;
mod cluster;
mod compact;
mod evidence;
mod evidence_ops;
mod ids;
mod normalise;
mod query;
mod relate;
mod schema;
mod triage;

// `pub(crate)` because the caller is `cli::dispatch::run`, which is not a
// descendant of this module and so cannot see the private leaves above.
pub(crate) mod dispatch;
