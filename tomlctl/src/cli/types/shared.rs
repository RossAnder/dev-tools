//! Argument bundles flattened into many verbs: the integrity, stamping and
//! query option groups.

use clap::Args;

use super::LegacyShortcuts;

/// Read-only integrity options. Read paths honour only
/// `--verify-integrity` — the other three flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`) are write-side concepts
/// that would be silently no-ops on a read, so they're structurally kept
/// off read subcommands.
///
/// `--strict-read` turns the "missing file → silent default" branches
/// (`items next-id --prefix <P>`, the `backlog` read verbs, `agents list`)
/// into a tagged `kind=not_found` error. On every other read subcommand the flag is a benign no-op —
/// `io::read_toml` already surfaces `kind=not_found` on a missing file via
/// its error tagging, so passing `--strict-read` there changes nothing. The
/// flag lives here (rather than only on `NextId`) so `ReadIntegrityArgs`
/// retains its "every read subcommand carries the same read-side switches"
/// contract; adding the bit to a single variant would fork that surface.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
pub(crate) struct ReadIntegrityArgs {
    /// Before any read operation, verify the target file against its
    /// `<file>.sha256` sidecar. Errors if the sidecar is missing or the
    /// digest disagrees. Never auto-repairs.
    #[arg(long = "verify-integrity")]
    pub(crate) verify_integrity: bool,

    /// Error on a missing target file (`kind=not_found`) instead of returning
    /// an empty default. Only the defaulting reads change: `items next-id
    /// --prefix <P>` (which returns `<P>1`), the `backlog` read verbs (an
    /// empty store) and `agents list` (`[]`); `items list` / `items orphans`
    /// already error on a missing file. Pass `--strict-read` when the caller
    /// needs to distinguish "no matches in an existing ledger" from "ledger
    /// does not exist".
    ///
    /// Fires BEFORE `--verify-integrity` — a missing file yields
    /// `kind=not_found`, not `kind=integrity`, even when both flags are set.
    #[arg(
        long = "strict-read",
        help = "Error on missing file instead of returning empty default (kind=not_found)"
    )]
    pub(crate) strict_read: bool,
}

/// Write-side integrity/containment flags. Writers
/// still honour `--verify-integrity` because an update is often preceded
/// by a pre-read verify; the other three flags only have a semantic hook
/// on write paths.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
pub(crate) struct WriteIntegrityArgs {
    /// Allow write operations on files outside the current repo's `.claude/` directory.
    /// By default, writes are refused if the canonical target path is not under
    /// `<git-top-level>/.claude/` (or the CWD, if not in a git repo). Use this to
    /// intentionally edit a flow file in another location.
    #[arg(long = "allow-outside")]
    pub(crate) allow_outside: bool,

    /// Suppress writing the `<file>.sha256` integrity sidecar. Default behaviour
    /// is to write a sidecar alongside every TOML write (standard `sha256sum`
    /// format: `<hex>  <basename>\n`). Pass this flag to opt out, e.g. when the
    /// target filesystem does not tolerate an extra sidecar file.
    #[arg(long = "no-write-integrity")]
    pub(crate) no_write_integrity: bool,

    /// Before any read operation, verify the target file against its
    /// `<file>.sha256` sidecar. Errors if the sidecar is missing or the
    /// digest disagrees. Never auto-repairs.
    #[arg(long = "verify-integrity")]
    pub(crate) verify_integrity: bool,

    /// Treat an integrity-sidecar write failure as a hard error instead of a
    /// stderr warning. Off by default — the primary data is already durable
    /// on disk by the time the sidecar is attempted, so a failed sidecar is
    /// usually recoverable by re-running the write. Pass this flag on a
    /// tight-integrity path (e.g. signed-artifact builds) where a missing or
    /// stale sidecar must fail CI.
    #[arg(long = "strict-integrity")]
    pub(crate) strict_integrity: bool,

    /// Refuse to auto-create a missing target file; restore the strict
    /// `kind=not_found` error. Default: a missing file is created (seeded
    /// with a schema-aware skeleton for recognised flow files — the ledgers
    /// `execution-record.toml` / `review-ledger.toml` / `optimise-findings.toml`
    /// / `plan-review-findings.toml`, and the stores `backlog.toml` /
    /// `tasks.toml` / `agents.toml` — and an empty table otherwise).
    /// `items sweep --update` is the exception: it never creates a ledger,
    /// and a missing file is reported as `kind=not_found` either way.
    #[arg(long = "no-create")]
    pub(crate) no_create: bool,
}

/// `last_updated` stamping for the generic write commands (`set`, `set-json`,
/// `array-append`, and the `items` writers). The `tasks`, `backlog`,
/// `inputs` and `agents` writers stamp their own stores and do not take it.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
pub(crate) struct StampArgs {
    /// Leave the root `last_updated` as it is. By default a write that
    /// changes the document refreshes an existing root `last_updated` to
    /// today (UTC); a file without the key never gains one, and a write that
    /// sets `last_updated` itself keeps the value it sets.
    #[arg(long = "no-stamp")]
    pub(crate) no_stamp: bool,
}

/// Flattened bundle of all `items list` query options — predicates,
/// projection, shaping, aggregation. Lives here rather than as inline
/// fields on the `List` variant so that `next_help_heading = "Query options"`
/// groups every flag under one heading in `--help` output (clap only
/// honours the attribute on a dedicated `Args` struct). Legacy shortcut
/// flags (`--status` / `--category` / `--file` / `--newer-than`) stay on
/// the variant so they retain their pre-query-engine help text; they
/// translate into `Predicate` entries in `build_query`.
#[derive(Args, Clone)]
#[command(next_help_heading = "Query options")]
pub(crate) struct QueryArgs {
    #[arg(
        long = "where",
        value_name = "KEY=VAL",
        help = "Filter: field equals value (repeatable)"
    )]
    pub(crate) where_eq: Vec<String>,
    #[arg(
        long = "where-not",
        value_name = "KEY=VAL",
        help = "Filter: field does not equal value (repeatable)"
    )]
    pub(crate) where_not: Vec<String>,
    #[arg(
        long = "where-in",
        value_name = "KEY=V1,V2,...",
        help = "Filter: field in comma-separated set (repeatable)"
    )]
    pub(crate) where_in: Vec<String>,
    #[arg(
        long = "where-has",
        value_name = "KEY",
        help = "Filter: field is present and non-empty (repeatable)"
    )]
    pub(crate) where_has: Vec<String>,
    #[arg(
        long = "where-missing",
        value_name = "KEY",
        help = "Filter: field is absent or empty (repeatable)"
    )]
    pub(crate) where_missing: Vec<String>,
    #[arg(
        long = "where-gt",
        value_name = "KEY=VAL",
        help = "Filter: field > value (repeatable)"
    )]
    pub(crate) where_gt: Vec<String>,
    #[arg(
        long = "where-gte",
        value_name = "KEY=VAL",
        help = "Filter: field >= value (repeatable)"
    )]
    pub(crate) where_gte: Vec<String>,
    #[arg(
        long = "where-lt",
        value_name = "KEY=VAL",
        help = "Filter: field < value (repeatable)"
    )]
    pub(crate) where_lt: Vec<String>,
    #[arg(
        long = "where-lte",
        value_name = "KEY=VAL",
        help = "Filter: field <= value (repeatable)"
    )]
    pub(crate) where_lte: Vec<String>,
    #[arg(
        long = "where-contains",
        value_name = "KEY=SUB",
        help = "Filter: field string contains SUB (repeatable)"
    )]
    pub(crate) where_contains: Vec<String>,
    #[arg(
        long = "where-prefix",
        value_name = "KEY=S",
        help = "Filter: field string starts with S (repeatable)"
    )]
    pub(crate) where_prefix: Vec<String>,
    #[arg(
        long = "where-suffix",
        value_name = "KEY=S",
        help = "Filter: field string ends with S (repeatable)"
    )]
    pub(crate) where_suffix: Vec<String>,
    #[arg(
        long = "where-regex",
        value_name = "KEY=PAT",
        help = "Filter: field string matches regex PAT (repeatable)"
    )]
    pub(crate) where_regex: Vec<String>,
    #[arg(
        long = "exclude",
        value_name = "F1,F2,...",
        help = "Projection: drop the listed fields"
    )]
    pub(crate) exclude: Option<String>,
    #[arg(
        long = "pluck",
        value_name = "FIELD",
        help = "Projection: return a flat [value, ...] array of FIELD"
    )]
    pub(crate) pluck: Option<String>,
    #[arg(
        long = "sort-by",
        value_name = "FIELD[:asc|desc]",
        help = "Sort by FIELD (repeatable for tiebreakers)"
    )]
    pub(crate) sort_by: Vec<String>,
    #[arg(long = "offset", value_name = "N", help = "Skip the first N items")]
    pub(crate) offset: Option<usize>,
    #[arg(long = "distinct", help = "Dedup on the projected shape")]
    pub(crate) distinct: bool,
    #[arg(
        long = "group-by",
        value_name = "FIELD",
        help = "Aggregate: emit {value: [item, ...], ...}"
    )]
    pub(crate) group_by: Option<String>,
    #[arg(
        long = "count-by",
        value_name = "FIELD",
        help = "Aggregate: emit {value: N, ...}"
    )]
    pub(crate) count_by: Option<String>,
    /// Scalar-cardinality aggregate. Emits
    /// `{"count_distinct": N, "field": "<name>"}` where N is the number of
    /// distinct non-null/non-missing values of FIELD in the filtered set.
    /// Mutually exclusive with the other shape flags via the `shape`
    /// ArgGroup below (`--count`, `--count-by`, `--group-by`, `--pluck`),
    /// and mutex with `--select`/`--exclude` at the `validate_query` layer
    /// (projection on an aggregation-only shape would be ambiguous).
    #[arg(
        long = "count-distinct",
        value_name = "FIELD",
        help = "Aggregate: count distinct values of FIELD (excludes null/missing), emit {\"count_distinct\":N,\"field\":\"<name>\"}"
    )]
    pub(crate) count_distinct: Option<String>,
    #[arg(
        long = "ndjson",
        help = "Output one JSON value per line (for piping into add-many/apply)"
    )]
    pub(crate) ndjson: bool,
    /// Bare-scalar output for single-value shapes. Composes as follows:
    ///
    /// - `--count --raw` / `--count-distinct --raw`: emit the bare integer
    ///   count (no `{"count":...}` / `{"count_distinct":...,"field":...}`
    ///   wrapping).
    /// - `--pluck f --raw` (N=1): emit the bare plucked value (strings
    ///   unquoted, numbers/bools bare).
    /// - `--pluck f --raw` (N != 1): errors.
    /// - `--pluck f --raw --lines`: one bare value per line (composes
    ///   with the streaming Pluck path).
    /// - `--count-by --raw` / `--group-by --raw`: rejected — the output is
    ///   a map, not a scalar; `--raw` has no well-defined conversion.
    #[arg(
        long = "raw",
        help = "Emit bare scalar (no JSON quoting) for --count/--count-distinct/single --pluck. With --lines + --pluck: bare value per line. Rejected on --count-by/--group-by."
    )]
    pub(crate) raw: bool,
}

impl QueryArgs {
    /// Trivial field-copy adapter from the two clap-derive types
    /// (`QueryArgs` + `LegacyShortcuts`) into the POD `QueryInput` that
    /// `query.rs` owns. A method on the clap type rather than a free
    /// function elsewhere, so the conversion is reachable from every verb
    /// group without any of them importing `cli::dispatch`, and `query.rs`
    /// stays free of any `use crate::cli` import — the dependency runs
    /// cli → query only.
    ///
    /// Pure plumbing: every field either `.clone()`s the owned value off
    /// `self` (the clap-derive layer already holds the `String` /
    /// `Vec<String>` / `Option<String>`) or clones out of the
    /// `&Option<String>` references on `LegacyShortcuts`. Logic that would
    /// creep in here belongs in `Query::from_query_input` instead — the POD
    /// type's whole job is to keep this boundary a straight-line data
    /// transfer. `select`, `limit` and `lines` come from the global output
    /// options in `out`: the engine applies them before `--distinct` and
    /// after `--sort-by`/`--offset`, which a post-hoc emitter could not.
    pub(crate) fn to_query_input(
        &self,
        legacy: &LegacyShortcuts<'_>,
        out: &crate::output::OutputOpts,
    ) -> crate::query::QueryInput {
        crate::query::QueryInput {
            status: legacy.status.clone(),
            category: legacy.category.clone(),
            file: legacy.file.clone(),
            newer_than: legacy.newer_than.clone(),
            count: legacy.count,
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
            select: out.select.clone(),
            exclude: self.exclude.clone(),
            pluck: self.pluck.clone(),
            sort_by: self.sort_by.clone(),
            limit: out.limit,
            offset: self.offset,
            distinct: self.distinct,
            group_by: self.group_by.clone(),
            count_by: self.count_by.clone(),
            count_distinct: self.count_distinct.clone(),
            ndjson: self.ndjson,
            lines: out.lines,
            raw: self.raw,
        }
    }
}
