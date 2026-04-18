use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use serde_json::Value as JsonValue;
use std::io::{BufWriter, IsTerminal, Read, Write};
use std::path::PathBuf;
use toml::Value as TomlValue;

// R24/R59: integrity-sidecar machinery, scalar/JSON conversion helpers, and
// (soon) the rest of the split modules live in sibling files so `main.rs` can
// shrink to pure dispatch plumbing.
mod blocks;
mod convert;
mod dedup;
mod integrity;
mod io;
mod orphans;
mod query;
#[cfg(test)]
mod test_support;

use blocks::blocks_verify;
#[cfg(test)]
use blocks::scan_block_names_warn;
use convert::{
    ScalarType, json_type_name, maybe_date_coerce, navigate, parse_scalar, set_at_path,
    toml_to_json,
};
// Surfaced to the in-file test module only — dispatch code doesn't call these
// directly, but several tests do via `use super::*;`. `str_field` moved here
// when task 5 routed `items list` through `query::run`; it's still referenced
// by the legacy `items_list_from` path kept for the in-file test suite.
#[cfg(test)]
use convert::{DATE_KEYS, infer_type, json_to_toml, str_field};
#[cfg(test)]
use io::{guard_write_path, with_exclusive_lock};
#[cfg(test)]
use std::fs;
#[cfg(test)]
use std::path::Path;
use dedup::{DupTier, items_find_duplicates};
use integrity::{IntegrityOpts, maybe_verify_integrity};
use io::{mutate_doc, read_toml, repo_or_cwd_root};
use orphans::items_orphans;
use query::{OutputShape, Predicate, Query, SortDir, validate_query};

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

/// Resolve a JSON argument: if it's literally "-", read stdin to a String.
/// Otherwise return the argument as-is.
///
/// Stdin handling (R7):
/// - Refuses to block on an interactive TTY — a user piping nothing into `--json -`
///   would otherwise hang forever with no prompt or feedback.
/// - Caps the read at `MAX_STDIN_BYTES`; an oversize payload is truncated on
///   the input side so a misrouted log doesn't balloon tomlctl's heap.
fn read_json_arg(arg: &str) -> Result<String> {
    if arg == "-" {
        // R32: a single invocation can only consume stdin once. A second `-`
        // sentinel would read an already-drained handle and silently return an
        // empty payload, which downstream would treat as a no-op or a parse
        // error with a confusing message. `swap(true, SeqCst)` is both the
        // check and the mark, so concurrent calls can't both see `false`.
        if STDIN_CONSUMED.swap(true, std::sync::atomic::Ordering::SeqCst) {
            bail!(
                "stdin already consumed by another flag on this invocation; only one --json/--ops/--patch flag can use the `-` sentinel per call"
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

/// R60: the four integrity/containment flags — previously `global = true` on
/// `Cli`, now flattened into each TOML-touching subcommand variant. This makes
/// `blocks verify` structurally refuse `--verify-integrity` / `--allow-outside`
/// / etc. (blocks operates on markdown, not the TOML + sidecar pair), and
/// documents the contract at the clap layer rather than only in help text.
#[derive(Args, Clone)]
#[command(next_help_heading = "Integrity options")]
struct IntegrityArgs {
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
        integrity: IntegrityArgs,
    },

    /// Print the value at a dotted key path as JSON (or the whole doc if path is omitted).
    Get {
        file: PathBuf,
        /// Dotted path, e.g. "tasks.total" or "artifacts.optimise_findings". Omit to dump whole file.
        path: Option<String>,
        #[command(flatten)]
        integrity: IntegrityArgs,
    },

    /// Set a scalar at a dotted key path. Type auto-inferred with --type.
    Set {
        file: PathBuf,
        path: String,
        value: String,
        #[arg(long = "type", value_enum)]
        ty: Option<ScalarType>,
        #[command(flatten)]
        integrity: IntegrityArgs,
    },

    /// Set a JSON-encoded value (array, object, or scalar) at a dotted key path.
    SetJson {
        file: PathBuf,
        path: String,
        #[arg(long, help = "JSON-encoded value; pass `-` to read from stdin")]
        json: String,
        #[command(flatten)]
        integrity: IntegrityArgs,
    },

    /// Parse-check only. Exit 0 on valid TOML, non-zero otherwise.
    Validate {
        file: PathBuf,
        #[command(flatten)]
        integrity: IntegrityArgs,
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
        integrity: IntegrityArgs,
    },
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum ItemsOp {
    /// List items as a JSON array. Optional filters combine via AND. With
    /// `--count`, print `{"count": <n>}` instead of the item array.
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
        integrity: IntegrityArgs,
    },

    /// Get a single item by its `id` field.
    Get {
        file: PathBuf,
        id: String,
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        #[command(flatten)]
        integrity: IntegrityArgs,
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
        integrity: IntegrityArgs,
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
        integrity: IntegrityArgs,
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
        integrity: IntegrityArgs,
    },

    /// Remove an item by id. Fails if no such id exists.
    Remove {
        file: PathBuf,
        id: String,
        /// R57: target array-of-tables name. See `List --array`.
        #[arg(long, default_value = "items")]
        array: String,
        #[command(flatten)]
        integrity: IntegrityArgs,
    },

    /// Print the next id string for the given prefix (default R).
    NextId {
        file: PathBuf,
        #[arg(long, default_value = "R")]
        prefix: String,
        #[command(flatten)]
        integrity: IntegrityArgs,
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
        integrity: IntegrityArgs,
    },

    /// Find duplicate items using one of the dedup tiers.
    FindDuplicates {
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = DupTier::A)]
        tier: DupTier,
        #[command(flatten)]
        integrity: IntegrityArgs,
    },

    /// Surface items whose file or symbol has drifted, or whose depends_on
    /// points at an id that isn't in the ledger.
    Orphans {
        file: PathBuf,
        #[command(flatten)]
        integrity: IntegrityArgs,
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

/// Translate the flattened `IntegrityArgs` from a subcommand variant into the
/// module-local `IntegrityOpts` bundle. Kept next to the CLI definition
/// (rather than in `integrity.rs`) so the integrity module stays free of the
/// clap-derived types.
fn integrity_opts_from_args(args: &IntegrityArgs) -> IntegrityOpts {
    IntegrityOpts {
        write_sidecar: !args.no_write_integrity,
        verify_on_read: args.verify_integrity,
        strict: args.strict_integrity,
    }
}

fn main() {
    if let Err(err) = run() {
        // R16: `{:#}` prints the full anyhow cause chain inline; combined with
        // `with_context(…"parsing {}", path)` in `read_toml`, toml's Display
        // impl then emits line:col + caret diagnostics for syntax errors.
        eprintln!("tomlctl: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Parse { file, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            print_json(&toml_to_json(&doc))?;
        }
        Cmd::Get { file, path, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            let out = match path.as_deref() {
                None | Some("") => toml_to_json(&doc),
                Some(p) => toml_to_json(
                    navigate(&doc, p).ok_or_else(|| anyhow!("key path `{}` not found", p))?,
                ),
            };
            print_json(&out)?;
        }
        Cmd::Set {
            file,
            path,
            value,
            ty,
            integrity,
        } => {
            let opts = integrity_opts_from_args(&integrity);
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let v = parse_scalar(&value, ty)?;
                set_at_path(doc, &path, v)
            })?;
            println!("{{\"ok\":true}}");
        }
        Cmd::SetJson { file, path, json, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                let parsed: JsonValue = serde_json::from_str(&json).context("parsing --json")?;
                let last_key = path.rsplit_once('.').map(|(_, k)| k).unwrap_or(path.as_str());
                let v = maybe_date_coerce(last_key, &parsed)?;
                set_at_path(doc, &path, v)
            })?;
            println!("{{\"ok\":true}}");
        }
        Cmd::Validate { file, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            read_toml(&file)?;
            println!("{{\"ok\":true}}");
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
            let opts = integrity_opts_from_args(&integrity);
            let rows: Vec<JsonValue> = if let Some(j) = json {
                let text = read_json_arg(&j)?;
                let parsed: JsonValue =
                    serde_json::from_str(&text).context("parsing --json")?;
                if !parsed.is_object() {
                    bail!("--json must be a JSON object");
                }
                vec![parsed]
            } else {
                let nd = ndjson.expect("checked above");
                let text = if nd == "-" {
                    read_json_arg("-")?
                } else {
                    std::fs::read_to_string(&nd)
                        .with_context(|| format!("reading NDJSON file `{}`", nd))?
                };
                parse_ndjson(&text)?
            };
            let mut appended: usize = 0;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                appended = array_append(doc, &array, &rows)?;
                Ok(())
            })?;
            println!("{{\"ok\":true,\"appended\":{}}}", appended);
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
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            let q = build_query(
                &status,
                &category,
                &newer_than,
                &file_filter,
                count,
                &query.where_eq,
                &query.where_not,
                &query.where_in,
                &query.where_has,
                &query.where_missing,
                &query.where_gt,
                &query.where_gte,
                &query.where_lt,
                &query.where_lte,
                &query.where_contains,
                &query.where_prefix,
                &query.where_suffix,
                &query.where_regex,
                query.select.as_deref(),
                query.exclude.as_deref(),
                query.pluck.as_deref(),
                &query.sort_by,
                query.limit,
                query.offset,
                query.distinct,
                query.group_by.as_deref(),
                query.count_by.as_deref(),
                query.ndjson,
            )?;
            validate_query(&q)?;
            let out = query::run(&doc, &array, &q)?;
            match q.shape {
                OutputShape::Ndjson => {
                    // `out` is a JSON array of items; emit one compact JSON
                    // value per line so the output pipes cleanly into
                    // `items add-many --ndjson -` / `items apply --ops -`.
                    let arr = out
                        .as_array()
                        .ok_or_else(|| anyhow!("ndjson expects array output"))?;
                    let stdout = std::io::stdout();
                    let mut h = stdout.lock();
                    for v in arr {
                        serde_json::to_writer(&mut h, v)?;
                        h.write_all(b"\n")?;
                    }
                    h.flush()?;
                }
                _ => print_json(&out)?,
            }
        }
        ItemsOp::Get { file, id, array, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            print_json(&items_get_from(&doc, &array, &id)?)?;
        }
        ItemsOp::Add { file, json, array, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_add_to(doc, &array, &json)
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::AddMany {
            file,
            ndjson,
            defaults_json,
            array,
            integrity,
        } => {
            let opts = integrity_opts_from_args(&integrity);
            // NDJSON source: `-` means stdin (routed through `read_json_arg`
            // so the STDIN_CONSUMED guard refuses a second `-` below when
            // `--defaults-json -` also wants stdin); any other value is a
            // file path read verbatim.
            let ndjson_text = if ndjson == "-" {
                read_json_arg("-")?
            } else {
                std::fs::read_to_string(&ndjson)
                    .with_context(|| format!("reading NDJSON file `{}`", ndjson))?
            };
            let rows = parse_ndjson(&ndjson_text)?;
            let defaults: Option<JsonValue> = match defaults_json.as_deref() {
                Some(s) => {
                    let text = read_json_arg(s)?;
                    Some(
                        serde_json::from_str(&text).context("parsing --defaults-json")?,
                    )
                }
                None => None,
            };
            let mut added: usize = 0;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                added = items_add_many(doc, &array, &rows, defaults.as_ref())?;
                Ok(())
            })?;
            println!("{{\"ok\":true,\"added\":{}}}", added);
        }
        ItemsOp::Update {
            file,
            id,
            json,
            unset,
            array,
            integrity,
        } => {
            let opts = integrity_opts_from_args(&integrity);
            let json = read_json_arg(&json)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_update_to(doc, &array, &id, &json, &unset)
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::Remove { file, id, array, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_remove_from(doc, &array, &id)
            })?;
            println!("{{\"ok\":true}}");
        }
        ItemsOp::Apply { file, ops, array, no_remove, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            let ops = read_json_arg(&ops)?;
            mutate_doc(&file, integrity.allow_outside, opts, |doc| {
                items_apply_to_opts(doc, &ops, &array, no_remove)
            })?;
            println!("{{\"ok\":true}}");
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
                let opts = integrity_opts_from_args(&integrity);
                maybe_verify_integrity(&file, opts)?;
                let doc = read_toml(&file)?;
                let id = items_next_id(&doc, &prefix)?;
                println!("{}", serde_json::to_string(&id)?);
            }
        }
        ItemsOp::FindDuplicates { file, tier, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            let groups = items_find_duplicates(&doc, tier)?;
            print_json(&JsonValue::Array(groups))?;
        }
        ItemsOp::Orphans { file, integrity } => {
            let opts = integrity_opts_from_args(&integrity);
            maybe_verify_integrity(&file, opts)?;
            let doc = read_toml(&file)?;
            let orphans = items_orphans(&doc)?;
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

/// Build a `query::Query` from the flat list of clap flag values on
/// `ItemsOp::List`. Does not call `validate_query` — the caller does that
/// so dispatch can surface the error via the same path as other runtime
/// errors. All legacy shortcut flags (`--status` / `--category` / `--file` /
/// `--newer-than`) are translated into equivalent `Predicate` entries so
/// `query::run` sees a single predicate list.
#[allow(clippy::too_many_arguments)]
fn build_query(
    status: &Option<String>,
    category: &Option<String>,
    newer_than: &Option<String>,
    file_filter: &Option<String>,
    count: bool,
    where_eq: &[String],
    where_not: &[String],
    where_in: &[String],
    where_has: &[String],
    where_missing: &[String],
    where_gt: &[String],
    where_gte: &[String],
    where_lt: &[String],
    where_lte: &[String],
    where_contains: &[String],
    where_prefix: &[String],
    where_suffix: &[String],
    where_regex: &[String],
    select: Option<&str>,
    exclude: Option<&str>,
    pluck: Option<&str>,
    sort_by: &[String],
    limit: Option<usize>,
    offset: Option<usize>,
    distinct: bool,
    group_by: Option<&str>,
    count_by: Option<&str>,
    ndjson: bool,
) -> Result<Query> {
    let mut predicates: Vec<Predicate> = Vec::new();

    // Legacy shortcut flags — map onto the new predicate surface so the
    // query engine has a single filter list to evaluate. Duplicating a
    // legacy flag with an equivalent `--where` is a no-op (same predicate
    // runs twice; same result).
    if let Some(v) = status {
        predicates.push(Predicate::Where {
            key: "status".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = category {
        predicates.push(Predicate::Where {
            key: "category".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = file_filter {
        predicates.push(Predicate::Where {
            key: "file".into(),
            rhs: v.clone(),
        });
    }
    if let Some(v) = newer_than {
        // `--newer-than` semantically means "first_flagged > v" where v is
        // a YYYY-MM-DD. The `@date:` prefix tells `parse_typed_value` to
        // coerce the RHS to a TOML date rather than comparing as a string.
        predicates.push(Predicate::WhereGt {
            key: "first_flagged".into(),
            rhs: format!("@date:{}", v),
        });
    }

    for s in where_eq {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::Where { key, rhs });
    }
    for s in where_not {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereNot { key, rhs });
    }
    for s in where_in {
        let (key, rhs) = split_kv(s)?;
        let values: Vec<String> = rhs.split(',').map(|s| s.to_string()).collect();
        predicates.push(Predicate::WhereIn { key, rhs: values });
    }
    for s in where_has {
        if s.is_empty() {
            bail!("--where-has expects a KEY, got empty string");
        }
        predicates.push(Predicate::WhereHas { key: s.clone() });
    }
    for s in where_missing {
        if s.is_empty() {
            bail!("--where-missing expects a KEY, got empty string");
        }
        predicates.push(Predicate::WhereMissing { key: s.clone() });
    }
    for s in where_gt {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereGt { key, rhs });
    }
    for s in where_gte {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereGte { key, rhs });
    }
    for s in where_lt {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereLt { key, rhs });
    }
    for s in where_lte {
        let (key, rhs) = split_kv(s)?;
        predicates.push(Predicate::WhereLte { key, rhs });
    }
    for s in where_contains {
        let (key, sub) = split_kv(s)?;
        predicates.push(Predicate::WhereContains { key, sub });
    }
    for s in where_prefix {
        let (key, prefix) = split_kv(s)?;
        predicates.push(Predicate::WherePrefix { key, prefix });
    }
    for s in where_suffix {
        let (key, suffix) = split_kv(s)?;
        predicates.push(Predicate::WhereSuffix { key, suffix });
    }
    for s in where_regex {
        let (key, pattern) = split_kv(s)?;
        predicates.push(Predicate::WhereRegex { key, pattern });
    }

    // Projection: parse `--select a,b` / `--exclude a,b` into Vec<String>.
    // `validate_query` enforces `select` / `exclude` / `pluck` mutual
    // exclusion; we just populate the struct.
    let select_fields: Option<Vec<String>> =
        select.map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
    let exclude_fields: Option<Vec<String>> =
        exclude.map(|s| s.split(',').map(|t| t.trim().to_string()).collect());

    // Sort: each entry is `FIELD` or `FIELD:asc` or `FIELD:desc`. Unknown
    // suffix defaults to `asc` (matches the plan).
    let mut sort_list: Vec<(String, SortDir)> = Vec::new();
    for entry in sort_by {
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
    // ndjson > default Array. Multiple shape flags would typically collapse
    // to the highest-priority one here; `validate_query` then rejects any
    // shape-vs-projection conflict with a clear error.
    let shape = if count {
        OutputShape::Count
    } else if let Some(f) = count_by {
        OutputShape::CountBy(f.to_string())
    } else if let Some(f) = group_by {
        OutputShape::GroupBy(f.to_string())
    } else if let Some(f) = pluck {
        OutputShape::Pluck(f.to_string())
    } else if ndjson {
        OutputShape::Ndjson
    } else {
        OutputShape::Array
    };

    Ok(Query {
        predicates,
        select: select_fields,
        exclude: exclude_fields,
        sort_by: sort_list,
        limit,
        offset,
        distinct,
        shape,
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

/// Read-side access to a named array-of-tables. Returns an empty slice when
/// the array is missing or the value at that key isn't an array — symmetric
/// with `items_array_mut`, which auto-creates on write. R44: the previous
/// signature returned `Err(…)` on missing, which every caller had to
/// immediately translate into an empty-list fallback; inlining that policy
/// here removes five `match items_array { Err(_) => … }` tails.
pub(crate) fn items_array<'a>(doc: &'a TomlValue, name: &str) -> &'a [TomlValue] {
    static EMPTY: Vec<TomlValue> = Vec::new();
    doc.get(name)
        .and_then(|v| v.as_array())
        .map(Vec::as_slice)
        .unwrap_or(EMPTY.as_slice())
}

fn items_array_mut<'a>(doc: &'a mut TomlValue, name: &str) -> Result<&'a mut Vec<TomlValue>> {
    let root = doc
        .as_table_mut()
        .ok_or_else(|| anyhow!("root is not a table"))?;
    let entry = root
        .entry(name.to_string())
        .or_insert_with(|| TomlValue::Array(Vec::new()));
    entry
        .as_array_mut()
        .ok_or_else(|| anyhow!("`{}` is not an array", name))
}

pub(crate) fn item_id(item: &TomlValue) -> Option<&str> {
    item.as_table()?.get("id")?.as_str()
}

/// Bundle of optional per-field filters for `items list`. All populated filters
/// combine via logical AND; unset filters are no-ops.
///
/// Task 5 rewired the production `items list` dispatch through `query::run`,
/// so this struct and the legacy `items_list_from` / `datetime_gt` below are
/// now only reached from the in-file `#[cfg(test)]` suite (the tests still
/// exercise the legacy filter semantics directly; `query::run` is covered by
/// its own module tests and by `tests/integration.rs`).
#[cfg(test)]
#[derive(Clone, Copy, Default)]
struct ListFilters<'a> {
    status: Option<&'a str>,
    category: Option<&'a str>,
    newer_than: Option<&'a toml::value::Datetime>,
    file_filter: Option<&'a str>,
}

#[cfg(test)]
fn items_list(doc: &TomlValue, filters: ListFilters<'_>) -> Result<Vec<JsonValue>> {
    items_list_from(doc, "items", filters)
}

/// R57: array-parametric variant of `items_list`. Reads from the named
/// array-of-tables (default `items`). Filters apply uniformly — there's no
/// per-array filter-key policy, the same `ListFilters` shape is used.
#[cfg(test)]
fn items_list_from(
    doc: &TomlValue,
    array_name: &str,
    filters: ListFilters<'_>,
) -> Result<Vec<JsonValue>> {
    let items = items_array(doc, array_name);
    let mut out = Vec::new();
    for item in items {
        let Some(tbl) = item.as_table() else { continue };
        if let Some(want) = filters.status
            && str_field(tbl, "status") != want
        {
            continue;
        }
        if let Some(want) = filters.category
            && str_field(tbl, "category") != want
        {
            continue;
        }
        if let Some(want) = filters.file_filter
            && str_field(tbl, "file") != want
        {
            continue;
        }
        if let Some(threshold) = filters.newer_than {
            let Some(ff) = tbl.get("first_flagged").and_then(|v| v.as_datetime()) else {
                continue;
            };
            if !datetime_gt(ff, threshold) {
                continue;
            }
        }
        out.push(toml_to_json(item));
    }
    Ok(out)
}

/// Strict `a > b` comparison for TOML Datetime values. Compares the string
/// representations, which is correct for ISO-8601 dates and date-times — the
/// lexicographic order on ISO-8601 matches chronological order.
#[cfg(test)]
fn datetime_gt(a: &toml::value::Datetime, b: &toml::value::Datetime) -> bool {
    a.to_string() > b.to_string()
}

#[cfg(test)]
fn items_get(doc: &TomlValue, id: &str) -> Result<JsonValue> {
    items_get_from(doc, "items", id)
}

/// R57: array-parametric `items get`. See `List --array`.
fn items_get_from(doc: &TomlValue, array_name: &str, id: &str) -> Result<JsonValue> {
    for item in items_array(doc, array_name) {
        if item_id(item) == Some(id) {
            return Ok(toml_to_json(item));
        }
    }
    bail!("no item with id = {}", id)
}


#[cfg(test)]
fn items_add(doc: &mut TomlValue, json: &str) -> Result<()> {
    items_add_to(doc, "items", json)
}

/// R57: array-parametric `items add`. See `List --array`.
fn items_add_to(doc: &mut TomlValue, array_name: &str, json: &str) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json).context("parsing --json")?;
    items_add_value_to(doc, &patch, array_name)
}


fn items_add_value_to(doc: &mut TomlValue, patch: &JsonValue, array_name: &str) -> Result<()> {
    let obj = patch
        .as_object()
        .ok_or_else(|| anyhow!("--json must be a JSON object"))?;
    let mut tbl = toml::Table::new();
    for (k, v) in obj.iter() {
        tbl.insert(k.clone(), maybe_date_coerce(k, v)?);
    }
    let arr = items_array_mut(doc, array_name)?;
    arr.push(TomlValue::Table(tbl));
    Ok(())
}

#[cfg(test)]
fn items_update(doc: &mut TomlValue, id: &str, json: &str, unset: &[String]) -> Result<()> {
    items_update_to(doc, "items", id, json, unset)
}

/// R57: array-parametric `items update`. See `List --array`.
fn items_update_to(
    doc: &mut TomlValue,
    array_name: &str,
    id: &str,
    json: &str,
    unset: &[String],
) -> Result<()> {
    let patch: JsonValue = serde_json::from_str(json).context("parsing --json")?;
    items_update_value_to(doc, array_name, id, &patch, unset)
}

fn items_update_value_to(
    doc: &mut TomlValue,
    array_name: &str,
    id: &str,
    patch: &JsonValue,
    unset: &[String],
) -> Result<()> {
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| anyhow!("--json must be a JSON object"))?;

    let arr = items_array_mut(doc, array_name)?;
    for item in arr.iter_mut() {
        let Some(tbl) = item.as_table_mut() else { continue };
        let matches = tbl.get("id").and_then(|v| v.as_str()) == Some(id);
        if !matches {
            continue;
        }
        for (k, v) in patch_obj.iter() {
            tbl.insert(k.clone(), maybe_date_coerce(k, v)?);
        }
        for key in unset {
            tbl.remove(key);
        }
        return Ok(());
    }
    bail!("no item with id = {}", id)
}

#[cfg(test)]
fn items_apply(doc: &mut TomlValue, ops_json: &str) -> Result<()> {
    items_apply_to(doc, ops_json, "items")
}

#[cfg(test)]
fn items_apply_to(doc: &mut TomlValue, ops_json: &str, array_name: &str) -> Result<()> {
    items_apply_to_opts(doc, ops_json, array_name, false)
}

/// Extended variant of `items_apply_to` honouring the `--no-remove` flag (R37).
/// When `no_remove` is true, the batch is scanned up-front for any `remove` op;
/// if present, the whole apply is refused — no partial mutation occurs because
/// the check runs before the mutation loop.
fn items_apply_to_opts(
    doc: &mut TomlValue,
    ops_json: &str,
    array_name: &str,
    no_remove: bool,
) -> Result<()> {
    let ops: JsonValue = serde_json::from_str(ops_json).context("parsing --ops")?;
    let arr = ops
        .as_array()
        .ok_or_else(|| anyhow!("--ops must be a JSON array"))?;
    if no_remove {
        for (i, op) in arr.iter().enumerate() {
            if op.get("op").and_then(|v| v.as_str()) == Some("remove") {
                bail!(
                    "op[{}] is a remove op, but --no-remove was set; this flag is used by review-apply/optimise-apply to prevent agent-generated payloads from erasing audit history",
                    i
                );
            }
        }
    }
    for (i, op) in arr.iter().enumerate() {
        apply_single_op(doc, op, array_name).with_context(|| format!("op[{}] failed", i))?;
    }
    Ok(())
}

fn apply_single_op(doc: &mut TomlValue, op: &JsonValue, array_name: &str) -> Result<()> {
    let obj = op
        .as_object()
        .ok_or_else(|| anyhow!("op must be a JSON object"))?;
    let op_name = obj
        .get("op")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("op missing `op` field"))?;
    match op_name {
        "add" => {
            let json = obj
                .get("json")
                .ok_or_else(|| anyhow!("add op missing `json` field"))?;
            items_add_value_to(doc, json, array_name)
        }
        "update" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("update op missing `id` field"))?;
            let json = obj
                .get("json")
                .ok_or_else(|| anyhow!("update op missing `json` field"))?;
            let unset: Vec<String> = match obj.get("unset") {
                None | Some(JsonValue::Null) => Vec::new(),
                Some(JsonValue::Array(a)) => {
                    let mut out = Vec::with_capacity(a.len());
                    for (idx, entry) in a.iter().enumerate() {
                        match entry {
                            JsonValue::String(s) => out.push(s.clone()),
                            // R36: report element type + index only; the value
                            // itself may be agent-generated text and must not
                            // land on stderr verbatim.
                            other => bail!(
                                "update op `unset` must be an array of strings, got {} at index {}",
                                json_type_name(other),
                                idx
                            ),
                        }
                    }
                    out
                }
                // R36: value suppressed — report only the JSON type.
                Some(other) => bail!(
                    "update op `unset` must be a JSON array of strings, got {}",
                    json_type_name(other)
                ),
            };
            // R57: update now honours the apply-op's --array parameter so a
            // batch targeting e.g. `rollback_events` can update entries there,
            // not just in `[[items]]`.
            items_update_value_to(doc, array_name, id, json, &unset)
        }
        "remove" => {
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("remove op missing `id` field"))?;
            // R57: remove also follows the --array parameter.
            items_remove_from(doc, array_name, id)
        }
        other => bail!("unknown op `{}`", other),
    }
}

#[cfg(test)]
fn items_remove(doc: &mut TomlValue, id: &str) -> Result<()> {
    items_remove_from(doc, "items", id)
}

/// R57: array-parametric `items remove`. See `List --array`.
fn items_remove_from(doc: &mut TomlValue, array_name: &str, id: &str) -> Result<()> {
    let arr = items_array_mut(doc, array_name)?;
    let before = arr.len();
    arr.retain(|item| item_id(item) != Some(id));
    if arr.len() == before {
        bail!("no item with id = {}", id);
    }
    Ok(())
}

fn items_next_id(doc: &TomlValue, prefix: &str) -> Result<String> {
    if prefix.is_empty() {
        bail!("prefix must not be empty — use a letter like R, O, or A");
    }
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        bail!("prefix must not be all-digit — would collide with numeric-suffix parsing");
    }
    let mut max_n: u64 = 0;
    for item in items_array(doc, "items") {
        if let Some(id) = item_id(item)
            && let Some(rest) = id.strip_prefix(prefix)
            && let Ok(n) = rest.parse::<u64>()
            && n > max_n
        {
            max_n = n;
        }
    }
    Ok(format!("{}{}", prefix, max_n + 1))
}

/// Parse NDJSON input (one JSON value per line) into a `Vec<JsonValue>`. Blank
/// lines (after trimming) are skipped but counted in the 1-indexed line number
/// used in error messages, so `line N` here matches the source line the caller
/// typed.
///
/// The function is all-or-nothing: on the first malformed line it returns
/// `Err`, so the caller may rely on receiving either a fully parsed batch or
/// no rows at all. No side effects.
fn parse_ndjson(s: &str) -> Result<Vec<JsonValue>> {
    let mut rows = Vec::new();
    for (idx, line) in s.lines().enumerate() {
        let n = idx + 1;
        if line.trim().is_empty() {
            continue;
        }
        let v: JsonValue = serde_json::from_str(line)
            .with_context(|| format!("line {}", n))?;
        rows.push(v);
    }
    Ok(rows)
}

/// Append each row in `rows` to `array_name` inside `doc`, stamping fields
/// from `defaults` first (when `Some`) and shallow-merging per-row keys on
/// top (per-row wins on conflict). Each row must be a JSON object; an
/// array/scalar row is rejected with `row N: must be a JSON object`. Date
/// coercion for `DATE_KEYS` is inherited from `items_add_value_to` — this
/// helper does not reimplement it.
///
/// The batch aborts on the first bad row. No explicit rollback is needed:
/// the caller holds the file lock and all mutation is in-memory until the
/// outer `mutate_doc` persists. Returns the number of rows appended.
fn items_add_many(
    doc: &mut TomlValue,
    array_name: &str,
    rows: &[JsonValue],
    defaults: Option<&JsonValue>,
) -> Result<usize> {
    let defaults_obj = match defaults {
        Some(v) => Some(
            v.as_object()
                .ok_or_else(|| anyhow!("--defaults-json must be a JSON object"))?,
        ),
        None => None,
    };
    for (i, row) in rows.iter().enumerate() {
        let row_obj = row
            .as_object()
            .ok_or_else(|| anyhow!("row {}: must be a JSON object", i + 1))?;
        let mut merged = serde_json::Map::new();
        if let Some(d) = defaults_obj {
            for (k, v) in d.iter() {
                merged.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in row_obj.iter() {
            merged.insert(k.clone(), v.clone());
        }
        items_add_value_to(doc, &JsonValue::Object(merged), array_name)
            .with_context(|| format!("row {}", i + 1))?;
    }
    Ok(rows.len())
}

/// Append `rows` to `array_name` with no defaults. Thin wrapper over
/// `items_add_many` so the `array-append` dispatch site (Task 5) stays a
/// one-liner.
fn array_append(
    doc: &mut TomlValue,
    array_name: &str,
    rows: &[JsonValue],
) -> Result<usize> {
    items_add_many(doc, array_name, rows, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXT: &str = r#"slug = "x"
plan_path = "docs/plans/x.md"
status = "draft"
created = 2026-04-08
updated = 2026-04-08
scope = ["src/**"]

[tasks]
total = 3
completed = 0
in_progress = 0

[artifacts]
review_ledger = ".claude/flows/x/review-ledger.toml"
optimise_findings = ".claude/flows/x/optimise-findings.toml"
"#;

    const LEDGER: &str = r#"schema_version = 1
last_updated = 2026-04-16

[[items]]
id = "R1"
file = "src/a.rs"
line = 10
severity = "warning"
effort = "small"
category = "quality"
summary = "foo"
first_flagged = 2026-04-08
rounds = 1
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
line = 20
severity = "critical"
effort = "small"
category = "quality"
summary = "bar"
first_flagged = 2026-04-08
rounds = 1
status = "fixed"
resolved = 2026-04-08
resolution = "fix in abc123"
"#;

    fn ctx() -> TomlValue {
        toml::from_str(CONTEXT).unwrap()
    }
    fn led() -> TomlValue {
        toml::from_str(LEDGER).unwrap()
    }

    #[test]
    fn navigate_finds_nested_value() {
        let doc = ctx();
        assert_eq!(
            navigate(&doc, "tasks.total").and_then(|v| v.as_integer()),
            Some(3)
        );
        assert_eq!(
            navigate(&doc, "artifacts.review_ledger").and_then(|v| v.as_str()),
            Some(".claude/flows/x/review-ledger.toml")
        );
        assert!(navigate(&doc, "missing.path").is_none());
    }

    #[test]
    fn navigate_indexes_into_array_with_integer_segment() {
        // R49: `items.0.status` walks through the [[items]] array-of-tables,
        // selects index 0, and reads its `status`. Out-of-bounds yields None.
        let doc = led();
        let first_status = navigate(&doc, "items.0.status").and_then(|v| v.as_str());
        assert_eq!(first_status, Some("open"));
        let second_status = navigate(&doc, "items.1.status").and_then(|v| v.as_str());
        assert_eq!(second_status, Some("fixed"));
        // Out-of-bounds and non-numeric segments return None.
        assert!(navigate(&doc, "items.99.status").is_none());
        assert!(navigate(&doc, "items.oops.status").is_none());
    }

    #[test]
    fn set_at_path_preserves_unrelated_fields_and_created() {
        let mut doc = ctx();
        set_at_path(&mut doc, "status", TomlValue::String("review".into())).unwrap();
        set_at_path(&mut doc, "tasks.completed", TomlValue::Integer(2)).unwrap();
        assert_eq!(
            navigate(&doc, "status").and_then(|v| v.as_str()),
            Some("review")
        );
        assert_eq!(
            navigate(&doc, "tasks.completed").and_then(|v| v.as_integer()),
            Some(2)
        );
        assert_eq!(
            navigate(&doc, "created").and_then(|v| v.as_datetime()).map(|d| d.to_string()),
            Some("2026-04-08".into())
        );
        assert_eq!(
            navigate(&doc, "slug").and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[test]
    fn set_json_replaces_array() {
        let mut doc = ctx();
        let patch: JsonValue = serde_json::from_str(r#"["a/**", "b/**"]"#).unwrap();
        let v = json_to_toml(&patch).unwrap();
        set_at_path(&mut doc, "scope", v).unwrap();
        let scope: Vec<&str> = navigate(&doc, "scope")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(scope, vec!["a/**", "b/**"]);
    }

    #[test]
    fn infer_type_distinguishes_date_int_bool_string() {
        assert!(matches!(infer_type("2026-04-17"), ScalarType::Date));
        assert!(matches!(infer_type("42"), ScalarType::Int));
        assert!(matches!(infer_type("true"), ScalarType::Bool));
        assert!(matches!(infer_type("false"), ScalarType::Bool));
        assert!(matches!(infer_type("review"), ScalarType::Str));
        assert!(matches!(infer_type("2026-4-1"), ScalarType::Str));
    }

    #[test]
    fn items_list_filters_by_status() {
        let doc = led();
        let open = items_list(
            &doc,
            ListFilters {
                status: Some("open"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0]["id"], "R1");
        let fixed = items_list(
            &doc,
            ListFilters {
                status: Some("fixed"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fixed.len(), 1);
        assert_eq!(fixed[0]["id"], "R4");
    }

    #[test]
    fn items_add_promotes_iso_date_strings_to_datetime() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R5").unwrap();
        assert_eq!(item["first_flagged"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected raw TOML date literal, got:\n{serialised}"
        );
    }

    #[test]
    fn items_update_merges_patch() {
        let mut doc = led();
        items_update(
            &mut doc,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
            &[],
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "fixed");
        assert_eq!(item["rounds"], 2);
        assert_eq!(item["resolved"], "2026-04-17");
        assert_eq!(item["summary"], "foo", "unrelated field must be preserved");
    }

    #[test]
    fn items_remove_drops_matching_item() {
        let mut doc = led();
        items_remove(&mut doc, "R1").unwrap();
        assert!(items_get(&doc, "R1").is_err());
        assert!(items_get(&doc, "R4").is_ok());
        assert!(items_remove(&mut doc, "R999").is_err());
    }

    #[test]
    fn items_next_id_respects_max_and_prefix() {
        let doc = led();
        assert_eq!(items_next_id(&doc, "R").unwrap(), "R5");
        assert_eq!(items_next_id(&doc, "O").unwrap(), "O1");
    }

    #[test]
    fn items_next_id_rejects_empty_prefix() {
        let doc = led();
        let err = items_next_id(&doc, "").unwrap_err();
        assert!(
            err.to_string().contains("prefix must not be empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn items_next_id_rejects_numeric_prefix() {
        let doc = led();
        let err = items_next_id(&doc, "123").unwrap_err();
        assert!(
            err.to_string().contains("prefix must not be all-digit"),
            "unexpected error: {err}"
        );
        // Single digit should also be rejected.
        assert!(items_next_id(&doc, "1").is_err());
    }

    #[test]
    fn roundtrip_preserves_datetime_and_key_order() {
        let doc = ctx();
        let s = toml::to_string_pretty(&doc).unwrap();
        assert!(s.contains("created = 2026-04-08"));
        let slug_pos = s.find("slug =").unwrap();
        let status_pos = s.find("status =").unwrap();
        assert!(slug_pos < status_pos);
    }

    #[test]
    fn json_to_toml_rejects_null() {
        let v: JsonValue = serde_json::from_str("null").unwrap();
        assert!(json_to_toml(&v).is_err());
    }

    #[test]
    fn date_keys_roundtrip_as_toml_datetime() {
        // R45: exhaustive pin — every entry in DATE_KEYS must round-trip from
        // an ISO-date JSON string through `maybe_date_coerce` into a TOML
        // `Datetime`. If a key is removed from DATE_KEYS or mistyped, this
        // test fails with the offending key named in the assertion message.
        for key in DATE_KEYS {
            let v = JsonValue::String("2026-04-18".into());
            let coerced = maybe_date_coerce(key, &v)
                .unwrap_or_else(|e| panic!("{key}: coerce failed: {e}"));
            match coerced {
                TomlValue::Datetime(dt) => {
                    assert_eq!(dt.to_string(), "2026-04-18", "{key} produced wrong dt");
                }
                other => panic!("DATE_KEYS entry {key} did not coerce to Datetime: {other:?}"),
            }
        }
    }

    #[test]
    fn items_add_does_not_coerce_non_date_keys() {
        let mut doc = led();
        items_add(
            &mut doc,
            r#"{"id":"R99","file":"2026-04-17","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"file name shaped like a date","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        let item = items_get(&doc, "R99").unwrap();
        assert_eq!(item["file"], "2026-04-17");
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains(r#"file = "2026-04-17""#),
            "expected quoted string for non-date key, got:\n{serialised}"
        );
        assert!(
            serialised.contains("first_flagged = 2026-04-17"),
            "expected date literal for date key, got:\n{serialised}"
        );
    }

    #[test]
    fn read_json_arg_returns_literal_when_not_dash() {
        let got = read_json_arg(r#"{"key":"value"}"#).unwrap();
        assert_eq!(got, r#"{"key":"value"}"#);
    }

    #[test]
    fn items_apply_runs_batch_atomically() {
        let batch_ops = r#"[
            {"op":"add","json":{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}},
            {"op":"update","id":"R1","json":{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}},
            {"op":"remove","id":"R4"}
        ]"#;

        let mut doc_batch = led();
        items_apply(&mut doc_batch, batch_ops).unwrap();

        let mut doc_seq = led();
        items_add(
            &mut doc_seq,
            r#"{"id":"R5","file":"src/c.rs","line":1,"severity":"suggestion","effort":"trivial","category":"quality","summary":"baz","first_flagged":"2026-04-17","rounds":1,"status":"open"}"#,
        )
        .unwrap();
        items_update(
            &mut doc_seq,
            "R1",
            r#"{"status":"fixed","resolved":"2026-04-17","resolution":"fix in def456","rounds":2}"#,
            &[],
        )
        .unwrap();
        items_remove(&mut doc_seq, "R4").unwrap();

        let s_batch = toml::to_string_pretty(&doc_batch).unwrap();
        let s_seq = toml::to_string_pretty(&doc_seq).unwrap();
        assert_eq!(s_batch, s_seq);
    }

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
        // Scan up from the crate manifest to find the repo root (the one with
        // `claude/commands/*.md`). When `cargo test` runs from the tomlctl/
        // crate dir, `env!("CARGO_MANIFEST_DIR")` ends in `tomlctl` and its
        // parent is the dev-tools workspace.
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = crate_dir.parent().expect("repo root").to_path_buf();
        let cmd_dir = repo_root.join("claude").join("commands");
        let files = [
            cmd_dir.join("optimise.md"),
            cmd_dir.join("review.md"),
            cmd_dir.join("optimise-apply.md"),
            cmd_dir.join("review-apply.md"),
        ];

        // Only run the assertion when all four files exist. The test crate is
        // consumable in isolation; if it's packaged without the command tree,
        // degrade gracefully.
        if !files.iter().all(|p| p.exists()) {
            eprintln!(
                "blocks_verify_reproduces_shell_hashes: command files not found, skipping"
            );
            return;
        }

        let report = blocks_verify(
            &files,
            &[
                "flow-context".to_string(),
                "ledger-schema".to_string(),
            ],
        )
        .unwrap();
        assert!(report.ok, "shared blocks must be parity: {:?}", report.report);
        let blocks = report
            .report
            .get("blocks")
            .and_then(|v| v.as_array())
            .unwrap();
        let mut seen = std::collections::HashMap::new();
        for b in blocks {
            let name = b.get("name").and_then(|v| v.as_str()).unwrap();
            let hash = b.get("hash").and_then(|v| v.as_str()).unwrap();
            seen.insert(name.to_string(), hash.to_string());
        }
        assert_eq!(
            seen.get("flow-context").map(String::as_str),
            Some("efd5619a706fcc012f2c1741cea7318b210e155048625ca04be7e09401f274f2")
        );
        assert_eq!(
            seen.get("ledger-schema").map(String::as_str),
            Some("1aaae4568ad16a7cf382af2111fca9e0a4a8700d164619b08211c14b31b95db3")
        );
    }

    // ----- items update --unset -------------------------------------------

    #[test]
    fn items_update_unset_removes_field() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "deferred"
defer_reason = "blocked"
defer_trigger = "when channel lands"
summary = "something"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_update(
            &mut doc,
            "R1",
            r#"{"status":"open"}"#,
            &["defer_trigger".into(), "defer_reason".into()],
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "open");
        assert!(item.get("defer_reason").is_none());
        assert!(item.get("defer_trigger").is_none());
        assert_eq!(item["summary"], "something");

        // No-op for absent key is fine.
        items_update(
            &mut doc,
            "R1",
            r#"{}"#,
            &["nonexistent_key".into()],
        )
        .unwrap();
    }

    #[test]
    fn items_apply_unset_respected_in_batch() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "deferred"
defer_reason = "blocked"
defer_trigger = "when x lands"
summary = "foo"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_apply(
            &mut doc,
            r#"[{"op":"update","id":"R1","json":{"status":"open"},"unset":["defer_reason","defer_trigger"]}]"#,
        )
        .unwrap();
        let item = items_get(&doc, "R1").unwrap();
        assert_eq!(item["status"], "open");
        assert!(item.get("defer_reason").is_none());
        assert!(item.get("defer_trigger").is_none());

        // Missing `unset` in a batch op stays back-compat (no-op, no error).
        items_apply(
            &mut doc,
            r#"[{"op":"update","id":"R1","json":{"rounds":2}}]"#,
        )
        .unwrap();
    }

    // ----- items list filters ---------------------------------------------

    #[test]
    fn items_list_filters_combine_with_and() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
file = "src/a.rs"
category = "quality"
summary = "a"
first_flagged = 2026-04-05
status = "open"

[[items]]
id = "R2"
file = "src/b.rs"
category = "quality"
summary = "b"
first_flagged = 2026-04-15
status = "open"

[[items]]
id = "R3"
file = "src/b.rs"
category = "security"
summary = "c"
first_flagged = 2026-04-15
status = "open"

[[items]]
id = "R4"
file = "src/b.rs"
category = "quality"
summary = "d"
first_flagged = 2026-04-15
status = "fixed"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let threshold: toml::value::Datetime = "2026-04-10".parse().unwrap();
        let result = items_list(
            &doc,
            ListFilters {
                status: Some("open"),
                category: Some("quality"),
                newer_than: Some(&threshold),
                file_filter: None,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1, "expected exactly one item, got {result:?}");
        assert_eq!(result[0]["id"], "R2");
    }

    #[test]
    fn items_list_file_filter_matches_exactly() {
        let doc = led();
        let result = items_list(
            &doc,
            ListFilters {
                file_filter: Some("src/a.rs"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["id"], "R1");
    }

    #[test]
    fn items_list_newer_than_rejects_bad_date() {
        // Parsing is delegated to the CLI arg handler, which re-uses
        // `toml::value::Datetime::from_str`. Validate that directly.
        let err = "not-a-date".parse::<toml::value::Datetime>().unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    // ----- R1: items list --count -----------------------------------------

    #[test]
    fn items_list_count_matches_filter() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
status = "open"
summary = "a"

[[items]]
id = "R2"
status = "open"
summary = "b"

[[items]]
id = "R3"
status = "fixed"
summary = "c"
"#;
        let doc: TomlValue = toml::from_str(src).unwrap();
        let open = items_list(
            &doc,
            ListFilters {
                status: Some("open"),
                ..Default::default()
            },
        )
        .unwrap();
        // Simulate the dispatch wrapping: count == list.len() for the same filter.
        assert_eq!(open.len(), 2);
        // And a manual-count sanity check using a different filter.
        let fixed = items_list(
            &doc,
            ListFilters {
                status: Some("fixed"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(fixed.len(), 1);
    }

    // ----- R57: items add/update/remove/list/get --array ------------------

    #[test]
    fn items_add_to_custom_array_appends_without_touching_items() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        items_add_to(
            &mut doc,
            "rollback_events",
            r#"{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"test-R57","items":["R1"],"stash_ref":"stash@{0}"}"#,
        )
        .unwrap();

        // rollback_events has one entry; items untouched.
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].as_table().unwrap().get("cause").unwrap().as_str(),
            Some("test-R57")
        );
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn items_update_remove_list_get_honour_custom_array() {
        let src = r#"schema_version = 1

[[items]]
id = "I1"
status = "open"

[[audit]]
id = "A1"
status = "pending"
detail = "one"

[[audit]]
id = "A2"
status = "pending"
detail = "two"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();

        // items_list_from targets `audit` and skips `items`.
        let list = items_list_from(
            &doc,
            "audit",
            ListFilters {
                status: Some("pending"),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list.len(), 2);

        // items_get_from fetches by id from the named array.
        let got = items_get_from(&doc, "audit", "A1").unwrap();
        assert_eq!(got["detail"], "one");
        assert!(items_get_from(&doc, "audit", "I1").is_err());

        // items_update_to mutates the named array's record, not `items`.
        items_update_to(&mut doc, "audit", "A1", r#"{"status":"closed"}"#, &[]).unwrap();
        assert_eq!(items_get_from(&doc, "audit", "A1").unwrap()["status"], "closed");
        assert_eq!(items_get_from(&doc, "items", "I1").unwrap()["status"], "open");

        // items_remove_from drops from the named array only.
        items_remove_from(&mut doc, "audit", "A2").unwrap();
        let remaining_audit = doc.get("audit").and_then(|v| v.as_array()).unwrap();
        assert_eq!(remaining_audit.len(), 1);
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
    }

    // ----- R14: items apply --array ---------------------------------------

    #[test]
    fn items_apply_to_custom_array_appends_without_touching_items() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        let ops = r#"[{"op":"add","json":{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"test","items":["R1"],"stash_ref":"stash@{0}"}}]"#;
        items_apply_to(&mut doc, ops, "rollback_events").unwrap();

        // `rollback_events` now has one entry.
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .expect("rollback_events array");
        assert_eq!(events.len(), 1);
        let evt = events[0].as_table().unwrap();
        assert_eq!(evt.get("command").unwrap().as_str(), Some("review-apply"));
        assert_eq!(evt.get("cause").unwrap().as_str(), Some("test"));

        // [[items]] is untouched — still exactly the single pre-existing entry.
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].as_table().unwrap().get("id").unwrap().as_str(),
            Some("R1")
        );
    }

    // ----- R37: items apply --no-remove -----------------------------------

    #[test]
    fn items_apply_no_remove_rejects_remove_op() {
        let mut doc = led();
        // Without the flag, a remove op succeeds.
        items_apply(
            &mut doc,
            r#"[{"op":"remove","id":"R1"}]"#,
        )
        .unwrap();
        // Target reset.
        let mut doc2 = led();
        // With --no-remove, the same op errors before any mutation.
        let err = items_apply_to_opts(
            &mut doc2,
            r#"[
                {"op":"update","id":"R1","json":{"status":"fixed"}},
                {"op":"remove","id":"R4"}
            ]"#,
            "items",
            true,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("remove op"), "expected remove-op rejection, got: {msg}");
        assert!(msg.contains("op[1]"), "expected index in error, got: {msg}");
        // Confirm no partial mutation: R1 still `open`, R4 still present.
        assert_eq!(items_get(&doc2, "R1").unwrap()["status"], "open");
        assert!(items_get(&doc2, "R4").is_ok());
    }

    // ----- R19: items_next_id on empty doc --------------------------------

    #[test]
    fn items_next_id_on_empty_doc_returns_prefix_one() {
        // Stand-in for a ledger that exists but has no items yet. The
        // handler's pre-existence check in main.rs covers the "file missing"
        // case without invoking items_next_id at all; this test pins the
        // direct-call behaviour for an empty doc.
        let empty: TomlValue = toml::from_str("schema_version = 1\n").unwrap();
        assert_eq!(items_next_id(&empty, "R").unwrap(), "R1");
    }

    // ----- R54: stdin sentinel / lock contention / guard_write_path ------

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

    #[test]
    fn with_exclusive_lock_contention_times_out() {
        use std::sync::mpsc;
        use std::thread;
        use std::time::{Duration, Instant};

        let _guard = env_lock();
        // Short timeout so the test finishes quickly.
        unsafe {
            std::env::set_var("TOMLCTL_LOCK_TIMEOUT", "1");
        }

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("ledger.toml");
        fs::write(&target, LEDGER).unwrap();

        // Thread A takes the lock and sleeps long enough for thread B to
        // hit its own timeout.
        let (a_ready_tx, a_ready_rx) = mpsc::channel();
        let (b_done_tx, b_done_rx) = mpsc::channel();
        let target_a = target.clone();
        let a = thread::spawn(move || {
            with_exclusive_lock(&target_a, || {
                a_ready_tx.send(()).unwrap();
                // Hold the lock longer than B's timeout budget.
                thread::sleep(Duration::from_millis(3_000));
                Ok(())
            })
            .unwrap();
        });
        a_ready_rx.recv().unwrap();

        let target_b = target.clone();
        let b = thread::spawn(move || {
            let started = Instant::now();
            let res: Result<()> = with_exclusive_lock(&target_b, || Ok(()));
            b_done_tx.send(started.elapsed()).unwrap();
            res
        });

        let b_elapsed = b_done_rx.recv().unwrap();
        let b_res = b.join().unwrap();
        a.join().unwrap();

        unsafe {
            std::env::remove_var("TOMLCTL_LOCK_TIMEOUT");
        }

        assert!(b_res.is_err(), "thread B must time out under contention");
        // With a 1-second timeout we should be done well under 3s (the hold).
        assert!(
            b_elapsed < Duration::from_millis(2_500),
            "B took {:?}, expected < 2.5s under a 1s lock timeout",
            b_elapsed
        );
    }

    #[test]
    fn guard_write_path_rejects_outside_claude_by_default() {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        // Anchor containment at the tempdir so `.claude/` becomes tempdir/.claude.
        let canonical = dir.path().canonicalize().unwrap();
        unsafe {
            std::env::set_var("TOMLCTL_ROOT", canonical.as_os_str());
        }
        // Path outside `.claude/` — refused when allow_outside=false.
        let outside = canonical.join("outside.toml");
        fs::write(&outside, "x = 1\n").unwrap();
        let refused = guard_write_path(&outside, false);
        // With --allow-outside the same call succeeds.
        let allowed = guard_write_path(&outside, true);

        // Path inside `.claude/` — permitted.
        let inside_dir = canonical.join(".claude");
        fs::create_dir_all(&inside_dir).unwrap();
        let inside = inside_dir.join("ledger.toml");
        fs::write(&inside, "x = 1\n").unwrap();
        let inside_ok = guard_write_path(&inside, false);

        unsafe {
            std::env::remove_var("TOMLCTL_ROOT");
        }

        assert!(
            refused.is_err(),
            "path outside .claude/ must be refused without --allow-outside"
        );
        assert!(
            allowed.is_ok(),
            "path outside .claude/ must be permitted with --allow-outside"
        );
        assert!(
            inside_ok.is_ok(),
            "path inside .claude/ must be permitted without --allow-outside"
        );
    }

    // ----- Task 2: items add-many + array-append helpers ------------------

    #[test]
    fn items_add_many_merges_defaults() {
        let mut doc = led();
        let defaults: JsonValue = serde_json::from_str(
            r#"{"status":"open","rounds":1,"severity":"warning"}"#,
        )
        .unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"R10","file":"a.rs","line":1,"summary":"a","category":"quality","effort":"small","first_flagged":"2026-04-18"}"#).unwrap(),
            serde_json::from_str(r#"{"id":"R11","file":"b.rs","line":2,"summary":"b","category":"quality","effort":"small","first_flagged":"2026-04-18","severity":"critical"}"#).unwrap(),
        ];
        let n = items_add_many(&mut doc, "items", &rows, Some(&defaults)).unwrap();
        assert_eq!(n, 2);
        let r10 = items_get(&doc, "R10").unwrap();
        assert_eq!(r10["status"], "open");
        assert_eq!(r10["rounds"], 1);
        assert_eq!(r10["severity"], "warning");
        let r11 = items_get(&doc, "R11").unwrap();
        // Per-row severity wins over default.
        assert_eq!(r11["severity"], "critical");
        // Default still stamps non-conflicting fields.
        assert_eq!(r11["status"], "open");
    }

    #[test]
    fn items_add_many_rejects_non_object_row() {
        let mut doc = led();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"R10","file":"a.rs","line":1,"summary":"a","category":"quality","effort":"small","severity":"warning","first_flagged":"2026-04-18","rounds":1,"status":"open"}"#).unwrap(),
            serde_json::from_str(r#"[1,2]"#).unwrap(),
        ];
        let err = items_add_many(&mut doc, "items", &rows, None).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("row 2"),
            "expected error to mention row 2, got: {msg}"
        );
    }

    #[test]
    fn items_add_many_preserves_date_coercion_for_first_flagged() {
        let mut doc = led();
        let defaults: JsonValue = serde_json::from_str(
            r#"{"first_flagged":"2026-04-18","status":"open","rounds":1}"#,
        )
        .unwrap();
        let rows: Vec<JsonValue> = vec![serde_json::from_str(
            r#"{"id":"R20","file":"c.rs","line":3,"severity":"warning","effort":"small","category":"quality","summary":"c"}"#,
        )
        .unwrap()];
        items_add_many(&mut doc, "items", &rows, Some(&defaults)).unwrap();
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("first_flagged = 2026-04-18"),
            "expected raw TOML date literal for first_flagged, got:\n{serialised}"
        );
    }

    #[test]
    fn items_add_many_into_rollback_events_array() {
        let src = r#"schema_version = 1

[[items]]
id = "R1"
summary = "existing"
"#;
        let mut doc: TomlValue = toml::from_str(src).unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"timestamp":"2026-04-18T00:00:00Z","command":"review-apply","cause":"one","items":["R1"],"stash_ref":"stash@{0}"}"#).unwrap(),
            serde_json::from_str(r#"{"timestamp":"2026-04-18T00:01:00Z","command":"optimise-apply","cause":"two","items":["R2"],"stash_ref":"stash@{1}"}"#).unwrap(),
        ];
        let n = items_add_many(&mut doc, "rollback_events", &rows, None).unwrap();
        assert_eq!(n, 2);
        let events = doc
            .get("rollback_events")
            .and_then(|v| v.as_array())
            .expect("rollback_events array");
        assert_eq!(events.len(), 2);
        let first = events[0].as_table().unwrap();
        assert_eq!(first.get("cause").unwrap().as_str(), Some("one"));
        // `timestamp` is not in DATE_KEYS, so it stays a plain string (JSON
        // strings pass through `json_to_toml` as TOML strings). This pins
        // that rollback_events.timestamp is never date-coerced by this path.
        assert_eq!(
            first.get("timestamp").unwrap().as_str(),
            Some("2026-04-18T00:00:00Z")
        );
        // `items` array untouched.
        let items = doc.get("items").and_then(|v| v.as_array()).unwrap();
        assert_eq!(items.len(), 1);
        let serialised = toml::to_string_pretty(&doc).unwrap();
        assert!(
            serialised.contains("[[rollback_events]]"),
            "expected [[rollback_events]] header, got:\n{serialised}"
        );
    }

    #[test]
    fn array_append_matches_items_add_many_with_no_defaults() {
        let src = r#"schema_version = 1
"#;
        let mut doc_a: TomlValue = toml::from_str(src).unwrap();
        let mut doc_b: TomlValue = toml::from_str(src).unwrap();
        let rows: Vec<JsonValue> = vec![
            serde_json::from_str(r#"{"id":"E1","kind":"note"}"#).unwrap(),
            serde_json::from_str(r#"{"id":"E2","kind":"note"}"#).unwrap(),
        ];
        let n_a = array_append(&mut doc_a, "events", &rows).unwrap();
        let n_b = items_add_many(&mut doc_b, "events", &rows, None).unwrap();
        assert_eq!(n_a, n_b);
        assert_eq!(
            toml::to_string_pretty(&doc_a).unwrap(),
            toml::to_string_pretty(&doc_b).unwrap(),
            "array_append must be byte-identical to items_add_many(.., None)"
        );
    }

    #[test]
    fn parse_ndjson_reports_line_number_on_bad_json() {
        let input = "{\"id\":\"R1\"}\n{\"id\":\"R2\"}\n{not json\n";
        let err = parse_ndjson(input).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("line 3"), "expected 'line 3', got: {msg}");
    }

    #[test]
    fn parse_ndjson_skips_blank_lines_but_keeps_line_numbering() {
        // Line 1: valid, line 2: blank (skipped), line 3: malformed.
        // Error must still name line 3, not line 2.
        let input = "{\"id\":\"R1\"}\n\n{bad\n";
        let err = parse_ndjson(input).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("line 3"), "expected 'line 3', got: {msg}");

        // Happy path with a blank line in the middle: 2 rows out.
        let ok_input = "{\"id\":\"R1\"}\n\n{\"id\":\"R2\"}\n";
        let rows = parse_ndjson(ok_input).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["id"], "R1");
        assert_eq!(rows[1]["id"], "R2");
    }

    // Some of the tests above mutate process-wide env vars. Serialise them
    // against each other to avoid races when `cargo test` runs them in
    // parallel.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(|p| p.into_inner())
    }
}
