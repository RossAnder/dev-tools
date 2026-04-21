//! R21: clap derive types — the `Cli` root, `Cmd` subcommand enum, the
//! per-variant argument bundles (`ReadIntegrityArgs`, `WriteIntegrityArgs`,
//! `QueryArgs`), and the legacy shortcut adapter (`LegacyShortcuts`). Split
//! out of the former monolithic `cli.rs` so the clap surface lives in one
//! place and the dispatch logic in another. Every type is `pub(crate)` so
//! the sibling `dispatch` module can match on it; field visibility is
//! preserved from the pre-split file (most fields were already `pub(crate)`
//! after R15/R18).

use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::convert::ScalarType;
use crate::dedup::DupTier;

/// T7: capabilities advertised by `tomlctl capabilities`. Each entry is
/// stable across patch versions within a minor release — removing an entry
/// is a breaking change. Add new entries for new user-facing flags;
/// don't version-qualify (the `version` field is the release marker). The
/// downstream flow-command templates call `tomlctl capabilities` at boot
/// and feature-gate on this list without having to parse `--help` prose.
pub(crate) const FEATURES: &[&str] = &[
    "count_distinct",         // T1
    "raw",                    // T2
    "lines",                  // T3
    "infer_prefix",           // T4
    "dedupe_by",              // T5
    "dedup_id_auto",          // T6b
    "find_duplicates_across", // T6c
    "capabilities",           // T7
    "error_format_json",      // T8
    "strict_read",            // T9
    "dry_run",                // T10
    "backfill_dedup_id",      // T11
    "integrity_refresh",      // sidecar bootstrap / recovery primitive
];

/// T7: user-facing top-level subcommand names, as they appear in
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
];

#[derive(Parser)]
#[command(
    name = "tomlctl",
    version,
    about = "Read and write TOML files used by Claude Code flows and ledgers"
)]
pub(crate) struct Cli {
    /// T8: stderr error rendering format. `text` (default) is byte-identical
    /// to the pre-T8 `tomlctl: <anyhow chain>` line. `json` emits a single
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

/// T8: stderr-format selector surfaced via `--error-format`. `pub(crate)` so
/// `main.rs` can pattern-match on the variant before dispatching to `run()`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ErrorFormat {
    /// Default — byte-identical to pre-T8 output: `tomlctl: <anyhow chain>`.
    Text,
    /// Single compact JSON line with `error.kind` taxonomy.
    Json,
}

/// R74: read-only integrity options. Read paths honour only
/// `--verify-integrity` — the other three flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`) are write-side concepts
/// that would be silently no-ops on a read, so they're structurally kept
/// off read subcommands.
///
/// T9: `--strict-read` turns the "missing file → silent default" branches
/// (today only `items next-id --prefix <P>`) into a tagged `kind=not_found`
/// error. On every other read subcommand the flag is a benign no-op —
/// `io::read_toml` already surfaces `kind=not_found` on a missing file via
/// the T8 tagging, so passing `--strict-read` there changes nothing. The
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
    /// an empty default. Default behaviour is unchanged for every read path:
    /// `items list` / `items orphans` already error on a missing file today,
    /// but `items next-id --prefix R` returns `"R1"` as a bootstrapping fast
    /// path. Pass `--strict-read` when the caller needs to distinguish
    /// "no matches in an existing ledger" from "ledger does not exist".
    ///
    /// T9: fires BEFORE `--verify-integrity` — a missing file yields
    /// `kind=not_found`, not `kind=integrity`, even when both flags are set.
    #[arg(
        long = "strict-read",
        help = "Error on missing file instead of returning empty default (kind=not_found)"
    )]
    pub(crate) strict_read: bool,
}

/// R74 (and prior R60): write-side integrity/containment flags. Writers
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
    #[arg(long = "where", value_name = "KEY=VAL", help = "Filter: field equals value (repeatable)")]
    pub(crate) where_eq: Vec<String>,
    #[arg(long = "where-not", value_name = "KEY=VAL", help = "Filter: field does not equal value (repeatable)")]
    pub(crate) where_not: Vec<String>,
    #[arg(long = "where-in", value_name = "KEY=V1,V2,...", help = "Filter: field in comma-separated set (repeatable)")]
    pub(crate) where_in: Vec<String>,
    #[arg(long = "where-has", value_name = "KEY", help = "Filter: field is present (repeatable)")]
    pub(crate) where_has: Vec<String>,
    #[arg(long = "where-missing", value_name = "KEY", help = "Filter: field is absent (repeatable)")]
    pub(crate) where_missing: Vec<String>,
    #[arg(long = "where-gt", value_name = "KEY=VAL", help = "Filter: field > value (repeatable)")]
    pub(crate) where_gt: Vec<String>,
    #[arg(long = "where-gte", value_name = "KEY=VAL", help = "Filter: field >= value (repeatable)")]
    pub(crate) where_gte: Vec<String>,
    #[arg(long = "where-lt", value_name = "KEY=VAL", help = "Filter: field < value (repeatable)")]
    pub(crate) where_lt: Vec<String>,
    #[arg(long = "where-lte", value_name = "KEY=VAL", help = "Filter: field <= value (repeatable)")]
    pub(crate) where_lte: Vec<String>,
    #[arg(long = "where-contains", value_name = "KEY=SUB", help = "Filter: field string contains SUB (repeatable)")]
    pub(crate) where_contains: Vec<String>,
    #[arg(long = "where-prefix", value_name = "KEY=S", help = "Filter: field string starts with S (repeatable)")]
    pub(crate) where_prefix: Vec<String>,
    #[arg(long = "where-suffix", value_name = "KEY=S", help = "Filter: field string ends with S (repeatable)")]
    pub(crate) where_suffix: Vec<String>,
    #[arg(long = "where-regex", value_name = "KEY=PAT", help = "Filter: field string matches regex PAT (repeatable)")]
    pub(crate) where_regex: Vec<String>,
    #[arg(long = "select", value_name = "F1,F2,...", help = "Projection: keep only the listed fields")]
    pub(crate) select: Option<String>,
    #[arg(long = "exclude", value_name = "F1,F2,...", help = "Projection: drop the listed fields")]
    pub(crate) exclude: Option<String>,
    #[arg(long = "pluck", value_name = "FIELD", help = "Projection: return a flat [value, ...] array of FIELD")]
    pub(crate) pluck: Option<String>,
    #[arg(long = "sort-by", value_name = "FIELD[:asc|desc]", help = "Sort by FIELD (repeatable for tiebreakers)")]
    pub(crate) sort_by: Vec<String>,
    #[arg(long = "limit", value_name = "N", help = "Return at most N items")]
    pub(crate) limit: Option<usize>,
    #[arg(long = "offset", value_name = "N", help = "Skip the first N items")]
    pub(crate) offset: Option<usize>,
    #[arg(long = "distinct", help = "Dedup on the projected shape")]
    pub(crate) distinct: bool,
    #[arg(long = "group-by", value_name = "FIELD", help = "Aggregate: emit {value: [item, ...], ...}")]
    pub(crate) group_by: Option<String>,
    #[arg(long = "count-by", value_name = "FIELD", help = "Aggregate: emit {value: N, ...}")]
    pub(crate) count_by: Option<String>,
    /// T1: scalar-cardinality aggregate. Emits
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
    #[arg(long = "ndjson", help = "Output one JSON value per line (for piping into add-many/apply)")]
    pub(crate) ndjson: bool,
    /// T3: discoverable spelling of `--ndjson` for the `--pluck` case. A clap
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
    /// T2: bare-scalar output for single-value shapes. Composes as follows:
    ///
    /// - `--count --raw` / `--count-distinct --raw`: emit the bare integer
    ///   count (no `{"count":...}` / `{"count_distinct":...,"field":...}`
    ///   wrapping).
    /// - `--pluck f --raw` (N=1): emit the bare plucked value (strings
    ///   unquoted, numbers/bools bare).
    /// - `--pluck f --raw` (N != 1): errors with the exact load-bearing
    ///   message the task spec pins — tests assert byte-for-byte.
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
        /// T2: bare-scalar output. On a scalar target (string / integer /
        /// float / bool / date), emit the value unquoted — strings print
        /// literally, numbers bare, booleans as `true` / `false`. On a
        /// table or array target, error with the exact load-bearing message
        /// the task spec pins. The motivation is parity with `items list
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
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Set a JSON-encoded value (array, object, or scalar) at a dotted key path.
    SetJson {
        file: PathBuf,
        path: String,
        #[arg(long, help = "JSON-encoded value; pass `-` to read from stdin")]
        json: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Parse-check only. Exit 0 on valid TOML, non-zero otherwise.
    Validate {
        file: PathBuf,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Operations on [[items]] arrays-of-tables (ledger schema).
    Items {
        #[command(subcommand)]
        op: ItemsOp,
    },

    /// Verify byte-identical shared blocks across multiple markdown files.
    /// R60: deliberately does NOT take `--allow-outside` / `--verify-integrity`
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
        #[arg(long, conflicts_with = "ndjson", help = "JSON object for a single record; pass `-` to read from stdin")]
        json: Option<String>,
        #[arg(long = "ndjson", conflicts_with = "json", help = "NDJSON source: `-` for stdin, otherwise a file path")]
        ndjson: Option<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// T7: emit a JSON description of this binary's capabilities. Downstream
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
    /// R5: refresh is a pure content-digest primitive — it hashes the raw
    /// on-disk bytes and never parses TOML. A malformed file (e.g. one
    /// truncated by a partial write) will silently receive a valid
    /// sidecar. For the recovery path, consider running `tomlctl validate
    /// <path>` before `integrity refresh` so syntactic corruption surfaces
    /// instead of being papered over.
    ///
    /// R4: carries the full `WriteIntegrityArgs` bundle for parity with
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
    /// R76: `--count`, `--count-by`, `--group-by`, `--pluck` are mutually
    /// exclusive at the CLI layer via an `ArgGroup`. Previously the four
    /// silently collapsed via a priority ladder inside `build_query`
    /// (`count > count-by > group-by > pluck`); the group makes the
    /// mismatch visible at parse time with a clean clap error instead
    /// of producing unexpected output. `--ndjson` is a separate encoding
    /// flag (R82), so it stays out of the group.
    ///
    /// T1: `--count-distinct <FIELD>` joins the same group, so it's also
    /// parse-time mutex with every other shape flag (pairwise — e.g.
    /// `--count-distinct x --pluck y` errors at clap).
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
        /// R57: target array-of-tables name. Defaults to `items` (the ledger
        /// schema). Use e.g. `--array rollback_events` to list a non-default
        /// array of records.
        #[arg(long, default_value = "items")]
        array: String,

        // The full predicate/projection/shaping surface defined in the plan
        // lives on `QueryArgs` so the `next_help_heading = "Query options"`
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
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Append a new item. --json is the JSON object payload.
    Add {
        file: PathBuf,
        #[arg(long, help = "JSON object for the new item; pass `-` to read from stdin")]
        json: String,
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        /// T5: skip the add when an existing item already matches the
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
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Append many items in one batch from NDJSON. `--defaults-json` stamps
    /// common fields on every row (per-row keys win on conflict). One parse,
    /// one lock, one rewrite. On a malformed line N the batch aborts before
    /// mutating the file. Output: `{"ok":true,"added":N}`.
    AddMany {
        file: PathBuf,
        #[arg(long = "ndjson", help = "NDJSON source: `-` for stdin, otherwise a file path")]
        ndjson: String,
        #[arg(long = "defaults-json", help = "JSON object of default field values; pass `-` to read from stdin")]
        defaults_json: Option<String>,
        #[arg(long, default_value = "items")]
        array: String,
        /// T5: skip rows whose merged payload already matches an existing
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
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Merge fields into an existing item (matched by `id`). --json is a patch object.
    Update {
        file: PathBuf,
        id: String,
        #[arg(long, help = "JSON patch object merged into the item; pass `-` to read from stdin")]
        json: String,
        /// Remove a field from the matched item. Repeatable. Applied AFTER the
        /// `--json` patch, so an `--unset` trumps a same-key set from `--json`.
        /// A key that does not exist on the item is silently ignored.
        #[arg(long = "unset")]
        unset: Vec<String>,
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Remove an item by id. Fails if no such id exists.
    Remove {
        file: PathBuf,
        id: String,
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        /// T10: preview the removal without writing. Emits a
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
    /// R74: this is a read-only path (reads the ledger to find the max
    /// existing id, never writes), so it carries `ReadIntegrityArgs` — the
    /// write-side containment/sidecar flags have no semantic hook here and
    /// would be silently ignored if they were accepted.
    ///
    /// R40: neither `--prefix` nor `--infer-from-file` has a default. With
    /// four ledger schemas now in circulation (R review, O optimise, E
    /// execution-record, plus any future additions), a default of "R" would
    /// silently mis-mint for three of four callers. Every
    /// `tomlctl items next-id` invocation in this repo's
    /// `claude/commands/*.md` and `SKILL.md` already passes an explicit
    /// `--prefix R|O|E`, so structurally requiring one of the two flags is
    /// a no-op for well-formed callers and a fail-fast for careless ones.
    ///
    /// T4 (plan `docs/plans/tomlctl-capability-gaps.md`): `--infer-from-file`
    /// is the alternative path for callers handed an arbitrary `<ledger>`
    /// without knowing its prefix up front. It scans existing ids and
    /// returns `{prefix}{max_n+1}` when exactly one prefix is in use; on
    /// zero (empty ledger, no explicit prefix) or more than one it errors
    /// out rather than guessing. Structurally mutually exclusive with
    /// `--prefix` via `conflicts_with = "prefix"`; `--prefix` stays
    /// `required_unless_present = "infer_from_file"` so R40's "no silent
    /// default" contract is preserved (omitting both still fails at clap
    /// with the "required arguments were not provided" message).
    NextId {
        file: PathBuf,
        #[arg(
            long,
            required_unless_present = "infer_from_file",
            help = "Prefix letter (e.g. R, O, E) for the new id"
        )]
        prefix: Option<String>,
        /// T4: derive the prefix by scanning existing ids in the ledger.
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
        #[arg(long, help = "JSON array of ops, each `{\"op\":\"add|update|remove\", ...}`; pass `-` to read from stdin")]
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
        /// T10: preview the batch without writing. Runs every validation
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
    /// T6c: `--across <other>` runs the selected tier over the UNION of
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
        /// T6c: run cross-ledger — compare items from `<file>` against
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

    /// T11: explicit, auditable upgrade path for pre-Task-6 ledgers whose
    /// items lack `dedup_id`. Walks every item in the ledger, computes
    /// `tier_b_fingerprint` on any item missing the field, and writes the
    /// updated ledger atomically via the same compute/apply split as T10's
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
        /// T11: preview the backfill without writing. Emits
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
}

/// Legacy shortcut flags that predate the `--where-*` family on `items list`.
/// Kept on the CLI for back-compat (`--status`, `--category`, `--file`,
/// `--newer-than`) but translated into equivalent `Predicate` entries in
/// `Query::from_query_input` so the query engine only sees one predicate
/// list. R69: bundled into a small struct so the adapter can take
/// `(legacy, query)` rather than the prior 26-positional-arg signature.
pub(crate) struct LegacyShortcuts<'a> {
    pub(crate) status: &'a Option<String>,
    pub(crate) category: &'a Option<String>,
    pub(crate) file: &'a Option<String>,
    pub(crate) newer_than: &'a Option<String>,
    pub(crate) count: bool,
}
