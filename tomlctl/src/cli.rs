//! R63: Top-level CLI definition, argument bundle structs, and `run()`
//! dispatch — extracted from `main.rs` so the binary entrypoint can shrink
//! to a one-line wrapper. Every `Cmd` / `ItemsOp` / `BlocksOp` arm is
//! matched here and delegated to `items::` / `blocks::` / `io::` helpers
//! that own the underlying behaviour. Pure plumbing; no business logic.

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::Value as JsonValue;
use std::io::{BufRead, BufWriter, IsTerminal, Read, Write};
use std::path::PathBuf;

use crate::blocks::blocks_verify;
use crate::convert::{
    ScalarType, detable_to_json, maybe_date_coerce, navigate, parse_scalar, set_at_path,
    toml_to_json,
};
use crate::dedup::{DupTier, items_find_duplicates};
use crate::integrity::IntegrityOpts;
use crate::io::{mutate_doc, read_doc, read_doc_borrowed, read_toml_str};
use crate::items::{
    array_append, items_add_many, items_add_to, items_apply_to_opts, items_get_from,
    items_next_id, items_remove_from, items_update_to, parse_ndjson,
};
use crate::orphans::items_orphans;
use crate::query::{self, OutputShape, Predicate, Query, SortDir};


/// Maximum JSON payload accepted from stdin via the `-` sentinel. 32 MiB is
/// well above any realistic review-ledger / flow-context apply-ops payload
/// (typical is < 64 KiB) while being small enough to fail fast if a caller
/// accidentally pipes a log or a binary into `--json -`.
const MAX_STDIN_BYTES: u64 = 32 * 1024 * 1024;

/// R32: guard against multiple `-` sentinels consuming stdin in a single
/// invocation (e.g. `--json - --ops -`). The second `read_json_arg("-")` call
/// errors out instead of silently returning an empty string (stdin already at
/// EOF) and corrupting the apply.
static STDIN_CONSUMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Resolve an NDJSON source argument. A literal dash reads stdin via
/// `read_json_arg` (preserving the STDIN_CONSUMED guard against a second
/// `-` sentinel on the same invocation); any other value is a file path
/// read verbatim with `fs::read_to_string`. Extracted (R84) so the
/// identical resolution logic doesn't live in both `Cmd::ArrayAppend` and
/// `ItemsOp::AddMany`.
fn read_ndjson_source(src: &str) -> Result<String> {
    if src == "-" {
        read_json_arg("-")
    } else {
        std::fs::read_to_string(src)
            .with_context(|| format!("reading NDJSON file `{}`", src))
    }
}

/// Resolve a JSON argument: if it's literally "-", read stdin to a String.
/// Otherwise return the argument as-is.
///
/// Stdin handling (R7):
///
/// - Refuses to block on an interactive TTY — a user piping nothing into
///   `--json -` would otherwise hang forever with no prompt or feedback.
/// - Caps the read at `MAX_STDIN_BYTES`; an oversize payload is truncated
///   on the input side so a misrouted log doesn't balloon tomlctl's heap.
fn read_json_arg(arg: &str) -> Result<String> {
    if arg == "-" {
        // R32: a single invocation can only consume stdin once. A second `-`
        // sentinel would read an already-drained handle and silently return an
        // empty payload, which downstream would treat as a no-op or a parse
        // error with a confusing message. `swap(true, SeqCst)` is both the
        // check and the mark, so concurrent calls can't both see `false`.
        if STDIN_CONSUMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            bail!(
                "stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call"
            );
        }
        if std::io::stdin().is_terminal() {
            bail!(
                "stdin is a TTY — pipe JSON (e.g. `cat payload.json | tomlctl … --json -`) or pass `--json '<literal>'`"
            );
        }
        let mut buf = String::new();
        std::io::stdin()
            .lock()
            .take(MAX_STDIN_BYTES)
            .read_to_string(&mut buf)
            .context("reading JSON from stdin")?;
        if buf.trim().is_empty() {
            bail!("stdin was empty — expected JSON payload");
        }
        Ok(buf)
    } else {
        Ok(arg.to_string())
    }
}

/// O35: parse a JSON `--json`/`--ops`/`--defaults-json` argument directly
/// into a `JsonValue`, skipping the intermediate `String` allocation that
/// the `read_json_arg` + `serde_json::from_str(&s)` two-step would incur.
///
/// Mirrors `read_json_arg`'s stdin discipline exactly:
///
/// - Honours STDIN_CONSUMED (R32): a second `-` sentinel on the same
///   invocation bails with the identical "already consumed" message.
/// - Refuses to block on a TTY (R7) with the identical guidance message.
/// - Caps the read at `MAX_STDIN_BYTES` via the same `take(...)` wrapper.
/// - Reports the same "stdin was empty — expected JSON payload" error when
///   stdin closes immediately, rather than letting serde surface its own
///   EOF message (which would silently change the public-facing error
///   text).
///
/// Callers add their own per-flag `.with_context("parsing --json"|"parsing
/// --ops"|"parsing --defaults-json")` so the user-visible error chain stays
/// byte-identical to the pre-O35 behaviour where each call site wrapped
/// `serde_json::from_str(&text).context("parsing --<flag>")`.
fn read_json_value_from_arg(arg: &str) -> Result<JsonValue> {
    if arg == "-" {
        // R32: identical swap-and-mark check as `read_json_arg`.
        if STDIN_CONSUMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            bail!(
                "stdin already consumed by another flag on this invocation; only one --json/--ops/--ndjson/--defaults-json flag can use the `-` sentinel per call"
            );
        }
        if std::io::stdin().is_terminal() {
            bail!(
                "stdin is a TTY — pipe JSON (e.g. `cat payload.json | tomlctl … --json -`) or pass `--json '<literal>'`"
            );
        }
        let stdin = std::io::stdin();
        let lock = stdin.lock();
        let mut r = std::io::BufReader::new(lock.take(MAX_STDIN_BYTES));
        // Preserve the "stdin was empty" sentinel: peek the first buffered
        // chunk; if it never arrives, stdin closed before sending anything
        // and we want our own message rather than serde's EOF wording.
        let initial = r.fill_buf().context("reading JSON from stdin")?;
        if initial.is_empty() {
            bail!("stdin was empty — expected JSON payload");
        }
        // `from_reader` consumes the BufReader's internal buffer before
        // refilling from the underlying `Take<StdinLock>`, so the peek
        // above does not strand any bytes.
        Ok(serde_json::from_reader(r)?)
    } else {
        Ok(serde_json::from_str(arg)?)
    }
}

#[derive(Parser)]
#[command(
    name = "tomlctl",
    version,
    about = "Read and write TOML files used by Claude Code flows and ledgers"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// R74: read-only integrity options. Read paths honour only
/// `--verify-integrity` — the other three flags (`--allow-outside`,
/// `--no-write-integrity`, `--strict-integrity`) are write-side concepts
/// that would be silently no-ops on a read, so they're structurally kept
/// off read subcommands.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
struct ReadIntegrityArgs {
    /// Before any read operation, verify the target file against its
    /// `<file>.sha256` sidecar. Errors if the sidecar is missing or the
    /// digest disagrees. Never auto-repairs.
    #[arg(long = "verify-integrity")]
    verify_integrity: bool,
}

/// R74 (and prior R60): write-side integrity/containment flags. Writers
/// still honour `--verify-integrity` because an update is often preceded
/// by a pre-read verify; the other three flags only have a semantic hook
/// on write paths.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
struct WriteIntegrityArgs {
    /// Allow write operations on files outside the current repo's `.claude/` directory.
    /// By default, writes are refused if the canonical target path is not under
    /// `<git-top-level>/.claude/` (or the CWD, if not in a git repo). Use this to
    /// intentionally edit a flow file in another location. Affects only TOML
    /// write paths (set / set-json / items *).
    #[arg(long = "allow-outside")]
    allow_outside: bool,

    /// Suppress writing the `<file>.sha256` integrity sidecar. Default behaviour
    /// is to write a sidecar alongside every TOML write (standard `sha256sum`
    /// format: `<hex>  <basename>\n`). Pass this flag to opt out, e.g. when the
    /// target filesystem does not tolerate an extra sidecar file.
    #[arg(long = "no-write-integrity")]
    no_write_integrity: bool,

    /// Before any read operation, verify the target file against its
    /// `<file>.sha256` sidecar. Errors if the sidecar is missing or the
    /// digest disagrees. Never auto-repairs.
    #[arg(long = "verify-integrity")]
    verify_integrity: bool,

    /// Treat an integrity-sidecar write failure as a hard error instead of a
    /// stderr warning. Off by default — the primary data is already durable
    /// on disk by the time the sidecar is attempted, so a failed sidecar is
    /// usually recoverable by re-running the write. Pass this flag on a
    /// tight-integrity path (e.g. signed-artifact builds) where a missing or
    /// stale sidecar must fail CI.
    #[arg(long = "strict-integrity")]
    strict_integrity: bool,
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
struct QueryArgs {
    #[arg(long = "where", value_name = "KEY=VAL", help = "Filter: field equals value (repeatable)")]
    where_eq: Vec<String>,
    #[arg(long = "where-not", value_name = "KEY=VAL", help = "Filter: field does not equal value (repeatable)")]
    where_not: Vec<String>,
    #[arg(long = "where-in", value_name = "KEY=V1,V2,...", help = "Filter: field in comma-separated set (repeatable)")]
    where_in: Vec<String>,
    #[arg(long = "where-has", value_name = "KEY", help = "Filter: field is present (repeatable)")]
    where_has: Vec<String>,
    #[arg(long = "where-missing", value_name = "KEY", help = "Filter: field is absent (repeatable)")]
    where_missing: Vec<String>,
    #[arg(long = "where-gt", value_name = "KEY=VAL", help = "Filter: field > value (repeatable)")]
    where_gt: Vec<String>,
    #[arg(long = "where-gte", value_name = "KEY=VAL", help = "Filter: field >= value (repeatable)")]
    where_gte: Vec<String>,
    #[arg(long = "where-lt", value_name = "KEY=VAL", help = "Filter: field < value (repeatable)")]
    where_lt: Vec<String>,
    #[arg(long = "where-lte", value_name = "KEY=VAL", help = "Filter: field <= value (repeatable)")]
    where_lte: Vec<String>,
    #[arg(long = "where-contains", value_name = "KEY=SUB", help = "Filter: field string contains SUB (repeatable)")]
    where_contains: Vec<String>,
    #[arg(long = "where-prefix", value_name = "KEY=S", help = "Filter: field string starts with S (repeatable)")]
    where_prefix: Vec<String>,
    #[arg(long = "where-suffix", value_name = "KEY=S", help = "Filter: field string ends with S (repeatable)")]
    where_suffix: Vec<String>,
    #[arg(long = "where-regex", value_name = "KEY=PAT", help = "Filter: field string matches regex PAT (repeatable)")]
    where_regex: Vec<String>,
    #[arg(long = "select", value_name = "F1,F2,...", help = "Projection: keep only the listed fields")]
    select: Option<String>,
    #[arg(long = "exclude", value_name = "F1,F2,...", help = "Projection: drop the listed fields")]
    exclude: Option<String>,
    #[arg(long = "pluck", value_name = "FIELD", help = "Projection: return a flat [value, ...] array of FIELD")]
    pluck: Option<String>,
    #[arg(long = "sort-by", value_name = "FIELD[:asc|desc]", help = "Sort by FIELD (repeatable for tiebreakers)")]
    sort_by: Vec<String>,
    #[arg(long = "limit", value_name = "N", help = "Return at most N items")]
    limit: Option<usize>,
    #[arg(long = "offset", value_name = "N", help = "Skip the first N items")]
    offset: Option<usize>,
    #[arg(long = "distinct", help = "Dedup on the projected shape")]
    distinct: bool,
    #[arg(long = "group-by", value_name = "FIELD", help = "Aggregate: emit {value: [item, ...], ...}")]
    group_by: Option<String>,
    #[arg(long = "count-by", value_name = "FIELD", help = "Aggregate: emit {value: N, ...}")]
    count_by: Option<String>,
    #[arg(long = "ndjson", help = "Output one JSON value per line (for piping into add-many/apply)")]
    ndjson: bool,
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
enum Cmd {
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
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum ItemsOp {
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
    #[command(group(clap::ArgGroup::new("shape").multiple(false).args(["count", "count_by", "group_by", "pluck"])))]
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
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Print the next id string for the given prefix (default R).
    /// R74: this is a read-only path (reads the ledger to find the max
    /// existing id, never writes), so it carries `ReadIntegrityArgs` — the
    /// write-side containment/sidecar flags have no semantic hook here and
    /// would be silently ignored if they were accepted.
    NextId {
        file: PathBuf,
        #[arg(long, default_value = "R")]
        prefix: String,
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
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Find duplicate items using one of the dedup tiers.
    FindDuplicates {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = DupTier::A)]
        tier: DupTier,
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
}

#[derive(Subcommand)]
enum BlocksOp {
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

/// Translate the flattened integrity-args structs from a subcommand variant
/// into the module-local `IntegrityOpts` bundle. Kept next to the CLI
/// definition (rather than in `integrity.rs`) so the integrity module stays
/// free of the clap-derived types. R74 split `IntegrityArgs` in two — read
/// paths hand us `ReadIntegrityArgs` (only `verify_integrity` matters), write
/// paths hand us `WriteIntegrityArgs` (the full set). Both flow through the
/// same `IntegrityOpts` so every downstream consumer
/// (`maybe_verify_integrity` / `write_toml_with_sidecar`) stays unchanged.
fn read_integrity_opts(args: &ReadIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        // Read-side paths never write a sidecar; default to true so that if
        // a future refactor funnels the same opts into a writer we don't
        // accidentally suppress the sidecar. `write_toml_with_sidecar` is
        // only reached on write paths, which use `write_integrity_opts`.
        write_sidecar: true,
        verify_on_read: args.verify_integrity,
        // Read paths never hit the sidecar-write failure branch, so `strict`
        // has no effect here. Pin it `false` so the opt's semantics stay
        // predictable if the struct is ever inspected after the read.
        strict: false,
    }
}

fn write_integrity_opts(args: &WriteIntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: !args.no_write_integrity,
        verify_on_read: args.verify_integrity,
        strict: args.strict_integrity,
    }
}

/// Top-level dispatch entrypoint. `main.rs` is a one-line wrapper over
/// this; splitting lets the binary target stay trivially small while all
/// the parsing/dispatch/output plumbing lives in a normal module.
pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Parse { file, integrity } => {
            let opts = read_integrity_opts(&integrity);
            // O10: `parse` is the single dispatch arm whose whole output is
            // "the entire TOML doc as JSON" — no dotted-path navigation, no
            // per-item filtering — so it benefits most from the borrowed
            // DeTable fast-path that skips the per-scalar `String` clone
            // done inside `toml::from_str::<TomlValue>`. When
            // `--verify-integrity` is requested we still need the shared
            // lock + sidecar verify dance from `read_doc`, so the owned
            // path is retained for that case. All other read dispatch arms
            // (`get`, `validate`, every `items *` op) stay on the owned
            // path — they either need `navigate` / TomlValue-level helpers
            // or the borrowed-lifetime plumbing doesn't yet cover their
            // downstream consumers.
            let out = if opts.verify_on_read {
                read_doc(&file, opts, |doc| Ok(toml_to_json(doc)))?
            } else {
                let source = read_toml_str(&file)?;
                read_doc_borrowed(&source, |table| Ok(detable_to_json(table)))?
            };
            print_json(&out)?;
        }
        Cmd::Get { file, path, integrity } => {
            let opts = read_integrity_opts(&integrity);
            let out = read_doc(&file, opts, |doc| {
                Ok(match path.as_deref() {
                    None | Some("") => toml_to_json(doc),
                    Some(p) => toml_to_json(
                        navigate(doc, p).ok_or_else(|| anyhow!("key path `{}` not found", p))?,
                    ),
                })
            })?;
            print_json(&out)?;
        }
        Cmd::Set {
            file,
            path,
            value,
            ty,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let v = parse_scalar(&value, ty)?;
                set_at_path(doc, &path, v)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::SetJson { file, path, json, integrity } => {
            let opts = write_integrity_opts(&integrity);
            // O35: parse stdin/literal JSON straight into a `JsonValue`,
            // skipping the intermediate String allocation. The parse moves
            // out of the `mutate_doc` closure, which is a side-benefit:
            // a malformed payload now fails before we open the doc.
            let parsed: JsonValue = read_json_value_from_arg(&json).context("parsing --json")?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let last_key = path.rsplit_once('.').map(|(_, k)| k).unwrap_or(path.as_str());
                let v = maybe_date_coerce(last_key, &parsed)?;
                set_at_path(doc, &path, v)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::Validate { file, integrity } => {
            let opts = read_integrity_opts(&integrity);
            read_doc(&file, opts, |_doc| Ok(()))?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        Cmd::Items { op } => items_dispatch(op)?,
        Cmd::Blocks { op } => blocks_dispatch(op)?,
        Cmd::ArrayAppend {
            file,
            array,
            json,
            ndjson,
            integrity,
        } => {
            // clap's `conflicts_with` guarantees at most one is set; enforce
            // "at least one" here since clap has no first-class
            // required-exactly-one primitive on optional flags.
            if json.is_none() && ndjson.is_none() {
                bail!("array-append requires one of --json or --ndjson");
            }
            let opts = write_integrity_opts(&integrity);
            let rows: Vec<JsonValue> = if let Some(j) = json {
                // O35: parse straight to `JsonValue`, dropping the prior
                // `read_json_arg` String + `serde_json::from_str` two-step.
                let parsed: JsonValue =
                    read_json_value_from_arg(&j).context("parsing --json")?;
                if !parsed.is_object() {
                    bail!("--json must be a JSON object");
                }
                vec![parsed]
            } else {
                let nd = ndjson.expect("checked above");
                let text = read_ndjson_source(&nd)?;
                parse_ndjson(&text)?
            };
            let mut appended: usize = 0;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                appended = array_append(doc, &array, &rows)?;
                Ok(())
            })?;
            print_json_compact(&serde_json::json!({"ok": true, "appended": appended}))?;
        }
    }
    Ok(())
}

fn items_dispatch(op: ItemsOp) -> Result<()> {
    match op {
        ItemsOp::List {
            file,
            status,
            category,
            newer_than,
            file_filter,
            count,
            array,
            query,
            integrity,
        } => {
            let opts = read_integrity_opts(&integrity);
            let legacy = LegacyShortcuts {
                status: &status,
                category: &category,
                file: &file_filter,
                newer_than: &newer_than,
                count,
            };
            let q = build_query(&legacy, &query)?;
            // R82: `ndjson` is an output-encoding choice, not a shape. Only
            // the Array shape + ndjson encoding combination is meaningful;
            // `validate_query` (called inside `run`) rejects other combos.
            if q.ndjson && matches!(q.shape, OutputShape::Array) {
                // O34: stream one compact JSON value per line directly via
                // `query::run_streaming`, avoiding the `Vec<JsonValue>` that
                // `query::run` would otherwise materialise only for us to
                // iterate and re-serialise. The streaming path walks the
                // same pipeline and emits per-item — peak memory scales with
                // the filtered set, not the full output array.
                let stdout = std::io::stdout();
                let mut h = stdout.lock();
                read_doc(&file, opts, |doc| query::run_streaming(doc, &array, &q, &mut h))?;
                h.flush()?;
            } else {
                let out = read_doc(&file, opts, |doc| query::run(doc, &array, &q))?;
                print_json(&out)?;
            }
        }
        ItemsOp::Get { file, id, array, integrity } => {
            let opts = read_integrity_opts(&integrity);
            let out = read_doc(&file, opts, |doc| items_get_from(doc, &array, &id))?;
            print_json(&out)?;
        }
        ItemsOp::Add { file, json, array, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_add_to(doc, &array, &json)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        ItemsOp::AddMany {
            file,
            ndjson,
            defaults_json,
            array,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            // NDJSON source resolution factored into `read_ndjson_source` (R84);
            // the STDIN_CONSUMED guard in `read_json_arg` still refuses a second
            // `-` when `--defaults-json -` also wants stdin on the same call.
            let ndjson_text = read_ndjson_source(&ndjson)?;
            let rows = parse_ndjson(&ndjson_text)?;
            let defaults: Option<JsonValue> = match defaults_json.as_deref() {
                // O35: parse straight to `JsonValue`, dropping the prior
                // `read_json_arg` String + `serde_json::from_str` two-step.
                Some(s) => Some(
                    read_json_value_from_arg(s).context("parsing --defaults-json")?,
                ),
                None => None,
            };
            let mut added: usize = 0;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                Ok(())
            })?;
            print_json_compact(&serde_json::json!({"ok": true, "added": added}))?;
        }
        ItemsOp::Update {
            file,
            id,
            json,
            unset,
            array,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_update_to(doc, &array, &id, &json, &unset)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        ItemsOp::Remove { file, id, array, integrity } => {
            let opts = write_integrity_opts(&integrity);
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_remove_from(doc, &array, &id)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        ItemsOp::Apply { file, ops, array, no_remove, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let ops = read_json_arg(&ops)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_apply_to_opts(doc, &ops, &array, no_remove)
            })?;
            print_json_compact(&serde_json::json!({"ok": true}))?;
        }
        ItemsOp::NextId { file, prefix, integrity } => {
            // R19: if the target ledger doesn't exist yet, there's nothing to
            // parse or verify — the "next" id is trivially `<prefix>1`. This
            // lets flows call `items next-id` before the ledger is initialised
            // (e.g. during bootstrap of a new flow directory).
            if !file.exists() {
                if prefix.is_empty() {
                    bail!("prefix must not be empty — use a letter like R, O, or A");
                }
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    bail!("prefix must not be all-digit — would collide with numeric-suffix parsing");
                }
                println!("{}", serde_json::to_string(&format!("{}1", prefix))?);
            } else {
                let opts = read_integrity_opts(&integrity);
                let id = read_doc(&file, opts, |doc| items_next_id(doc, &prefix))?;
                println!("{}", serde_json::to_string(&id)?);
            }
        }
        ItemsOp::FindDuplicates { file, tier, integrity } => {
            let opts = read_integrity_opts(&integrity);
            let groups = read_doc(&file, opts, |doc| items_find_duplicates(doc, tier))?;
            print_json(&JsonValue::Array(groups))?;
        }
        ItemsOp::Orphans { file, integrity } => {
            let opts = read_integrity_opts(&integrity);
            let orphans = read_doc(&file, opts, items_orphans)?;
            print_json(&JsonValue::Array(orphans))?;
        }
    }
    Ok(())
}

/// Split a `KEY=VAL` string on the first `=`. Empty keys are rejected. The
/// value is returned verbatim (no trimming) so callers that care about
/// whitespace-significant RHS values (e.g. `--where-prefix name= foo`) keep
/// their payload intact. Used by `build_query` for every `--where-*` family.
fn split_kv(s: &str) -> Result<(String, String)> {
    let Some((k, v)) = s.split_once('=') else {
        bail!("expected KEY=VAL, got `{}`", s);
    };
    if k.is_empty() {
        bail!("KEY=VAL has empty key in `{}`", s);
    }
    Ok((k.to_string(), v.to_string()))
}

/// Legacy shortcut flags that predate the `--where-*` family on `items list`.
/// Kept on the CLI for back-compat (`--status`, `--category`, `--file`,
/// `--newer-than`) but translated into equivalent `Predicate` entries in
/// `build_query` so the query engine only sees one predicate list. R69:
/// bundled into a small struct so `build_query` can take `(legacy, query)`
/// rather than the prior 26-positional-arg signature.
struct LegacyShortcuts<'a> {
    status: &'a Option<String>,
    category: &'a Option<String>,
    file: &'a Option<String>,
    newer_than: &'a Option<String>,
    count: bool,
}

/// Build a `query::Query` from the clap flag values on `ItemsOp::List`.
/// Validation is handled by `query::run` itself — the first thing it does
/// is call `validate_query` on the spec, so callers don't need to (R88).
/// R69: signature collapsed to two references (a `LegacyShortcuts` for the
/// back-compat shortcut flags + the full `QueryArgs` bundle) so the dispatch
/// site is a one-line call rather than a 26-line arg spray.
fn build_query(legacy: &LegacyShortcuts<'_>, q: &QueryArgs) -> Result<Query> {
    // O46: pre-size the predicate vec. The `4` covers the four legacy shortcut
    // slots (`status`, `category`, `file`, `newer_than`); the remaining terms
    // sum the upper bound for every `--where-*` family. Slight over-allocation
    // when legacy shortcuts are absent is fine; this avoids the 4+ realloc-
    // grow cycles of pushing into an empty `Vec::new()` on busy list calls.
    let mut predicates: Vec<Predicate> = Vec::with_capacity(
        4 + q.where_eq.len()
            + q.where_not.len()
            + q.where_in.len()
            + q.where_has.len()
            + q.where_missing.len()
            + q.where_gt.len()
            + q.where_gte.len()
            + q.where_lt.len()
            + q.where_lte.len()
            + q.where_contains.len()
            + q.where_prefix.len()
            + q.where_suffix.len()
            + q.where_regex.len(),
    );

    // Legacy shortcut flags — map onto the new predicate surface so the
    // query engine has a single filter list to evaluate. Duplicating a
    // legacy flag with an equivalent `--where` is a no-op (same predicate
    // runs twice; same result).
    if let Some(v) = legacy.status {
        predicates.push(Predicate::Where {
            key: "status".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = legacy.category {
        predicates.push(Predicate::Where {
            key: "category".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = legacy.file {
        predicates.push(Predicate::Where {
            key: "file".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = legacy.newer_than {
        // `--newer-than` semantically means "first_flagged > v" where v is
        // a YYYY-MM-DD. The `@date:` prefix tells `parse_typed_value` to
        // coerce the RHS to a TOML date rather than comparing as a string.
        predicates.push(Predicate::WhereGt {
            key: "first_flagged".into(),
            rhs: format!("@date:{}", v),
        });
    }

    for s in &q.where_eq {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::Where { key, rhs });
    }
    for s in &q.where_not {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereNot { key, rhs });
    }
    for s in &q.where_in {
        let (key, rhs) = split_kv(s)?;
        let values: Vec<String> = rhs.split(',').map(|s| s.to_string()).collect();
        predicates.push(Predicate::WhereIn { key, rhs: values });
    }
    for s in &q.where_has {
        if s.is_empty() {
            bail!("--where-has expects a KEY, got empty string");
        }
        predicates.push(Predicate::WhereHas { key: s.clone() });
    }
    for s in &q.where_missing {
        if s.is_empty() {
            bail!("--where-missing expects a KEY, got empty string");
        }
        predicates.push(Predicate::WhereMissing { key: s.clone() });
    }
    for s in &q.where_gt {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereGt { key, rhs });
    }
    for s in &q.where_gte {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereGte { key, rhs });
    }
    for s in &q.where_lt {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereLt { key, rhs });
    }
    for s in &q.where_lte {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereLte { key, rhs });
    }
    for s in &q.where_contains {
        let (key, sub) = split_kv(s)?;
        predicates.push(Predicate::WhereContains { key, sub });
    }
    for s in &q.where_prefix {
        let (key, prefix) = split_kv(s)?;
        predicates.push(Predicate::WherePrefix { key, prefix });
    }
    for s in &q.where_suffix {
        let (key, suffix) = split_kv(s)?;
        predicates.push(Predicate::WhereSuffix { key, suffix });
    }
    for s in &q.where_regex {
        let (key, pattern) = split_kv(s)?;
        predicates.push(Predicate::WhereRegex { key, pattern });
    }

    // Projection: parse `--select a,b` / `--exclude a,b` into Vec<String>.
    // `validate_query` enforces `select` / `exclude` / `pluck` mutual
    // exclusion; we just populate the struct.
    let select_fields: Option<Vec<String>> = q
        .select
        .as_deref()
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
    let exclude_fields: Option<Vec<String>> = q
        .exclude
        .as_deref()
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

    // Sort: each entry is `FIELD` or `FIELD:asc` or `FIELD:desc`. Unknown
    // suffix defaults to `asc` (matches the plan).
    let mut sort_list: Vec<(String, SortDir)> = Vec::new();
    for entry in &q.sort_by {
        let (field, dir) = match entry.split_once(':') {
            Some((f, d)) => {
                let dir = match d {
                    "desc" => SortDir::Desc,
                    _ => SortDir::Asc,
                };
                (f.to_string(), dir)
            }
            None => (entry.clone(), SortDir::Asc),
        };
        sort_list.push((field, dir));
    }

    // OutputShape priority (plan): count > count-by > group-by > pluck >
    // default Array. `ndjson` is an *encoding* choice (R82), not a shape —
    // it lives on `Query.ndjson` and only applies when the chosen shape is
    // Array. Multiple shape flags would typically collapse to the
    // highest-priority one here; `validate_query` (inside `query::run`)
    // then rejects any shape-vs-projection conflict with a clear error.
    let shape = if legacy.count {
        OutputShape::Count
    } else if let Some(f) = q.count_by.as_deref() {
        OutputShape::CountBy(f.to_string())
    } else if let Some(f) = q.group_by.as_deref() {
        OutputShape::GroupBy(f.to_string())
    } else if let Some(f) = q.pluck.as_deref() {
        OutputShape::Pluck(f.to_string())
    } else {
        OutputShape::Array
    };

    Ok(Query {
        predicates,
        select: select_fields,
        exclude: exclude_fields,
        sort_by: sort_list,
        limit: q.limit,
        offset: q.offset,
        distinct: q.distinct,
        shape,
        ndjson: q.ndjson,
    })
}

fn blocks_dispatch(op: BlocksOp) -> Result<()> {
    match op {
        BlocksOp::Verify { files, block } => {
            let report = blocks_verify(&files, &block)?;
            print_json(&report.report)?;
            if !report.ok {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn print_json(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer_pretty(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

/// Compact single-line sibling of `print_json`, used for the `{"ok":true,...}`
/// terminal status lines emitted by write-path dispatch arms (R83). Keeping
/// this separate from `print_json` preserves the pretty-printed contract that
/// downstream consumers rely on for read-path output (tests + humans) while
/// letting every OK-status emitter funnel through a single helper rather
/// than hand-constructing JSON strings. Compact form also matches the
/// pre-refactor byte-for-byte output so integration tests that do a
/// `.contains(r#"{"ok":true,"added":N}"#)` continue to pass.
fn print_json_compact(v: &JsonValue) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    serde_json::to_writer(&mut out, v)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{self, scan_block_names_warn};
    use std::fs;
    use std::path::Path;

    // ----- blocks verify ---------------------------------------------------

    #[test]
    fn blocks_verify_detects_drift() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        let good = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
line two
<!-- SHARED-BLOCK:flow-context END -->
";
        fs::write(&a, good).unwrap();
        fs::write(&b, good).unwrap();
        let report =
            blocks_verify(&[a.clone(), b.clone()], &["flow-context".to_string()]).unwrap();
        assert!(report.ok, "equal content must be ok");

        let drifted = "\
<!-- SHARED-BLOCK:flow-context START -->
line one
DIFFERENT
<!-- SHARED-BLOCK:flow-context END -->
";
        fs::write(&b, drifted).unwrap();
        let report = blocks_verify(&[a, b], &["flow-context".to_string()]).unwrap();
        assert!(!report.ok);
        // drift entries carry per-file hash detail
        let blocks = report.report.get("blocks").and_then(|v| v.as_array()).unwrap();
        assert_eq!(blocks.len(), 1);
        let drift_arr = blocks[0].get("drift").and_then(|v| v.as_array()).unwrap();
        assert_eq!(drift_arr.len(), 2);
        let h0 = drift_arr[0].get("hash").and_then(|v| v.as_str()).unwrap();
        let h1 = drift_arr[1].get("hash").and_then(|v| v.as_str()).unwrap();
        assert_ne!(h0, h1, "drift implies distinct hashes");
    }

    #[test]
    fn blocks_verify_missing_marker_reports_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        let good = "\
<!-- SHARED-BLOCK:x START -->
body
<!-- SHARED-BLOCK:x END -->
";
        fs::write(&a, good).unwrap();
        fs::write(&b, "nothing here\n").unwrap();
        let report = blocks_verify(&[a, b], &["x".to_string()]).unwrap();
        assert!(!report.ok);
        let blocks = report.report.get("blocks").and_then(|v| v.as_array()).unwrap();
        let missing = blocks[0].get("missing").and_then(|v| v.as_array()).unwrap();
        assert_eq!(missing.len(), 1);
    }

    #[test]
    fn scan_block_names_warn_emits_for_typo_but_keeps_canonical() {
        // R53: a line that looks like a SHARED-BLOCK marker but is malformed
        // (missing hyphen, wrong case, trailing whitespace) must NOT be
        // picked up as a block name, AND must NOT break verification — it's
        // only advisory. We can't easily capture stderr from within a unit
        // test without invasive plumbing; assert on the behavioural
        // guarantees instead: canonical names are still discovered and the
        // typo isn't silently treated as a block.
        let contents = "\
<!-- SHAREDBLOCK:typo START -->
should-be-ignored
<!-- SHAREDBLOCK:typo END -->
<!-- SHARED-BLOCK:real START -->
body
<!-- SHARED-BLOCK:real END -->
";
        let names = scan_block_names_warn(contents, Some("synthetic-fixture"));
        assert_eq!(names, vec!["real".to_string()]);

        // A full verify over two files, each with a typo line, still passes
        // for the canonical block.
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, contents).unwrap();
        fs::write(&b, contents).unwrap();
        let report = blocks_verify(&[a, b], &["real".to_string()]).unwrap();
        assert!(
            report.ok,
            "typo lines must not break verification: {:?}",
            report.report
        );
    }

    #[test]
    fn blocks_verify_reproduces_shell_hashes() {
        // R87: pin hashes for every block enumerated in
        // `scripts/shared-blocks.toml`, not just the two flow-wide ones. The
        // three apply-only blocks (apply-dependency-sort,
        // apply-rollback-protocol, apply-constraints) live in 2 files each
        // (optimise-apply + review-apply), so they're verified over the
        // 2-file subset rather than the full 4. Splitting the assertions
        // this way lets a drift in any one of the five blocks surface
        // independently with a named hash, instead of a confusing "missing"
        // report from running a 2-file block over 4 files.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
        let cmd_dir = repo_root.join("claude").join("commands");
        let all_four = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];
        let apply_pair = [
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];

        // Only run when every file is present. The test crate is consumable
        // in isolation; degrade gracefully if someone packages it without
        // the command tree.
        if !all_four.iter().all(|p| p.exists()) {
            eprintln!(
                "blocks_verify_reproduces_shell_hashes: command files not found, skipping"
            );
            return;
        }

        let expect_hash = |report: &blocks::BlocksReport, name: &str, expected: &str| {
            let blocks_arr = report
                .report
                .get("blocks")
                .and_then(|v| v.as_array())
                .expect("blocks array");
            let block = blocks_arr
                .iter()
                .find(|b| b.get("name").and_then(|v| v.as_str()) == Some(name))
                .unwrap_or_else(|| panic!("block `{name}` missing from report: {:?}", report.report));
            let hash = block
                .get("hash")
                .and_then(|v| v.as_str())
                .unwrap_or_else(|| panic!("block `{name}` has no `hash` field (drift?): {:?}", block));
            assert_eq!(hash, expected, "block `{name}` hash drift");
        };

        // --- 4-file blocks ---
        let report = blocks_verify(
            &all_four,
            &[
                "flow-context".to_string(),
                "ledger-schema".to_string(),
            ],
        )
        .unwrap();
        assert!(report.ok, "flow-wide blocks must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "flow-context",
            "efd5619a706fcc012f2c1741cea7318b210e155048625ca04be7e09401f274f2",
        );
        expect_hash(
            &report,
            "ledger-schema",
            "23df0a7893ea44e356979328ef62592edd6493c2d1d34e0520e59958129ca14c",
        );

        // --- 2-file apply-only blocks ---
        let report = blocks_verify(
            &apply_pair,
            &[
                "apply-dependency-sort".to_string(),
                "apply-rollback-protocol".to_string(),
                "apply-constraints".to_string(),
            ],
        )
        .unwrap();
        assert!(
            report.ok,
            "apply-only blocks must be parity across the 2-file subset: {:?}",
            report.report
        );
        expect_hash(
            &report,
            "apply-dependency-sort",
            "482172a20b6f88eef4abf4af93464e8b80825d7719672e390d1d785230be846a",
        );
        expect_hash(
            &report,
            "apply-rollback-protocol",
            "60e5c94601da99267431825e6eae19f68477ad9d7405f5c7c24945b40cfabc8f",
        );
        expect_hash(
            &report,
            "apply-constraints",
            "e136930179c1ac9145e769eaa4389826cfa572432a5a57aba2fba6b591066509",
        );
    }

    // ----- R54: stdin sentinel ------

    #[test]
    fn read_json_arg_returns_literal_when_not_dash() {
        let got = read_json_arg(r#"{"key":"value"}"#).unwrap();
        assert_eq!(got, r#"{"key":"value"}"#);
    }

    #[test]
    fn read_json_arg_literal_roundtrip() {
        // R54 part 1 (stdin sentinel): the pure literal path is tested here;
        // the `-` sentinel path is covered by a subprocess integration test
        // in a future assert_cmd harness — exercising it in unit tests would
        // require rewiring `std::io::stdin()`, which is invasive enough that
        // we defer it rather than carry a test-only file descriptor seam.
        let got = read_json_arg(r#"{"a":1}"#).unwrap();
        assert_eq!(got, r#"{"a":1}"#);
    }

    // R32: a second `-` sentinel on the same invocation must bail rather than
    // silently re-reading stdin (already at EOF) and returning empty. Hold the
    // env lock so we serialise against any other test that might touch the
    // shared STDIN_CONSUMED flag, then restore it for downstream tests.
    #[test]
    fn read_json_arg_dash_second_call_errors_already_consumed() {
        let _guard = env_lock();
        let prev = STDIN_CONSUMED.swap(false, std::sync::atomic::Ordering::SeqCst);
        // First `-` call: either succeeds (stdin readable) or errors (TTY / empty).
        // In both cases it should have set the consumed flag BEFORE returning.
        let _ = read_json_arg("-");
        let second = read_json_arg("-").unwrap_err();
        let msg = format!("{second:#}");
        assert!(
            msg.contains("already consumed"),
            "expected already-consumed error, got: {msg}"
        );
        // Restore for any other test that might run afterwards in this process.
        STDIN_CONSUMED.store(prev, std::sync::atomic::Ordering::SeqCst);
    }

    // Some tests above mutate process-wide env vars. Serialise them against
    // each other to avoid races when `cargo test` runs them in parallel.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
    }
}
