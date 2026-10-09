//! Clap derive types — the `Cli` root, `Cmd` subcommand enum, the
//! per-variant argument bundles (`ReadIntegrityArgs`, `WriteIntegrityArgs`,
//! `QueryArgs`), and the legacy shortcut adapter (`LegacyShortcuts`). The
//! clap surface lives here and the dispatch logic in the sibling
//! `dispatch` module; every type is `pub(crate)` so that module can match
//! on it.
//!
//! The root types and metadata consts live in this file; each verb group's
//! enums live in a child module, and every child item is re-exported here so
//! `crate::cli::types::X` names the type wherever it is defined.

use clap::{Args, Parser, ValueEnum};

mod backlog;
mod cmd;
mod flow;
mod inputs;
mod items;
mod shared;
mod tasks;

pub(crate) use backlog::*;
pub(crate) use cmd::*;
pub(crate) use flow::*;
pub(crate) use inputs::*;
pub(crate) use items::*;
pub(crate) use shared::*;
pub(crate) use tasks::*;

/// Capabilities advertised by `tomlctl capabilities`. Each entry is
/// stable across patch versions within a minor release — removing an entry
/// is a breaking change. Add new entries for new user-facing flags;
/// don't version-qualify (the `version` field is the release marker). The
/// downstream flow-command templates call `tomlctl capabilities` at boot
/// and feature-gate on this list without having to parse `--help` prose.
pub(crate) const FEATURES: &[&str] = &[
    "count_distinct",
    "raw",
    "lines",
    "infer_prefix",
    "dedupe_by",
    "dedup_id_auto",
    "find_duplicates_across",
    "fingerprint",
    "capabilities",
    "error_format_json",
    "strict_read",
    "dry_run",
    "backfill_dedup_id",
    "integrity_refresh", // sidecar bootstrap / recovery primitive
    "agent_context",     // capabilities .commands flag schema
    // Flow / json subcommand cluster.
    "flow_resolve",
    "flow_active",
    "flow_doctor",
    "flow_init",
    "flow_ensure_artifact",
    "flow_envelope_build",
    "flow_stale",
    "flow_find_plans",
    "flow_list",
    "flow_render_progress_log",
    "json_ops",
    // Repo-scoped capture log: the `backlog` subcommand cluster.
    "backlog_capture", // the `add` verb
    "backlog_add_many",
    "backlog_check",
    "backlog_cluster",
    "backlog_compact",
    "backlog_evidence",
    "backlog_list",
    "backlog_show",
    "backlog_relate",
    "backlog_triage",
    "backlog_triage_expect", // `triage --expect-status`
    "backlog_reconcile",
    // Per-flow task DAG store: the `tasks` subcommand cluster.
    "tasks_import_plan",
    "tasks_add",
    "tasks_add_many",
    "tasks_update",
    "tasks_remove",
    "tasks_show",
    "tasks_list",
    "tasks_edges",
    "tasks_ready",
    "tasks_batches",
    "tasks_closure",
    "tasks_check",
    "tasks_render",
    "tasks_snapshot",
    // Hook-written agent lifecycle records: the `agents` subcommand cluster.
    "agents_record",
    "agents_list",
    // Repo-scoped input store: the `inputs` subcommand cluster.
    "inputs",
    // Per-op `expect` precondition and `--on-stale` on `items apply`.
    "items_apply_expect",
    // Regex sweep over tracked files and the ledger verbs built on it.
    "sweep",
    "items_sweep",
    "items_clusters",
    "orphans_instances",
    "output_options", // the root's global output flags
    // Write ergonomics.
    "next_id_bare",      // `items next-id` prints the bare id
    "id_prefix",         // `items add`/`add-many --id-prefix` mint inside the lock
    "auto_last_updated", // CLI writes stamp an existing `last_updated`
    "multi_id",          // `tasks update`/`tasks show`/`backlog show` take id lists
];

/// User-facing top-level subcommand names, as they appear in
/// `tomlctl --help`. Enumerated statically rather than clap-reflected
/// because clap's command introspection is brittle (name-mangled enum
/// variants, re-derives on every build). Keep this list in sync with the
/// `Cmd` enum by hand — adding a new subcommand means one edit here and
/// one integration assertion in `tests/integration.rs`.
pub(crate) const SUBCOMMANDS: &[&str] = &[
    "parse",
    "get",
    "set",
    "set-json",
    "validate",
    "items",
    "blocks",
    "array-append",
    "capabilities",
    "integrity",
    "flow",
    "json",
    "backlog",
    "tasks",
    "agents",
    "inputs",
    "sweep",
];

#[derive(Parser)]
#[command(
    name = "tomlctl",
    version,
    about = "Read and write TOML files used by Claude Code flows and ledgers"
)]
pub(crate) struct Cli {
    /// Stderr error rendering format. `text` (default) emits the plain
    /// `tomlctl: <anyhow chain>` line. `json` emits a single
    /// compact JSON envelope (`{"error":{"kind":...,"message":...,"file":...,"arg":...}}`)
    /// so downstream agents can branch on `kind` without regexing prose. Exit
    /// code stays 1 regardless; this flag only affects stderr shape. `global`
    /// so the flag can appear either before or after the subcommand name.
    #[arg(
        long = "error-format",
        value_enum,
        default_value_t = ErrorFormat::Text,
        global = true,
        help = "Stderr error format on failure (text|json)"
    )]
    pub(crate) error_format: ErrorFormat,

    #[command(flatten)]
    pub(crate) output: OutputArgs,

    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

/// The output-shaping flags every command accepts, before or after the
/// subcommand. Ids and types must equal any same-named per-command flag:
/// clap merges a same-id local into the global but panics on a different id
/// sharing the long name. Conflicts are checked by `output::configure`.
#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Output options")]
pub(crate) struct OutputArgs {
    #[arg(
        long = "select",
        value_name = "P1,P2,...",
        global = true,
        help = "Keep only these dotted paths — of each row on a row report, of the object otherwise"
    )]
    pub(crate) select: Option<String>,

    #[arg(
        long = "limit",
        value_name = "N",
        global = true,
        help = "Keep at most N rows of a row report"
    )]
    pub(crate) limit: Option<usize>,

    #[arg(
        long = "lines",
        global = true,
        help = "Compact NDJSON: a header line, then one row per line; one compact line for a single object"
    )]
    pub(crate) lines: bool,

    #[arg(
        long = "get",
        value_name = "PATH",
        global = true,
        help = "Print the bare value at PATH — one line per row on a row report",
        long_help = "Print the bare value at PATH — one line per row on a row report; a row lacking PATH prints an empty line. A string containing newlines prints them verbatim, so one row can span several lines; use --lines or --select for line-safe output"
    )]
    pub(crate) get: Option<String>,

    #[arg(
        long = "template",
        value_name = "T",
        global = true,
        help = "One text line per row (or one for a single report); `{path}` placeholders, `{{`/`}}` for literal braces",
        long_help = "One text line per row (or one for a single report); `{path}` placeholders, `{{`/`}}` for literal braces. A string value containing newlines prints them verbatim, so one row can span several lines; use --lines or --select for line-safe output"
    )]
    pub(crate) template: Option<String>,

    #[arg(
        short = 'q',
        long = "quiet",
        global = true,
        help = "Print nothing on success; the exit code carries the result"
    )]
    pub(crate) quiet: bool,

    #[arg(
        long = "rows",
        value_name = "PATH",
        global = true,
        help = "Treat the array at PATH of a single-object report as its rows; the rest is the header"
    )]
    pub(crate) rows: Option<String>,

    #[arg(
        long = "header",
        global = true,
        help = "Shape a row report's header (the report minus its rows) as one object"
    )]
    pub(crate) header: bool,

    #[arg(
        long = "max-chars",
        value_name = "N",
        global = true,
        help = "Cut every string value longer than N characters, marking how many were cut"
    )]
    pub(crate) max_chars: Option<usize>,

    #[arg(
        long = "omit",
        value_name = "P1,P2,...",
        global = true,
        help = "Drop these dotted paths — from each row and the header on a row report, from the object otherwise"
    )]
    pub(crate) omit: Option<String>,

    #[command(flatten)]
    pub(crate) filters: GlobalWhereArgs,
}

/// The `--where*` row filters as globals. Each field's id and type equal the
/// matching `QueryArgs` field, so on the list verbs clap merges the two into
/// one argument and the engine reads it through `QueryArgs`. A repeatable
/// global keeps only the values given after the subcommand when both sides
/// carry some, which `cli::run` refuses by counting argv tokens.
#[derive(Args, Clone, Default)]
#[command(next_help_heading = "Output options")]
pub(crate) struct GlobalWhereArgs {
    #[arg(
        long = "where",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field equals value (repeatable)"
    )]
    pub(crate) where_eq: Vec<String>,
    #[arg(
        long = "where-not",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field does not equal value (repeatable)"
    )]
    pub(crate) where_not: Vec<String>,
    #[arg(
        long = "where-in",
        value_name = "KEY=V1,V2,...",
        global = true,
        help = "Filter rows: field in comma-separated set (repeatable)"
    )]
    pub(crate) where_in: Vec<String>,
    #[arg(
        long = "where-has",
        value_name = "KEY",
        global = true,
        help = "Filter rows: field is present and non-empty (repeatable)"
    )]
    pub(crate) where_has: Vec<String>,
    #[arg(
        long = "where-missing",
        value_name = "KEY",
        global = true,
        help = "Filter rows: field is absent or empty (repeatable)"
    )]
    pub(crate) where_missing: Vec<String>,
    #[arg(
        long = "where-gt",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field > value (repeatable)"
    )]
    pub(crate) where_gt: Vec<String>,
    #[arg(
        long = "where-gte",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field >= value (repeatable)"
    )]
    pub(crate) where_gte: Vec<String>,
    #[arg(
        long = "where-lt",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field < value (repeatable)"
    )]
    pub(crate) where_lt: Vec<String>,
    #[arg(
        long = "where-lte",
        value_name = "KEY=VAL",
        global = true,
        help = "Filter rows: field <= value (repeatable)"
    )]
    pub(crate) where_lte: Vec<String>,
    #[arg(
        long = "where-contains",
        value_name = "KEY=SUB",
        global = true,
        help = "Filter rows: field string contains SUB (repeatable)"
    )]
    pub(crate) where_contains: Vec<String>,
    #[arg(
        long = "where-prefix",
        value_name = "KEY=S",
        global = true,
        help = "Filter rows: field string starts with S (repeatable)"
    )]
    pub(crate) where_prefix: Vec<String>,
    #[arg(
        long = "where-suffix",
        value_name = "KEY=S",
        global = true,
        help = "Filter rows: field string ends with S (repeatable)"
    )]
    pub(crate) where_suffix: Vec<String>,
    #[arg(
        long = "where-regex",
        value_name = "KEY=PAT",
        global = true,
        help = "Filter rows: field string matches regex PAT (repeatable)"
    )]
    pub(crate) where_regex: Vec<String>,
}

impl GlobalWhereArgs {
    pub(crate) fn to_filters(&self) -> crate::output::WhereFilters {
        crate::output::WhereFilters {
            where_eq: self.where_eq.clone(),
            where_not: self.where_not.clone(),
            where_in: self.where_in.clone(),
            where_has: self.where_has.clone(),
            where_missing: self.where_missing.clone(),
            where_gt: self.where_gt.clone(),
            where_gte: self.where_gte.clone(),
            where_lt: self.where_lt.clone(),
            where_lte: self.where_lte.clone(),
            where_contains: self.where_contains.clone(),
            where_prefix: self.where_prefix.clone(),
            where_suffix: self.where_suffix.clone(),
            where_regex: self.where_regex.clone(),
        }
    }
}

/// Stderr-format selector surfaced via `--error-format`. `pub(crate)` so
/// the library root's `run` can match on the variant when reporting an error.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ErrorFormat {
    /// Default — the plain `tomlctl: <anyhow chain>` line.
    Text,
    /// Single compact JSON line with `error.kind` taxonomy.
    Json,
}
