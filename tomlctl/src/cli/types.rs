//! Clap derive types — the `Cli` root, `Cmd` subcommand enum, the
//! per-variant argument bundles (`ReadIntegrityArgs`, `WriteIntegrityArgs`,
//! `QueryArgs`), and the legacy shortcut adapter (`LegacyShortcuts`). The
//! clap surface lives here and the dispatch logic in the sibling
//! `dispatch` module; every type is `pub(crate)` so that module can match
//! on it.

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::convert::ScalarType;
use crate::dedup::DupTier;

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
    "json_ops",
    // Repo-scoped capture log: the `backlog` subcommand cluster.
    "backlog_capture", // the `add` verb
    "backlog_check",
    "backlog_cluster",
    "backlog_compact",
    "backlog_evidence",
    "backlog_list",
    "backlog_show",
    "backlog_relate",
    "backlog_triage",
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
    /// compact JSON envelope (`{"error":{"kind":...,"message":...,"file":...}}`)
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

    #[command(subcommand)]
    pub(crate) cmd: Cmd,
}

/// Stderr-format selector surfaced via `--error-format`. `pub(crate)` so
/// `main.rs` can pattern-match on the variant before dispatching to `run()`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ErrorFormat {
    /// Default — the plain `tomlctl: <anyhow chain>` line.
    Text,
    /// Single compact JSON line with `error.kind` taxonomy.
    Json,
}

/// Read-only integrity options. Read paths honour only
/// `--verify-integrity` — the other three flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`) are write-side concepts
/// that would be silently no-ops on a read, so they're structurally kept
/// off read subcommands.
///
/// `--strict-read` turns the "missing file → silent default" branches
/// (today only `items next-id --prefix <P>`) into a tagged `kind=not_found`
/// error. On every other read subcommand the flag is a benign no-op —
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
    /// an empty default. Every other read path is unaffected: `items list` /
    /// `items orphans` already error on a missing file, but
    /// `items next-id --prefix <P>` returns `"<P>1"` as a bootstrapping fast
    /// path. Pass `--strict-read` when the caller needs to distinguish
    /// "no matches in an existing ledger" from "ledger does not exist".
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
    /// intentionally edit a flow file in another location. Affects only TOML
    /// write paths (set / set-json / items *).
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
    /// / `plan-review-findings.toml` — and an empty table otherwise). Affects
    /// only TOML write paths (set / set-json / items * / array-append).
    #[arg(long = "no-create")]
    pub(crate) no_create: bool,
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
        help = "Filter: field is present (repeatable)"
    )]
    pub(crate) where_has: Vec<String>,
    #[arg(
        long = "where-missing",
        value_name = "KEY",
        help = "Filter: field is absent (repeatable)"
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
        long = "select",
        value_name = "F1,F2,...",
        help = "Projection: keep only the listed fields"
    )]
    pub(crate) select: Option<String>,
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
    #[arg(long = "limit", value_name = "N", help = "Return at most N items")]
    pub(crate) limit: Option<usize>,
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
    /// (projection on an aggregation-only shape would be ambiguous). The
    /// whole motivation is to replace the ~140 `--pluck f | jq -r '.[]'
    /// | sort -u | wc -l` pipe chains that agents were spelling out for
    /// cardinality readouts.
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
    /// Discoverable spelling of `--ndjson` for the `--pluck` case. A clap
    /// `alias` wouldn't appear in `items list --help`, defeating the whole
    /// point of exposing the flag — agents need to see it at a glance.
    /// `Query::from_query_input` merges this with `ndjson` (`lines || ndjson`),
    /// so downstream pipeline logic still inspects a single boolean.
    ///
    /// For non-Pluck/non-Array shapes (Count, CountBy, CountDistinct,
    /// GroupBy) this is a silent no-op — the output is a single JSON value
    /// regardless, so "one value per line" collapses to the same bytes.
    /// This keeps scripts free to blanket-add `--lines` without branching
    /// on shape.
    #[arg(
        long = "lines",
        help = "Emit one JSON value per line on --pluck (alias-of-semantics for --ndjson). No-op on --count/--count-by/--count-distinct/--group-by."
    )]
    pub(crate) lines: bool,
    /// Bare-scalar output for single-value shapes. Composes as follows:
    ///
    /// - `--count --raw` / `--count-distinct --raw`: emit the bare integer
    ///   count (no `{"count":...}` / `{"count_distinct":...,"field":...}`
    ///   wrapping).
    /// - `--pluck f --raw` (N=1): emit the bare plucked value (strings
    ///   unquoted, numbers/bools bare).
    /// - `--pluck f --raw` (N != 1): errors; the message is load-bearing —
    ///   tests assert it byte-for-byte.
    /// - `--pluck f --raw --lines`: one bare value per line (composes
    ///   with the streaming Pluck path).
    /// - `--count-by --raw` / `--group-by --raw`: rejected — the output is
    ///   a map, not a scalar; `--raw` has no well-defined conversion.
    ///
    /// Motivation: replaces the ~35 `tomlctl items list ... --count
    /// | jq -r .count` pipe chains the transcript audit found. Agents
    /// consuming counts into a `read -r N` bash loop want the bare integer
    /// on stdout without piping through jq.
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
    /// transfer.
    pub(crate) fn to_query_input(&self, legacy: &LegacyShortcuts<'_>) -> crate::query::QueryInput {
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
            select: self.select.clone(),
            exclude: self.exclude.clone(),
            pluck: self.pluck.clone(),
            sort_by: self.sort_by.clone(),
            limit: self.limit,
            offset: self.offset,
            distinct: self.distinct,
            group_by: self.group_by.clone(),
            count_by: self.count_by.clone(),
            count_distinct: self.count_distinct.clone(),
            ndjson: self.ndjson,
            lines: self.lines,
            raw: self.raw,
        }
    }
}

// The CLI subcommand enums carry a lot of `Vec<String>` / nested-struct
// fields by design — that's how clap's derive surface encodes a rich flag
// set. Clippy's `large_enum_variant` lint would have us `Box<…>` every
// heavy variant; doing that wouldn't improve clarity and would bloat the
// dispatch match arms. The CLI enums are constructed once per invocation
// and never collected into a Vec, so the size-asymmetry concern doesn't
// bite here.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Cmd {
    /// Parse a TOML file and print the whole document as JSON.
    Parse {
        file: PathBuf,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print the value at a dotted key path as JSON (or the whole doc if path is omitted).
    Get {
        file: PathBuf,
        /// Dotted path, e.g. "tasks.total" or "artifacts.optimise_findings". Omit to dump whole file.
        path: Option<String>,
        /// Bare-scalar output. On a scalar target (string / integer /
        /// float / bool / date), emit the value unquoted — strings print
        /// literally, numbers bare, booleans as `true` / `false`. On a
        /// table or array target, error with a load-bearing message tests
        /// assert byte-for-byte. The motivation is parity with `items list
        /// --count --raw`: agents consuming `tomlctl get <file>
        /// tasks.total` into a bash `read -r N` loop want the bare integer,
        /// not a JSON-quoted string.
        #[arg(
            long = "raw",
            help = "Emit bare scalar (no JSON quoting). Errors on table/array target."
        )]
        raw: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Set a scalar at a dotted key path. Type auto-inferred with --type.
    Set {
        file: PathBuf,
        path: String,
        value: String,
        #[arg(long = "type", value_enum)]
        ty: Option<ScalarType>,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Set a JSON-encoded value (array, object, or scalar) at a dotted key path.
    SetJson {
        file: PathBuf,
        path: String,
        #[arg(long, help = "JSON-encoded value; pass `-` to read from stdin")]
        json: String,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Parse-check only. Exit 0 on valid TOML, non-zero otherwise.
    Validate {
        file: PathBuf,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Operations on `[[items]]` arrays-of-tables (ledger schema).
    Items {
        #[command(subcommand)]
        op: ItemsOp,
    },

    /// Verify byte-identical shared blocks across multiple markdown files.
    /// Deliberately does NOT take `--allow-outside` / `--verify-integrity`
    /// / `--no-write-integrity` / `--strict-integrity` — `blocks verify` scans
    /// markdown (no TOML + sidecar pair) and never writes, so those flags
    /// have no semantic hook here. Passing one errors at the clap layer.
    Blocks {
        #[command(subcommand)]
        op: BlocksOp,
    },

    /// Append one or more records to an arbitrary array-of-tables. Thin
    /// discoverable wrapper over `items apply --array <name> --ops [...]`:
    /// `--json` appends a single object; `--ndjson` appends one per line
    /// (from stdin with `-` or from a file path). Primary use: append to
    /// `[[rollback_events]]` logs from `/review-apply` / `/optimise-apply`
    /// without constructing the `items apply` op-framing JSON.
    ArrayAppend {
        file: PathBuf,
        #[arg(help = "Array-of-tables name (e.g. rollback_events)")]
        array: String,
        #[arg(
            long,
            conflicts_with = "ndjson",
            help = "JSON object for a single record; pass `-` to read from stdin"
        )]
        json: Option<String>,
        #[arg(
            long = "ndjson",
            conflicts_with = "json",
            help = "NDJSON source: `-` for stdin, otherwise a file path"
        )]
        ndjson: Option<String>,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Emit a JSON description of this binary's capabilities. Downstream
    /// flow-command templates call this at boot and feature-gate on the
    /// returned `features` / `subcommands` lists without parsing `--help`
    /// prose. Pure metadata — no file arg, no integrity flags, no stdin.
    /// Output shape:
    ///
    /// ```json
    /// {"version":"0.2.0","features":[...],"subcommands":[...]}
    /// ```
    ///
    /// The `version` field is wired to `env!("CARGO_PKG_VERSION")` so the
    /// Cargo.toml version is the single source of truth; bumping the
    /// manifest automatically updates this output on the next rebuild.
    Capabilities,

    /// Sidecar-maintenance operations. Carved out as its own subcommand
    /// group so bootstrap / recovery primitives live next to the read-side
    /// `--verify-integrity` flag they support, rather than competing for
    /// real estate under `items` or `set`.
    Integrity {
        #[command(subcommand)]
        op: IntegrityOp,
    },

    /// Flow-aware operations: resolve the active flow, manage the
    /// `.claude/active-flow.toml` registry, bootstrap flow artifacts, and
    /// run invariant checks.
    Flow {
        #[command(subcommand)]
        op: FlowOp,
    },

    /// JSON-document read/write operations on a dotted path. Sibling of the
    /// TOML-side `get` / `set` / `set-json` triple, scoped to JSON files
    /// (e.g. `.claude/settings.json`).
    Json {
        #[command(subcommand)]
        op: JsonOp,
    },

    /// Repo-scoped capture log over `.claude/backlog.toml` — record a
    /// tangential discovery, ask whether one is already known, and triage
    /// what accumulates.
    Backlog {
        #[command(subcommand)]
        op: BacklogOp,
    },
}

/// Flow subcommand cluster. Each leaf op maps onto a dedicated
/// `flow/<leaf>.rs` module.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum FlowOp {
    /// Manage `.claude/active-flow.toml` registry.
    Active {
        #[command(subcommand)]
        op: ActiveOp,
    },
    /// Discover plan files under configured directories.
    FindPlans {
        /// One or more directories to scan for plan markdown files. Repeat the
        /// flag for each additional directory; absolute or repo-relative paths
        /// are accepted. Overrides `tomlctl.plansDirectories` / `plansDirectory`
        /// in `.claude/settings.json` when provided.
        #[arg(long = "dirs", value_name = "DIR")]
        dirs: Vec<PathBuf>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Report staleness of a flow's `context.toml`.
    Stale {
        /// Flow slug to inspect (`<root>/.claude/flows/<slug>/context.toml`).
        #[arg(long = "slug")]
        slug: String,
        /// Staleness threshold as `<n>{s|m|h|d|w}` (default: `7d`).
        /// `flow stale` flips `stale=true` when the flow's `updated` date is
        /// older than this duration.
        #[arg(long = "threshold", default_value = "7d")]
        threshold: String,
        /// Emit single-line compact JSON instead of pretty-printed JSON.
        #[arg(long = "json")]
        json: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Initialise a new flow (idempotent).
    Init {
        /// Flow slug — must match `^[a-z0-9][a-z0-9-]{0,63}$`.
        #[arg(long = "slug")]
        slug: String,
        /// Path to the plan markdown file the flow tracks.
        #[arg(long = "plan")]
        plan: PathBuf,
        /// Optional `branch` to record in `context.toml` and the active-flow registry.
        #[arg(long = "branch")]
        branch: Option<String>,
        /// Optional `worktree` (absolute path) recorded on the active-flow entry.
        #[arg(long = "worktree")]
        worktree: Option<PathBuf>,
        /// Scope-glob patterns recorded on the active-flow entry. Repeatable.
        #[arg(long = "scope")]
        scope: Vec<String>,
        /// Preview the bootstrap without writing. Emits a `would_change` summary.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// Build the canonical flow-bootstrap input envelope as JSON and emit
    /// it on stdout. Replaces ~15 lines of inline carrier prose that
    /// hand-rolled this envelope in every flow command's Step-0 dispatch.
    /// See `claude/agents/flow-bootstrap.md` for the schema this emits.
    Envelope {
        #[command(subcommand)]
        op: EnvelopeOp,
    },
    /// Report (and optionally bootstrap) a flow artifact.
    EnsureArtifact {
        /// Flow slug whose artifact is under inspection.
        #[arg(long = "slug")]
        slug: String,
        /// Artifact kind (context, execution-record, review-ledger,
        /// optimise-findings, plan-review-findings).
        #[arg(long = "kind", value_enum)]
        kind: ArtifactKind,
        /// When set on `kind=execution-record`, materialise the 2-line
        /// `schema_version=1` skeleton (idempotent — no-op if file present).
        #[arg(long = "bootstrap")]
        bootstrap: bool,
        /// Preview the bootstrap without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// Resolve the active flow via the 6-step algorithm.
    Resolve {
        /// Step-1 explicit override: bypass discovery and use this slug.
        #[arg(
            long = "flow",
            help = "Step-1 override: resolve to this flow slug verbatim"
        )]
        flow: Option<String>,
        /// Step-2 scope-glob filter: paths to test against each candidate flow's
        /// `scope` array. Repeatable.
        #[arg(
            long = "path",
            value_name = "PATH",
            help = "Step-2 scope-glob: caller path tested against each flow's scope (repeatable)"
        )]
        path: Vec<PathBuf>,
        /// Step-3/5 branch hint — match active-flow registry binding by branch,
        /// else fall through to step-5 branch-match against `context.toml`.
        #[arg(
            long = "branch",
            help = "Step-3 binding hint / step-5 branch-match filter"
        )]
        branch: Option<String>,
        /// Step-3 binding hint — match active-flow registry binding by worktree path.
        #[arg(
            long = "worktree",
            help = "Step-3 binding hint: match active-flow registry by worktree"
        )]
        worktree: Option<PathBuf>,
        /// Annotate the resolved envelope with `{stale, age_seconds, reason}`.
        #[arg(
            long = "with-staleness",
            help = "Annotate envelope with staleness verdict (7d threshold)"
        )]
        with_staleness: bool,
        /// Emit JSON. On by default — `--json=false` would emit JSON anyway
        /// (read-side tomlctl idiom). Retained for callers that pass it explicitly.
        #[arg(
            long = "json",
            default_value_t = true,
            help = "Emit JSON envelope (always on — flag retained for explicit-pass callers)"
        )]
        json: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Run invariant checks across flows.
    Doctor {
        /// When set, scope checks to a single flow slug; otherwise every flow
        /// under `.claude/flows/` is checked.
        #[arg(long = "slug")]
        slug: Option<String>,
        /// Auto-repair sidecar mismatches and prune stale active-flow entries.
        #[arg(long = "fix")]
        fix: bool,
        /// Accepted no-op (compat). `flow doctor` always emits JSON on stdout —
        /// the flag exists so callers may uniformly pass `--json` across every
        /// tomlctl subcommand without per-command special-casing. Removing it
        /// breaks `claude/agents/flow-bootstrap.md` step 4, which passes it.
        #[arg(long = "json")]
        _json: bool,
        /// Preview `--fix` actions without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// List all flows.
    List {
        /// Filter by `context.toml`'s `status` field (exact-string match).
        #[arg(long = "status")]
        status: Option<String>,
        /// Filter by `context.toml`'s `branch` field (exact-string match).
        #[arg(long = "branch")]
        branch: Option<String>,
        /// Cross-reference with `.claude/active-flow.toml` and only emit slugs
        /// present in the registry.
        #[arg(long = "active-only")]
        active_only: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Regenerate a flow's `PROGRESS-LOG.md` from its `execution-record.toml`.
    ///
    /// Deterministic render-from-log: the markdown is a pure function of the
    /// execution record + the flow title (read from the plan's `# Plan:` header,
    /// falling back to a title-cased slug). Re-running produces byte-identical
    /// output. The written file is a DERIVED artifact — no `.sha256` sidecar is
    /// written for it.
    RenderProgressLog {
        /// Flow slug whose `PROGRESS-LOG.md` is regenerated
        /// (`<root>/.claude/flows/<slug>/`).
        #[arg(long = "slug")]
        slug: String,
        /// Print the rendered markdown to stdout instead of writing the file
        /// (preview / testing).
        #[arg(long = "stdout")]
        stdout: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ActiveOp {
    /// List entries in the active-flow registry.
    List {
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Add (or update) a flow in the active-flow registry.
    Add {
        /// Slug to upsert in the registry.
        #[arg(long = "slug")]
        slug: String,
        /// Branch to record on the entry's `[active.binding]` table.
        #[arg(long = "branch")]
        branch: Option<String>,
        /// Worktree absolute path (per-clone) recorded on the binding.
        #[arg(long = "worktree")]
        worktree: Option<PathBuf>,
        /// Scope-glob pattern (repeatable) recorded on the binding.
        #[arg(long = "scope")]
        scope: Vec<String>,
        /// Preview the upsert without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// Remove a flow from the active-flow registry.
    Remove {
        /// Slug to remove.
        #[arg(long = "slug")]
        slug: String,
        /// Preview the removal without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// Update a flow's last-touched timestamp in the registry.
    Touch {
        /// Slug whose `last_used` should be refreshed.
        #[arg(long = "slug")]
        slug: String,
        /// Preview the touch without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
}

/// `flow envelope` subcommand cluster — currently a single `build` leaf
/// that emits the canonical `flow-bootstrap` input envelope. Lives as its
/// own nested enum (rather than a flat `FlowOp::EnvelopeBuild` variant) so
/// the on-CLI spelling is `tomlctl flow envelope build …` — matches the
/// invocation form documented in the `flow-bootstrap` agent contract and
/// the carriers' Step-0 prose.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum EnvelopeOp {
    /// Build the canonical flow-bootstrap input envelope as JSON and emit
    /// it on stdout. Pure / read-only: no filesystem writes, no flow-state
    /// mutation. Validates `--command` against the carrier whitelist and
    /// `--require-artifact` against the canonical artifact set.
    Build {
        /// Carrier command this envelope is for (e.g. "review", "implement", "plan-new").
        #[arg(long)]
        command: String,
        /// Optional explicit flow slug override (passed through to the bootstrap agent).
        #[arg(long = "flow-override")]
        flow_override: Option<String>,
        /// Repeatable path argument; each value is appended to the envelope's
        /// `path_args` array verbatim.
        #[arg(long = "path-arg")]
        path_arg: Vec<String>,
        /// Current git branch — typically `$(git branch --show-current)`. Omit if detached HEAD.
        #[arg(long)]
        branch: Option<String>,
        /// Git worktree top-level — typically `$(git rev-parse --show-toplevel)`.
        #[arg(long)]
        worktree: Option<String>,
        /// Current working directory — typically `$(pwd)`.
        #[arg(long)]
        cwd: Option<String>,
        /// Repeatable artifact key that must exist (review_ledger,
        /// optimise_findings, execution_record, plan_review_findings).
        #[arg(long = "require-artifact")]
        require_artifact: Vec<String>,
        /// Staleness threshold (default "7d").
        #[arg(long = "staleness-threshold", default_value = "7d")]
        staleness_threshold: String,
    },
}

/// JSON-document read/write surface — sibling of the TOML side's
/// `get` / `set` / `set-json`, scoped to JSON files (e.g.
/// `.claude/settings.json`).
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum JsonOp {
    /// Read a value at a dotted path from a JSON file.
    Get {
        file: PathBuf,
        path: String,
        #[arg(long = "raw")]
        raw: bool,
        #[arg(long = "json")]
        json: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Set a JSON-encoded value at a dotted path.
    Set {
        file: PathBuf,
        path: String,
        #[arg(long = "json", value_name = "VALUE")]
        json: String,
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
    /// Remove the value at a dotted path.
    Unset {
        file: PathBuf,
        path: String,
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
}

/// Backlog subcommand cluster. Every op resolves its own store path
/// (`<repo-root>/.claude/backlog.toml`) rather than taking a file
/// argument, and every op emits JSON — there is no `--json` output flag
/// anywhere in the group.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum BacklogOp {
    /// Capture a discovery.
    Add {
        /// Hashed, after normalisation, into the item's content-derived id —
        /// a rephrasing that survives normalisation mints a second item.
        #[arg(long, required_unless_present = "json")]
        summary: Option<String>,
        /// bug|flaky-test|debt|direction|annoyance|question|other. Free-form
        /// rather than a `value_enum`: the store coerces an unrecognised
        /// kind to `other` with a warning, and a parser-level enum would
        /// turn that fail-soft rule into a hard error.
        #[arg(long)]
        kind: Option<String>,
        /// Repo-relative file or directory prefix the discovery sits under.
        #[arg(long)]
        area: Option<String>,
        #[arg(long = "tag", value_name = "TAG", help = "Free-form tag (repeatable)")]
        tag: Vec<String>,
        #[arg(
            long = "evidence",
            value_name = "REF",
            help = "`path:line` pointer into tracked source, or a bare filename in the item's evidence directory (repeatable)"
        )]
        evidence: Vec<String>,
        #[arg(
            long = "related",
            value_name = "ID",
            help = "Existing backlog id this item relates to (repeatable)"
        )]
        related: Vec<String>,
        /// How to work around the issue. This is what makes a later `check`
        /// hit actionable rather than merely informative.
        #[arg(long)]
        context: Option<String>,
        #[arg(long, help = "Command or agent that minted this item")]
        origin: Option<String>,
        #[arg(long, help = "Flow slug in force at mint time")]
        flow: Option<String>,
        #[arg(
            long = "on-duplicate",
            value_enum,
            default_value_t = OnDuplicate::Bump,
            help = "Behaviour when the computed dedup_id already exists"
        )]
        on_duplicate: OnDuplicate,
        #[arg(
            long,
            help = "Whole-item JSON payload instead of the field flags; pass `-` to read from stdin"
        )]
        json: Option<String>,
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Ask whether a discovery is already known, before minting it.
    /// Read-only; emits a graded verdict plus the matching items' stored
    /// context.
    Check {
        #[arg(
            long,
            help = "The discovery being weighed; pass `-` to read it from stdin"
        )]
        summary: String,
        #[arg(long)]
        area: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long = "tag", value_name = "TAG", help = "Free-form tag (repeatable)")]
        tag: Vec<String>,
        #[arg(long, default_value_t = 5, help = "Return at most N candidates")]
        limit: usize,
        /// Char-trigram Jaccard at or above which a candidate is reported as
        /// `likely-duplicate`. Omit to use the pinned default.
        #[arg(long = "similarity-strong", value_name = "0.0-1.0")]
        similarity_strong: Option<f64>,
        /// Word Jaccard at or above which a candidate is reported as
        /// `related`. Named for the threshold, not for `add --related`,
        /// which is an id list.
        #[arg(long = "similarity-related", value_name = "0.0-1.0")]
        similarity_related: Option<f64>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Query the store. The full `--where-*` / projection / aggregation
    /// surface applies, plus the convenience filters below.
    List {
        #[arg(long, help = "Exact match on the item's `status` field")]
        status: Option<String>,
        #[arg(long, help = "Exact match on the item's `kind` field")]
        kind: Option<String>,
        #[arg(
            long = "tag",
            value_name = "TAG",
            help = "Item carries TAG (repeatable, AND across repeats)"
        )]
        tag: Vec<String>,
        #[arg(long, help = "Shorthand for --status open")]
        open: bool,
        /// Matches on repo-path component boundaries, so `lumina/server`
        /// selects `lumina/server/pty/x.rs` but not `lumina/server-extras/y.rs`.
        #[arg(long = "area-prefix", value_name = "PATH")]
        area_prefix: Option<String>,
        /// Computed by reading `.claude/backlog-evidence/<id>/` — nothing in
        /// the store records whether evidence exists.
        #[arg(
            long = "has-evidence",
            help = "Keep only items whose evidence directory holds files"
        )]
        has_evidence: bool,
        #[arg(long, help = "Emit only the row count")]
        count: bool,
        #[command(flatten)]
        query: QueryArgs,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Print one item with its one-hop relation neighbourhood and its
    /// evidence-directory listing.
    Show {
        id: String,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Write a typed edge between two items.
    Relate {
        /// Subject of the edge — the item that gains `related` /
        /// `duplicate_of` / `supersedes`.
        a: String,
        #[arg(long = "to", value_name = "ID")]
        to: String,
        #[arg(long = "as", value_enum, value_name = "KIND")]
        relation: RelationKind,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Transition one or more items out of (or back into) `open`.
    Triage {
        #[arg(required = true, value_name = "ID")]
        ids: Vec<String>,
        #[command(flatten)]
        mode: TriageMode,
        /// Flow slug or repo-relative plan path, stored verbatim. Nothing is
        /// generated from it.
        #[arg(long = "to", value_name = "REF")]
        to: Option<String>,
        #[arg(long, value_name = "TEXT", help = "Companion to --dismiss")]
        reason: Option<String>,
        #[arg(long, value_name = "TEXT", help = "Companion to --resolve")]
        resolution: Option<String>,
        #[arg(long, value_name = "TEXT", help = "Companion to --reopen")]
        rationale: Option<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Group open items into candidate work scopes.
    Cluster {
        #[arg(long = "by", value_enum, default_value_t = ClusterBy::All)]
        by: ClusterBy,
        #[arg(
            long = "min-size",
            default_value_t = 2,
            help = "Smallest group the area view will emit"
        )]
        min_size: usize,
        #[arg(
            long = "min-shared-tags",
            default_value_t = 2,
            help = "Tags two items must share before the tags view groups them"
        )]
        min_shared_tags: usize,
        #[arg(
            long = "all-statuses",
            help = "Cluster every item, not just `open` ones"
        )]
        all_statuses: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Age decided items out of `[[backlog]]` into `[[compacted]]`.
    /// `open` items are never touched regardless of age.
    Compact {
        /// `<n>{s|m|h|d|w}`, the same grammar as `flow stale --threshold`.
        #[arg(long = "older-than", default_value = "90d", value_name = "DURATION")]
        older_than: String,
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Per-item evidence directories under `.claude/backlog-evidence/`.
    /// Files arrive by `cp` and leave by `rm`; there is no copy verb and no
    /// prune verb.
    Evidence {
        #[command(subcommand)]
        op: EvidenceOp,
    },
}

/// `backlog evidence` leaves. Both read the store, so both carry
/// `ReadIntegrityArgs`. Neither carries `WriteIntegrityArgs`: `dir` writes
/// one non-TOML marker file and `audit` writes nothing, so there is no
/// sidecar to refresh — and handing `dir` the write bundle would hand it
/// `--allow-outside`.
#[derive(Subcommand)]
pub(crate) enum EvidenceOp {
    /// Resolve an id against the store and print its evidence directory,
    /// creating the directory and its `.evidence` marker when absent.
    ///
    /// Resolving rather than deriving is the whole job: ids widen from 8 to
    /// 10 to 12 hex on collision, so a path built from an eyeballed 8-hex
    /// prefix is owned by nobody and `audit` later reports it as `unowned`.
    Dir {
        id: String,
        #[arg(
            long = "no-create",
            help = "Report the directory without creating it; error if absent"
        )]
        no_create: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
    /// Walk `.claude/backlog-evidence/` and report every directory the
    /// store does not own, plus policy and stale-reference findings.
    Audit {
        /// Exit 1 on `unowned`, `no-marker`, `oversize`,
        /// `disallowed-extension` or `referenced-missing`. Never on
        /// `tracked` or `empty` — a tracked file is a deliberate
        /// `git add -f` and an empty directory is the expected state in a
        /// fresh clone.
        #[arg(long)]
        strict: bool,
        #[arg(
            long = "max-bytes",
            value_name = "N",
            help = "Oversize threshold; omit for the built-in default"
        )]
        max_bytes: Option<u64>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },
}

/// The four `backlog triage` transitions, as a mutually-exclusive required
/// group. An `ArgGroup` has to hang off an `Args` struct — a Subcommand
/// variant takes only `skip` / `flatten` / `external_subcommand` — so the
/// mode flags live here while their companion values stay on the variant,
/// which is what keeps a companion from counting as a second mode.
#[derive(Args, Clone)]
#[group(required = true, multiple = false)]
pub(crate) struct TriageMode {
    #[arg(long, help = "Status → promoted; takes --to")]
    pub(crate) promote: bool,
    #[arg(long, help = "Status → dismissed; takes --reason")]
    pub(crate) dismiss: bool,
    #[arg(long, help = "Status → resolved; takes --resolution")]
    pub(crate) resolve: bool,
    /// Clears the terminal date and companion. `--rationale` is enforced at
    /// the parser because `reopen_rationale` is the only companion field an
    /// `open` item is allowed to carry, so a bare `--reopen` would write an
    /// item the validator then rejects.
    #[arg(
        long,
        requires = "rationale",
        help = "Status → open; requires --rationale"
    )]
    pub(crate) reopen: bool,
}

/// What `backlog add` does when the computed `dedup_id` is already in the
/// store.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum OnDuplicate {
    /// Increment `seen_count`, refresh `last_seen`, union `tags`, `evidence`
    /// and `related`; leave `summary` and `status` alone.
    Bump,
    /// Report the existing item and write nothing.
    Skip,
    /// Error.
    Fail,
}

/// Typed relation written by `backlog relate`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum RelationKind {
    /// Symmetric — both items gain the other in `related`.
    RelatesTo,
    /// `a` duplicates `b`: sets `a.duplicate_of` and dismisses `a`.
    Duplicates,
    /// `a` supersedes `b`: sets `a.supersedes` and dismisses `b`.
    Supersedes,
}

/// Which clustering views `backlog cluster` emits. The views are
/// independent and separately keyed; they are never blended into one score.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ClusterBy {
    /// Longest common repo-path prefix, collapsing upward to `--min-size`.
    Area,
    /// Items sharing at least `--min-shared-tags` tags, merged transitively.
    Tags,
    /// Connected components over the typed edge set.
    Relations,
    All,
}

/// Flow-artifact kinds surfaced by `flow ensure-artifact`. Variants are
/// rendered by clap's default `value_enum` casing as kebab-case
/// (`context`, `execution-record`, `review-ledger`, `optimise-findings`,
/// `plan-review-findings`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ArtifactKind {
    Context,
    ExecutionRecord,
    ReviewLedger,
    OptimiseFindings,
    PlanReviewFindings,
}

#[derive(Subcommand)]
pub(crate) enum IntegrityOp {
    /// Regenerate `<file>.sha256` from the file's current on-disk bytes.
    ///
    /// Bootstrap: `/plan-new` materialises `execution-record.toml` via the
    /// `Write` tool (a single-filesystem-op atomic write that bypasses
    /// tomlctl's write pipeline and therefore never produces a sidecar).
    /// The first downstream read with `--verify-integrity` then fails.
    /// Running `integrity refresh` immediately after the `Write` closes
    /// the gap so every subsequent read honours the integrity contract
    /// without a special "first-read-after-bootstrap" grace branch.
    ///
    /// Recovery: if a sidecar was accidentally deleted (git clean, stray
    /// rm) but the TOML is intact, refresh regenerates the sidecar from
    /// the existing bytes without a round-trip through `set` (which would
    /// rewrite the TOML and bump mtime for no semantic reason).
    ///
    /// Does NOT modify the TOML file itself — the caller is trusting that
    /// the current on-disk bytes are authoritative. Acquires the same
    /// exclusive lock a write path would, so it serialises correctly
    /// with concurrent writers.
    ///
    /// Refresh is a pure content-digest primitive — it hashes the raw
    /// on-disk bytes and never parses TOML. A malformed file (e.g. one
    /// truncated by a partial write) will silently receive a valid
    /// sidecar. For the recovery path, consider running `tomlctl validate
    /// <path>` before `integrity refresh` so syntactic corruption surfaces
    /// instead of being papered over.
    ///
    /// Carries the full `WriteIntegrityArgs` bundle for parity with
    /// every other write subcommand, but not every flag has a semantic
    /// hook on this sidecar-only operation:
    ///
    /// - `--allow-outside`: honoured (same containment guard as other writes).
    /// - `--verify-integrity`: when set, if a sidecar already exists, it is
    ///   verified before being overwritten. A digest mismatch propagates as
    ///   a hard error — guards against clobbering a mismatched sidecar
    ///   during recovery. No existing sidecar → silent proceed (the whole
    ///   point of the bootstrap path).
    /// - `--no-write-integrity`: structurally meaningless (refresh IS the
    ///   sidecar write); passing it errors with a directed message.
    /// - `--strict-integrity`: structurally meaningless (refresh has no
    ///   fallback path to strict-ify); silently ignored so composable
    ///   wrapper scripts that blanket-add the flag don't trip.
    Refresh {
        file: PathBuf,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ItemsOp {
    /// List items as a JSON array. Optional filters combine via AND. With
    /// `--count`, print `{"count": <n>}` instead of the item array.
    ///
    /// `--count`, `--count-by`, `--group-by`, `--pluck` and
    /// `--count-distinct` are mutually exclusive at the CLI layer via an
    /// `ArgGroup`, so a mismatched pair surfaces as a clap error at parse
    /// time (e.g. `--count-distinct x --pluck y`) rather than silently
    /// collapsing to a single shape. `--ndjson` is a separate encoding
    /// flag, so it stays out of the group.
    #[command(group(clap::ArgGroup::new("shape").multiple(false).args(["count", "count_by", "group_by", "pluck", "count_distinct"])))]
    List {
        file: PathBuf,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(
            long = "newer-than",
            help = "Include items whose first_flagged is strictly after this ISO date (YYYY-MM-DD)"
        )]
        newer_than: Option<String>,
        #[arg(long = "file", help = "Exact match on the item's `file` field")]
        file_filter: Option<String>,
        #[arg(
            long,
            help = "Print `{\"count\": N}` of matching items instead of the array"
        )]
        count: bool,
        /// Target array-of-tables name. Defaults to `items` (the ledger
        /// schema). Use e.g. `--array rollback_events` to list a non-default
        /// array of records.
        #[arg(long, default_value = "items")]
        array: String,

        // The full predicate/projection/shaping surface lives on
        // `QueryArgs` so the `next_help_heading = "Query options"`
        // setting can be applied there (clap forbids it inside a Subcommand
        // variant field). All repeatable flags AND-combine with the legacy
        // shortcut flags above.
        #[command(flatten)]
        query: QueryArgs,

        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Get a single item by its `id` field.
    Get {
        file: PathBuf,
        id: String,
        /// Target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Append a new item. --json is the JSON object payload.
    Add {
        file: PathBuf,
        #[arg(
            long,
            help = "JSON object for the new item; pass `-` to read from stdin"
        )]
        json: String,
        /// Target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        /// Skip the add when an existing item already matches the
        /// incoming payload on every listed field. Comma-separated list,
        /// dotted paths for nested object fields (e.g. `summary,file` or
        /// `meta.source_run`). Raw JSON equality; use `--where` upstream
        /// for typed comparison. Does NOT implicitly include `dedup_id`.
        #[arg(
            long = "dedupe-by",
            value_name = "F1,F2,...",
            help = "Skip the add when an existing item matches these fields (raw equality; use --where for typed comparison)"
        )]
        dedupe_by: Option<String>,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Append many items in one batch from NDJSON. `--defaults-json` stamps
    /// common fields on every row (per-row keys win on conflict). One parse,
    /// one lock, one rewrite. On a malformed line N the batch aborts before
    /// mutating the file. Output: `{"ok":true,"added":N}`.
    AddMany {
        file: PathBuf,
        #[arg(
            long = "ndjson",
            help = "NDJSON source: `-` for stdin, otherwise a file path"
        )]
        ndjson: String,
        #[arg(
            long = "defaults-json",
            help = "JSON object of default field values; pass `-` to read from stdin"
        )]
        defaults_json: Option<String>,
        #[arg(long, default_value = "items")]
        array: String,
        /// Skip rows whose merged payload already matches an existing
        /// item on every listed field. See `Add --dedupe-by`. When any
        /// rows are skipped, the output adds `"skipped":M` and
        /// `"skipped_rows":[{"row":N,"matched_id":"..."}, ...]`
        /// (input-order ascending).
        #[arg(
            long = "dedupe-by",
            value_name = "F1,F2,...",
            help = "Skip rows whose values at these fields already exist (raw equality; use --where for typed comparison)"
        )]
        dedupe_by: Option<String>,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Merge fields into an existing item (matched by `id`). --json is a patch object.
    Update {
        file: PathBuf,
        id: String,
        #[arg(
            long,
            help = "JSON patch object merged into the item; pass `-` to read from stdin"
        )]
        json: String,
        /// Remove a field from the matched item. Repeatable. Applied AFTER the
        /// `--json` patch, so an `--unset` trumps a same-key set from `--json`.
        /// A key that does not exist on the item is silently ignored.
        #[arg(long = "unset")]
        unset: Vec<String>,
        /// Target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Remove an item by id. Fails if no such id exists.
    Remove {
        file: PathBuf,
        id: String,
        /// Target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        /// Preview the removal without writing. Emits a
        /// `would_change` summary (counts + ids) on stdout and leaves
        /// the ledger + sidecar byte-identical. The compute phase runs
        /// in full (same validation gates, same errors on missing id)
        /// so the preview is a faithful rehearsal of the real remove.
        #[arg(
            long = "dry-run",
            help = "Preview the removal without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Print the next id string for the given prefix.
    /// This is a read-only path (reads the ledger to find the max
    /// existing id, never writes), so it carries `ReadIntegrityArgs` — the
    /// write-side containment/sidecar flags have no semantic hook here and
    /// would be silently ignored if they were accepted.
    ///
    /// Neither `--prefix` nor `--infer-from-file` has a default. With
    /// four ledger schemas now in circulation (R review, O optimise, E
    /// execution-record, plus any future additions), a default of "R" would
    /// silently mis-mint for three of four callers. Every
    /// `tomlctl items next-id` invocation in this repo's
    /// `claude/commands/*.md` and `SKILL.md` already passes an explicit
    /// `--prefix R|O|E`, so structurally requiring one of the two flags is
    /// a no-op for well-formed callers and a fail-fast for careless ones.
    ///
    /// `--infer-from-file` is the alternative path for callers handed an
    /// arbitrary `<ledger>` without knowing its prefix up front. It scans
    /// existing ids and returns `{prefix}{max_n+1}` when exactly one prefix
    /// is in use; on zero (empty ledger, no explicit prefix) or more than
    /// one it errors out rather than guessing. Structurally mutually
    /// exclusive with `--prefix` via `conflicts_with = "prefix"`;
    /// `--prefix` stays `required_unless_present = "infer_from_file"` so
    /// the "no silent default" contract above is preserved (omitting both
    /// still fails at clap with the "required arguments were not provided"
    /// message).
    NextId {
        file: PathBuf,
        #[arg(
            long,
            required_unless_present = "infer_from_file",
            help = "Prefix letter (e.g. R, O, E) for the new id"
        )]
        prefix: Option<String>,
        /// Derive the prefix by scanning existing ids in the ledger.
        /// Errors if the ledger is empty or uses more than one prefix.
        #[arg(
            long = "infer-from-file",
            conflicts_with = "prefix",
            help = "Infer the prefix from existing ids in <file>"
        )]
        infer_from_file: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Apply a batch of add/update/remove operations in a single file rewrite.
    Apply {
        file: PathBuf,
        #[arg(
            long,
            help = "JSON array of ops, each `{\"op\":\"add|update|remove\", ...}`; pass `-` to read from stdin"
        )]
        ops: String,
        /// Target array-of-tables name. Defaults to `items` (the ledger schema).
        /// Use e.g. `--array rollback_events` to append to a different array.
        #[arg(long, default_value = "items")]
        array: String,
        /// Reject any `remove` op in the batch. Used by review-apply and
        /// optimise-apply to prevent an agent-generated ops payload from
        /// erasing audit history — those flows transition status via
        /// `update`, never delete. Off by default so the CLI still supports
        /// legitimate batch deletions from trusted callers.
        #[arg(long = "no-remove")]
        no_remove: bool,
        /// Preview the batch without writing. Runs every validation
        /// gate (`--no-remove`, op-shape, missing-id, dedup_id auto-populate)
        /// so an agent can rehearse the batch shape before committing.
        /// Emits `{"ok":true,"dry_run":true,"would_change":{...}}`.
        #[arg(
            long = "dry-run",
            help = "Preview the batch without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Find duplicate items using one of the dedup tiers.
    ///
    /// `--across <other>` runs the selected tier over the UNION of
    /// `<file>`'s items and `<other>`'s items, tagging each emitted
    /// JSON entry with its source ledger's basename under `source_file`.
    /// Tier C is file-scoped by design (its line-window grouping assumes
    /// one source file); passing `--tier C` together with `--across`
    /// errors at runtime with the exact documented message. Tier A and
    /// tier B both work cross-ledger.
    FindDuplicates {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = DupTier::A)]
        tier: DupTier,
        /// Run cross-ledger — compare items from `<file>` against
        /// items from `<PATH>` and emit matches from the union. Output
        /// items carry a `source_file` basename tag. Tier C errors.
        #[arg(
            long = "across",
            value_name = "PATH",
            help = "Compare against a second ledger; output items carry a `source_file` tag (tier A or B only)"
        )]
        across: Option<PathBuf>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Surface items whose file or symbol has drifted, or whose depends_on
    /// points at an id that isn't in the ledger.
    Orphans {
        file: PathBuf,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Explicit, auditable upgrade path for legacy ledgers whose items lack
    /// `dedup_id`. Walks every item in the ledger, computes
    /// `tier_b_fingerprint` on any item missing the field, and writes the
    /// updated ledger atomically via the same compute/apply split as
    /// `items remove --dry-run` / `items apply --dry-run`.
    ///
    /// Contract (idempotent, preservation-safe):
    ///
    /// - Items that already carry `dedup_id` are NEVER recomputed — the
    ///   existing value is preserved byte-for-byte regardless of whether
    ///   the fingerprinted fields have since drifted. If a legacy digest
    ///   needs replacing, use `items update --json '{"dedup_id":"..."}'`.
    /// - Re-running the subcommand on a fully-populated ledger is a no-op:
    ///   the file is NOT rewritten, the `.sha256` sidecar does not bump
    ///   (no mtime churn, no lock take other than the initial read).
    /// - `TOMLCTL_NO_DEDUP_ID=1` short-circuits to a documented
    ///   `{"ok":true,"backfilled":0,"reason":"disabled-by-env"}` output
    ///   without reading the ledger.
    ///
    /// Output shape:
    ///
    /// - Work done: `{"ok":true,"backfilled":N}` where N is the count of
    ///   newly-populated items.
    /// - Nothing to do: `{"ok":true,"backfilled":0}`.
    /// - `--dry-run`: `{"ok":true,"dry_run":true,"would_backfill":N,"ids":[...]}`.
    BackfillDedupId {
        file: PathBuf,
        /// Target array-of-tables name. Defaults to `items` (the ledger
        /// schema). Use e.g. `--array rollback_events` for non-standard
        /// arrays that carry a `dedup_id` contract.
        #[arg(long, default_value = "items")]
        array: String,
        /// Preview the backfill without writing. Emits
        /// `{"ok":true,"dry_run":true,"would_backfill":N,"ids":[...]}` and
        /// leaves the ledger + sidecar byte-identical. Honours the kill
        /// switch env var the same way the live path does — a dry run
        /// with `TOMLCTL_NO_DEDUP_ID=1` set emits the same `disabled-by-env`
        /// shape as a real run, just without ever touching the filesystem.
        #[arg(
            long = "dry-run",
            help = "Preview the backfill without writing. Emits a would_backfill summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
}

#[derive(Subcommand)]
pub(crate) enum BlocksOp {
    /// Verify one or more named shared-blocks are byte-identical across files.
    ///
    /// Each `<marker-name>` is scanned for the HTML-comment pair:
    ///   `<!-- SHARED-BLOCK:<marker-name> START -->` … `<!-- SHARED-BLOCK:<marker-name> END -->`
    /// The hash is taken over the byte-content strictly between the markers
    /// (each line joined by `\n`, matching `awk '{print}' | sha256sum`).
    Verify {
        /// Files to check.
        files: Vec<PathBuf>,
        /// Block name(s) to verify. If omitted, the union of block names
        /// present in the first listed file is used.
        #[arg(long = "block")]
        block: Vec<String>,
    },
    /// Verify each externalised flow-contract skill body matches the copies
    /// still embedded in non-migrated carriers (drift check). Reads the
    /// `skill = "..."` field from the shared-blocks manifest; skips blocks
    /// without a skill or with an empty file list. Exits 1 on drift.
    VerifySkills {
        /// Manifest path (defaults to scripts/shared-blocks.toml when omitted; resolved in dispatch).
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
}

/// Legacy shortcut flags that predate the `--where-*` family on `items list`.
/// Kept on the CLI for back-compat (`--status`, `--category`, `--file`,
/// `--newer-than`) but translated into equivalent `Predicate` entries in
/// `Query::from_query_input` so the query engine only sees one predicate
/// list. Bundled into a small struct so the adapter takes
/// `(legacy, query)` rather than one positional parameter per flag.
pub(crate) struct LegacyShortcuts<'a> {
    pub(crate) status: &'a Option<String>,
    pub(crate) category: &'a Option<String>,
    pub(crate) file: &'a Option<String>,
    pub(crate) newer_than: &'a Option<String>,
    pub(crate) count: bool,
}
