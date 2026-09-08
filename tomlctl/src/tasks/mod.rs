//! `tomlctl tasks` — the per-flow task DAG over `.claude/flows/<slug>/tasks.toml`.
//!
//! Leaf layout is one module per verb plus five shared substrates (`schema`,
//! `slug`, `graph`, `store`, `markdown`), with `dispatch` fanning `TasksOp` out
//! to them, so parallel tasks each own one file.
//!
//! The store's array is named `items` so the existing `[[items]]` machinery and
//! `QueryArgs` apply unchanged. Only `needs` and `coupling` edges are stored:
//! file overlap, checkpoint closures and marker `after` lists are computed on
//! every read, never persisted.

mod add;
mod batches;
mod check;
mod closure;
mod edges;
mod graph;
mod import_plan;
mod list;
mod markdown;
mod parse_policy;
mod parse_tasks;
mod ready;
mod remove;
mod render;
mod schema;
mod show;
mod slug;
mod store;
mod update;

// `pub(crate)` because the caller is `cli::dispatch::run`, which is not a
// descendant of this module and so cannot see the private leaves above.
pub(crate) mod dispatch;
