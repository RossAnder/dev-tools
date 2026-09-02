//! Top-level CLI module, split along concern seams — clap derive types,
//! `run()` dispatch + per-command routing, and output helpers:
//!
//! - [`types`] (`cli/types.rs`) — the clap-derive `Cli`, `Cmd`, `ItemsOp`,
//!   `BlocksOp` enums plus the per-variant argument bundles
//!   (`ReadIntegrityArgs`, `WriteIntegrityArgs`, `QueryArgs`) and the
//!   legacy-shortcut adapter (`LegacyShortcuts`). Exports the
//!   `FEATURES` / `SUBCOMMANDS` metadata consts used by `Cmd::Capabilities`.
//! - [`dispatch`] (`cli/dispatch.rs`) — `fn run()`, `items_dispatch`,
//!   `blocks_dispatch`, the NDJSON source resolver, and the integrity-opts
//!   translators. Pure plumbing; delegates to `items::` / `blocks::` /
//!   `io::` for real work.
//! - Output helpers (`print_json`, `print_json_compact`, `print_raw_value`,
//!   `emit_list_raw`, `emit_dry_run_plan`) live in the top-level
//!   [`crate::output`] module — sibling of `cli`, not child — because they
//!   don't touch clap types and shouldn't carry a CLI-scoped path.
//!
//! External callers (`main.rs`) see the same import surface they saw
//! pre-split: `use crate::cli::{Cli, ErrorFormat}` and `cli::run(cli)`. The
//! `pub(crate) use` re-exports below keep that stable.

mod dispatch;
mod types;

/// Only clap-derived TYPES cross this boundary outward, plus `run` (the
/// entrypoint) and the two integrity-args translators that exist solely to
/// read those types. Every other function in [`dispatch`] is private to the
/// CLI layer: a verb group that needs one needs the helper moved down into
/// infrastructure (`crate::io`) or onto the clap type it converts — never a
/// new re-export here.
pub(crate) use dispatch::{read_integrity_opts, run, write_integrity_opts};
pub(crate) use types::{
    ActiveOp, ArtifactKind, BacklogOp, Cli, ClusterBy, EnvelopeOp, ErrorFormat, EvidenceOp, FlowOp,
    JsonOp, LegacyShortcuts, OnDuplicate, QueryArgs, ReadIntegrityArgs, RelationKind, TriageMode,
    WriteIntegrityArgs,
};
