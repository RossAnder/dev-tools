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
use crate::dedup::{DupTier, items_find_duplicates, items_find_duplicates_across};
use crate::integrity::IntegrityOpts;
use crate::io::{
    mutate_doc, mutate_doc_conditional, mutate_doc_plan, read_doc, read_doc_borrowed,
    read_toml_str,
};
use crate::items::{
    AddManyOutcome, AddOutcome, MutationPlan, array_append, compute_apply_mutation,
    compute_remove_mutation, items_add_many, items_add_many_with_dedupe, items_add_to,
    items_add_value_with_dedupe_to, items_get_from, items_infer_and_next_id, items_next_id,
    items_update_to, parse_ndjson,
};
use crate::orphans::items_orphans;
use crate::query::{self, OutputShape, Query};


/// Maximum JSON payload accepted from stdin via the `-` sentinel. 32 MiB is
/// well above any realistic review-ledger / flow-context apply-ops payload
/// (typical is < 64 KiB) while being small enough to fail fast if a caller
/// accidentally pipes a log or a binary into `--json -`.
const MAX_STDIN_BYTES: u64 = 32 * 1024 * 1024;

/// R44: maximum number of ops accepted in a single `items apply` batch.
/// `MAX_STDIN_BYTES` alone does not bound op count — a well-formed 32 MiB
/// JSON array of tiny `{"op":"update","id":"Rx"}` records can hold tens of
/// thousands of operations, and `items_apply_to_opts` iterates serially.
/// 10_000 is far above any legitimate batch (typical ledgers have ~50 items
/// and typical apply batches ≤ 60 ops) while still bounded enough that an
/// accidental loop-generated mega-payload fails fast instead of timing out
/// the wrapping shell.
const MAX_OPS_PER_APPLY: usize = 10_000;

/// R32: guard against multiple `-` sentinels consuming stdin in a single
/// invocation (e.g. `--json - --ops -`). The second `read_json_arg("-")` call
/// errors out instead of silently returning an empty string (stdin already at
/// EOF) and corrupting the apply.
///
/// R38: the flag is deliberately a process-global `AtomicBool`:
///
/// - A CLI invocation is exactly one OS process with exactly one stdin
///   handle. "Multiple invocations" means multiple processes, each with
///   their own flag — so the global is semantically scoped to the right
///   thing at runtime.
/// - Threading an `&mut bool` through `run()` → every dispatcher → every
///   `read_json_arg` / `read_json_value_from_arg` call site would touch
///   ~12 functions for no runtime benefit (the flag's "global" reach is
///   already the whole process).
///
/// **Test contract**: unit tests that flip or rely on this flag (e.g.
/// `read_json_arg_dash_second_call_errors_already_consumed`) MUST hold
/// `env_lock()` for the duration of the test. `cargo test` parallelises
/// within a process, so without the lock two tests touching stdin would
/// race on the single flag. The lock is the test-side substitute for the
/// per-invocation isolation the real CLI gets for free.
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

/// T5: parse the `--dedupe-by` flag value into a `Vec<String>` of field
/// paths. `None` (flag absent) returns an empty Vec — the caller treats
/// that as "dedupe off" and the existing add/add-many code paths run
/// unchanged. `Some("")` or `Some(",,")` (all-empty after split-and-trim)
/// is a fail-loud case: the user typed the flag with no payload, which
/// almost certainly isn't what they meant; we error with a directed
/// message instead of silently disabling dedup.
fn parse_dedupe_fields(raw: Option<&str>) -> Result<Vec<String>> {
    let Some(s) = raw else {
        return Ok(Vec::new());
    };
    let fields: Vec<String> = s
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(String::from)
        .collect();
    if fields.is_empty() {
        bail!("--dedupe-by requires at least one field name");
    }
    Ok(fields)
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
    #[arg(long = "ndjson", help = "Output one JSON value per line (for piping into add-many/apply)")]
    pub(crate) ndjson: bool,
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
    /// `--prefix` via a `required(true)` ArgGroup — clap enforces the
    /// "exactly one" contract at parse time with a clean error message.
    #[command(group(
        clap::ArgGroup::new("id_source")
            .required(true)
            .multiple(false)
            .args(["prefix", "infer_from_file"])
    ))]
    NextId {
        file: PathBuf,
        #[arg(long, help = "Prefix letter (e.g. R, O, E) for the new id")]
        prefix: Option<String>,
        /// T4: derive the prefix by scanning existing ids in the ledger.
        /// Errors if the ledger is empty or uses more than one prefix.
        #[arg(
            long = "infer-from-file",
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
            let q = Query::from_cli_args(&legacy, &query)?;
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
        ItemsOp::Add { file, json, array, dedupe_by, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
            if dedupe_fields.is_empty() {
                // No-dedupe path: byte-identical behaviour to pre-T5 —
                // same helper, same `{"ok":true}` output, same
                // `mutate_doc` (always-write) pipeline. The T5 plan
                // suggested emitting `{"ok":true,"added":1}` even in the
                // no-dedupe case, but that would break the byte-identity
                // constraint ("absent --dedupe-by → current behaviour
                // byte-identical") since today's shape is plain
                // `{"ok":true}`. Keep the legacy shape for back-compat
                // and reserve the enriched shape for the `--dedupe-by`
                // branch below.
                let json = read_json_arg(&json)?;
                mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                    items_add_to(doc, &array, &json)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            } else {
                // Dedupe path: parse JSON once up-front so we can feed it
                // to the pre-scan inside the lock without a re-parse.
                // `mutate_doc_conditional` elides the write-and-sidecar
                // bump when the scan returns a match; the caller sees
                // `added:0,matched_id:...` and the on-disk file + sidecar
                // are untouched.
                let patch: JsonValue =
                    read_json_value_from_arg(&json).context("parsing --json")?;
                let mut outcome: Option<AddOutcome> = None;
                mutate_doc_conditional(&file, integrity.allow_outside, opts, |doc| {
                    let result = items_add_value_with_dedupe_to(
                        doc,
                        patch,
                        &array,
                        &dedupe_fields,
                    )?;
                    let mutated = matches!(result, AddOutcome::Added);
                    outcome = Some(result);
                    Ok(mutated)
                })?;
                match outcome.expect("closure always sets outcome on success") {
                    AddOutcome::Added => {
                        print_json_compact(&serde_json::json!({"ok": true, "added": 1}))?;
                    }
                    AddOutcome::Skipped { matched_id } => {
                        print_json_compact(&serde_json::json!({
                            "ok": true,
                            "added": 0,
                            "matched_id": matched_id,
                        }))?;
                    }
                }
            }
        }
        ItemsOp::AddMany {
            file,
            ndjson,
            defaults_json,
            array,
            dedupe_by,
            integrity,
        } => {
            let opts = write_integrity_opts(&integrity);
            let dedupe_fields = parse_dedupe_fields(dedupe_by.as_deref())?;
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
            if dedupe_fields.is_empty() {
                // No-dedupe path: byte-identical to pre-T5. Same helper,
                // same output shape (`{"ok":true,"added":N}`), same
                // always-write pipeline.
                let mut added: usize = 0;
                mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                    added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                    Ok(())
                })?;
                print_json_compact(&serde_json::json!({"ok": true, "added": added}))?;
            } else {
                // Dedupe path: run the pre-scan + append loop inside the
                // lock via `mutate_doc_conditional`. Skip the file write
                // entirely when the batch added zero rows — the doc is
                // untouched and the sidecar must not bump for a pure-
                // skip batch. Any `added > 0` takes the write branch.
                let mut outcome: Option<AddManyOutcome> = None;
                mutate_doc_conditional(&file, integrity.allow_outside, opts, |doc| {
                    let result = items_add_many_with_dedupe(
                        doc,
                        &array,
                        &rows,
                        defaults.as_ref(),
                        &dedupe_fields,
                    )?;
                    let mutated = result.added > 0;
                    outcome = Some(result);
                    Ok(mutated)
                })?;
                let outcome = outcome.expect("closure always sets outcome on success");
                let skipped_rows_json: Vec<JsonValue> = outcome
                    .skipped_rows
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "row": s.row,
                            "matched_id": s.matched_id,
                        })
                    })
                    .collect();
                print_json_compact(&serde_json::json!({
                    "ok": true,
                    "added": outcome.added,
                    "skipped": outcome.skipped_rows.len(),
                    "skipped_rows": skipped_rows_json,
                }))?;
            }
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
        ItemsOp::Remove { file, id, array, dry_run, integrity } => {
            let opts = write_integrity_opts(&integrity);
            if dry_run {
                // T10: dry-run path — compute the plan on a locally-read
                // doc (no exclusive lock) and emit the would_change
                // summary. The compute phase runs the same validation
                // as the live path (`compute_remove_mutation` delegates
                // to `items_remove_from` on a cloned doc), so a missing
                // id bails with the identical "no item with id = X"
                // error a real remove would surface.
                let read_opts = IntegrityOpts {
                    write_sidecar: false,
                    verify_on_read: integrity.verify_integrity,
                    strict: false,
                };
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_remove_mutation(doc, &array, &id)
                })?;
                emit_dry_run_plan(&plan)?;
            } else {
                // Live path: compute + apply via the split helpers so
                // the "live" and "dry-run" branches share the compute
                // stage byte-for-byte. The read happens inside the
                // exclusive lock via `mutate_doc_plan` so the same
                // TOCTOU narrowing as `mutate_doc` holds.
                mutate_doc_plan(&file, integrity.allow_outside, opts, |doc| {
                    compute_remove_mutation(doc, &array, &id)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            }
        }
        ItemsOp::Apply { file, ops, array, no_remove, dry_run, integrity } => {
            let opts = write_integrity_opts(&integrity);
            let ops = read_json_arg(&ops)?;
            // R44: bound the ops count at the CLI boundary. `MAX_STDIN_BYTES`
            // only caps the raw payload size; a 32 MiB JSON array of minimal
            // `{"op":"update","id":"Rx"}` records can still hold tens of
            // thousands of ops, which `items_apply_to_opts` iterates serially.
            // Check length here (before locking + parsing inside the mutator)
            // so an over-large payload fails fast with a directed message,
            // and the user-visible error predates any disk mutation.
            // T10: the check also gates `--dry-run`, so an over-large preview
            // refuses with the same message a real run would emit.
            let parsed_for_count: JsonValue =
                serde_json::from_str(&ops).context("parsing --ops")?;
            if let JsonValue::Array(arr) = &parsed_for_count
                && arr.len() > MAX_OPS_PER_APPLY
            {
                bail!(
                    "--ops contains {} operations, which exceeds the cap of {}; \
                     split the batch into smaller /review-apply or /optimise-apply \
                     invocations",
                    arr.len(),
                    MAX_OPS_PER_APPLY
                );
            }
            if dry_run {
                // T10: same compute phase as the live path, but we stop
                // before the I/O stage. `compute_apply_mutation` runs
                // `items_apply_to_opts` on a cloned doc, so every
                // validation gate — `--no-remove`, op-shape, missing id,
                // dedup_id auto-populate — fires with a byte-identical
                // error surface.
                let read_opts = IntegrityOpts {
                    write_sidecar: false,
                    verify_on_read: integrity.verify_integrity,
                    strict: false,
                };
                let plan = read_doc(&file, read_opts, |doc| {
                    compute_apply_mutation(doc, &array, &ops, no_remove)
                })?;
                emit_dry_run_plan(&plan)?;
            } else {
                mutate_doc_plan(&file, integrity.allow_outside, opts, |doc| {
                    compute_apply_mutation(doc, &array, &ops, no_remove)
                })?;
                print_json_compact(&serde_json::json!({"ok": true}))?;
            }
        }
        ItemsOp::NextId { file, prefix, infer_from_file, integrity } => {
            // The clap ArgGroup `id_source` guarantees exactly one of
            // `--prefix` / `--infer-from-file` reaches us; no runtime
            // "both unset" or "both set" check is needed.
            //
            // R19: if the target ledger doesn't exist yet, there's nothing to
            // parse or verify — the "next" id is trivially `<prefix>1`. This
            // lets flows call `items next-id` before the ledger is initialised
            // (e.g. during bootstrap of a new flow directory). When the caller
            // passed `--infer-from-file` and the file is absent, inference has
            // no corpus to work from, which is indistinguishable from the
            // "empty ledger" failure case — surface the same error so the
            // caller's remediation is the same either way.
            if !file.exists() {
                if infer_from_file {
                    bail!("--infer-from-file requires a non-empty ledger or explicit --prefix");
                }
                let prefix = prefix.as_deref().expect("clap group guarantees prefix is Some");
                if prefix.is_empty() {
                    bail!("prefix must not be empty — use a letter like R, O, or A");
                }
                if prefix.chars().all(|c| c.is_ascii_digit()) {
                    bail!("prefix must not be all-digit — would collide with numeric-suffix parsing");
                }
                println!("{}", serde_json::to_string(&format!("{}1", prefix))?);
            } else {
                let opts = read_integrity_opts(&integrity);
                let id = read_doc(&file, opts, |doc| {
                    if infer_from_file {
                        items_infer_and_next_id(doc)
                    } else {
                        let prefix =
                            prefix.as_deref().expect("clap group guarantees prefix is Some");
                        items_next_id(doc, prefix)
                    }
                })?;
                println!("{}", serde_json::to_string(&id)?);
            }
        }
        ItemsOp::FindDuplicates { file, tier, across, integrity } => {
            let opts = read_integrity_opts(&integrity);
            let groups = match across {
                None => read_doc(&file, opts, |doc| items_find_duplicates(doc, tier))?,
                Some(other_path) => {
                    // T6c: load both ledgers under the same integrity
                    // contract; errors propagate for either. Clone the
                    // primary's items out of the locked closure so the
                    // second `read_doc` can fire sequentially without
                    // nesting locks (nesting them would risk lock-order
                    // inversion against any concurrent writer).
                    let primary_file = file.to_string_lossy().into_owned();
                    let other_file = other_path.to_string_lossy().into_owned();
                    let primary_items: Vec<toml::Value> = read_doc(&file, opts, |doc| {
                        Ok(crate::io::items_array(doc, "items").to_vec())
                    })?;
                    let other_items: Vec<toml::Value> = read_doc(&other_path, opts, |doc| {
                        Ok(crate::io::items_array(doc, "items").to_vec())
                    })?;
                    items_find_duplicates_across(
                        &primary_items,
                        &primary_file,
                        &other_items,
                        &other_file,
                        tier,
                    )?
                }
            };
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

/// Legacy shortcut flags that predate the `--where-*` family on `items list`.
/// Kept on the CLI for back-compat (`--status`, `--category`, `--file`,
/// `--newer-than`) but translated into equivalent `Predicate` entries in
/// `Query::from_cli_args` so the query engine only sees one predicate list.
/// R69: bundled into a small struct so `from_cli_args` can take
/// `(legacy, query)` rather than the prior 26-positional-arg signature.
pub(crate) struct LegacyShortcuts<'a> {
    pub(crate) status: &'a Option<String>,
    pub(crate) category: &'a Option<String>,
    pub(crate) file: &'a Option<String>,
    pub(crate) newer_than: &'a Option<String>,
    pub(crate) count: bool,
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

/// T10: emit the `--dry-run` summary for `items remove --dry-run` and
/// `items apply --dry-run`. Output shape is a single compact JSON line:
///
/// ```text
/// {"ok":true,"dry_run":true,"would_change":{"added":N,"updated":N,"removed":N,"ids":[...]}}
/// ```
///
/// `ids` is the concatenation `[...added, ...updated, ...removed]` in
/// that order, matching `MutationPlan::union_ids`. `N` values are plain
/// integer counts (not arrays) so the output stays stable and terse
/// across both dispatch arms.
fn emit_dry_run_plan(plan: &MutationPlan) -> Result<()> {
    let summary = serde_json::json!({
        "ok": true,
        "dry_run": true,
        "would_change": {
            "added": plan.added.len(),
            "updated": plan.updated.len(),
            "removed": plan.removed.len(),
            "ids": plan.union_ids(),
        },
    });
    print_json_compact(&summary)
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
        // `scripts/shared-blocks.toml`. The blocks have divergent file
        // coverage — `flow-context` spans 8 command files (all flow-aware
        // commands); `ledger-schema` spans 4 (the review/optimise pair); the
        // three apply-only blocks span 2 (optimise-apply + review-apply).
        // Splitting the assertions this way lets a drift in any one block
        // surface independently with a named hash, instead of a confusing
        // "missing" report from running a narrower block over a wider file
        // list.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
        let cmd_dir = repo_root.join("claude").join("commands");
        let flow_context_eight = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
            cmd_dir.join("plan-new.md"),
            cmd_dir.join("plan-update.md"),
            cmd_dir.join("implement.md"),
            cmd_dir.join("review-plan.md"),
        ];
        let ledger_schema_four = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];
        let execution_record_three = [
            cmd_dir.join("plan-new.md"),
            cmd_dir.join("plan-update.md"),
            cmd_dir.join("implement.md"),
        ];
        let apply_pair = [
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];

        // Only run when every file is present. The test crate is consumable
        // in isolation; degrade gracefully if someone packages it without
        // the command tree.
        if !flow_context_eight.iter().all(|p| p.exists())
            || !ledger_schema_four.iter().all(|p| p.exists())
            || !execution_record_three.iter().all(|p| p.exists())
        {
            eprintln!(
                "blocks_verify_reproduces_shell_hashes: command files not found, skipping"
            );
            return;
        }

        // R53: on hash-drift the bare `assertion_failed` message is hard to
        // act on — the caller sees "block X hash drift" and has to reverse-
        // engineer both the actual hash and which file(s) moved. Emit a
        // structured multi-line message instead:
        //   - expected (the pinned constant that's now stale)
        //   - actual   (the in-parity hash currently produced by the blocks
        //              under test; absent when parity itself is broken)
        //   - per-file hashes (for parity: the single hash each file maps
        //              to; for drift: every (file, hash) pair so the
        //              operator can spot the outlier file without re-running)
        //   - remediation (the literal pinned-hash constant update to make)
        let expect_hash = |report: &blocks::BlocksReport, name: &str, expected: &str| {
            let blocks_arr = report
                .report
                .get("blocks")
                .and_then(|v| v.as_array())
                .expect("blocks array");
            let block = blocks_arr
                .iter()
                .find(|b| b.get("name").and_then(|v| v.as_str()) == Some(name))
                .unwrap_or_else(|| {
                    panic!(
                        "block `{name}` missing from report: {:?}",
                        report.report
                    )
                });

            // The "happy" shape (`blocks_verify` reports parity): a single
            // `hash` field + a `files` array. Compare the pinned constant
            // against it; on mismatch, print every contributing file so the
            // operator can copy the new hash into the source.
            if let Some(hash) = block.get("hash").and_then(|v| v.as_str()) {
                if hash != expected {
                    let files: Vec<String> = block
                        .get("files")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut msg = String::new();
                    msg.push_str(&format!("block `{name}` pinned-hash drift\n"));
                    msg.push_str(&format!("  expected: {expected}\n"));
                    msg.push_str(&format!("  actual:   {hash}\n"));
                    msg.push_str("  per-file (all match each other):\n");
                    for f in &files {
                        msg.push_str(&format!("    {f}: {hash}\n"));
                    }
                    msg.push_str(&format!(
                        "  fix: update the pinned hash for `{name}` to {hash}"
                    ));
                    panic!("{msg}");
                }
                return;
            }

            // The "sad" shape: `blocks_verify` already detected drift across
            // files — there is no single `hash`, only a `drift` array of
            // per-file hashes. Emit all of them so the operator can see
            // both WHICH file moved and whether the pinned constant is
            // stale as well.
            let drift_arr = block
                .get("drift")
                .and_then(|v| v.as_array())
                .unwrap_or_else(|| {
                    panic!(
                        "block `{name}` has neither `hash` nor `drift`: {:?}",
                        block
                    )
                });
            let mut msg = String::new();
            msg.push_str(&format!(
                "block `{name}` parity broken across files (pre-pinned-hash check)\n"
            ));
            msg.push_str(&format!("  expected (pinned): {expected}\n"));
            msg.push_str("  per-file hashes (should be identical, but differ):\n");
            for entry in drift_arr {
                let f = entry
                    .get("file")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<unknown>");
                let h = entry
                    .get("hash")
                    .and_then(|v| v.as_str())
                    .unwrap_or("<no-hash>");
                msg.push_str(&format!("    {f}: {h}\n"));
            }
            msg.push_str(
                "  fix: restore block parity across the listed files first, \
                 then re-run this test to see whether the pinned constant \
                 also needs updating",
            );
            panic!("{msg}");
        };

        // --- 8-file flow-context block ---
        let report = blocks_verify(
            &flow_context_eight,
            &["flow-context".to_string()],
        )
        .unwrap();
        assert!(report.ok, "flow-context block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "flow-context",
            "bb87cac4d3af0ff86737342f7691023901a6f1bd9811c316c85f928edacd38eb",
        );

        // --- 4-file ledger-schema block ---
        let report = blocks_verify(
            &ledger_schema_four,
            &["ledger-schema".to_string()],
        )
        .unwrap();
        assert!(report.ok, "ledger-schema block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "ledger-schema",
            "23df0a7893ea44e356979328ef62592edd6493c2d1d34e0520e59958129ca14c",
        );

        // --- 3-file execution-record-schema block ---
        let report = blocks_verify(
            &execution_record_three,
            &["execution-record-schema".to_string()],
        )
        .unwrap();
        assert!(report.ok, "execution-record-schema block must be parity: {:?}", report.report);
        expect_hash(
            &report,
            "execution-record-schema",
            "80d533204acce2774fe73028d3b7c7b3789a2695d70ec24b788a5f7f51027d5f",
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
