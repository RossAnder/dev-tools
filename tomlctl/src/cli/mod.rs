//! R63 / R21: top-level CLI module. Formerly a single `cli.rs` (~2100 lines)
//! mixing four concerns — clap derive types, `run()` dispatch + per-command
//! routing, output helpers, and unit tests. R21 split the file along those
//! seams:
//!
//! - [`types`] (`cli/types.rs`) — the clap-derive `Cli`, `Cmd`, `ItemsOp`,
//!   `BlocksOp` enums plus the per-variant argument bundles
//!   (`ReadIntegrityArgs`, `WriteIntegrityArgs`, `QueryArgs`) and the
//!   legacy-shortcut adapter (`LegacyShortcuts`). Exports the
//!   `FEATURES` / `SUBCOMMANDS` metadata consts used by `Cmd::Capabilities`.
//! - [`dispatch`] (`cli/dispatch.rs`) — `fn run()`, `items_dispatch`,
//!   `blocks_dispatch`, the stdin / NDJSON helpers, and the integrity-opts
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

pub(crate) use dispatch::run;
pub(crate) use types::{Cli, ErrorFormat};
