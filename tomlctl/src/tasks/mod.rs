//! `tomlctl tasks` — the per-flow task DAG over `.claude/flows/<slug>/tasks.toml`.
//!
//! Leaf layout is one module per verb plus six shared substrates (`schema`,
//! `slug`, `graph`, `store`, `markdown`, `finding`), with `dispatch` fanning
//! `TasksOp` out to them, so parallel tasks each own one file.
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
mod finding;
mod graph;
mod import_plan;
mod list;
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

// `flow::resolve` discriminates a plan that declares tasks off the same
// fence-aware scan the store's own parsers run on; a second scanner over the
// same bytes drifts from this one on fence and heading edge cases.
pub(crate) mod markdown;

/// The plan document `context.toml` binds `slug` to, through the plan-path
/// seam's containment and `.md` validation — a recorded value is
/// file-controlled input, and resolving one verbatim makes a reader of it an
/// oracle for any path on the machine. No store is bound here: whether store
/// and context agree is `tasks check --plan`'s finding.
pub(crate) fn context_plan_path(slug: &str) -> anyhow::Result<std::path::PathBuf> {
    import_plan::resolve_recorded_plan_path(slug, &schema::Store::default())
}
