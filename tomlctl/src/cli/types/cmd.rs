//! The `Cmd` subcommand enum, the small verb groups without a module of
//! their own (`json`, `integrity`, `blocks`, `agents`), and the legacy
//! shortcut adapter.

use clap::Subcommand;
use std::path::PathBuf;

use crate::convert::ScalarType;
use crate::sweep::{DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_HITS};

use super::{
    BacklogOp, FlowOp, InputsOp, ItemsOp, ReadIntegrityArgs, StampArgs, TasksOp, WriteIntegrityArgs,
};

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
    // `read` is the name agents reach for first; the alias stays out of help.
    #[command(alias = "read")]
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

    /// Set scalars at dotted key paths in one write: the positional pair
    /// and every `--set PATH=VALUE`. Types are inferred; `--type` forces
    /// the positional pair's type only.
    Set {
        file: PathBuf,
        // `required_unless_present` alone: pairing it with `required = true`
        // panics clap's debug asserts.
        #[arg(required_unless_present = "set", requires = "value")]
        path: Option<String>,
        #[arg(required_unless_present = "set", requires = "path")]
        value: Option<String>,
        #[arg(long = "type", value_enum, requires = "path")]
        ty: Option<ScalarType>,
        /// Set another scalar in the same write: PATH=VALUE (repeatable).
        #[arg(long = "set", value_name = "PATH=VALUE")]
        set: Vec<String>,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
        #[command(flatten)]
        stamp: StampArgs,
    },

    /// Set a JSON-encoded value (array, object, or scalar) at a dotted key path.
    SetJson {
        file: PathBuf,
        path: String,
        #[arg(
            long,
            help = "JSON-encoded value; pass `-` to read from stdin or `@<path>` to read a file"
        )]
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
        #[command(flatten)]
        stamp: StampArgs,
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
    /// `--json` and the field flags build a single object; `--ndjson`
    /// appends one per line (from stdin with `-` or from a file path).
    /// Primary use: append to `[[rollback_events]]` logs from
    /// `/review-apply` / `/optimise-apply` without constructing the
    /// `items apply` op-framing JSON.
    ArrayAppend {
        file: PathBuf,
        #[arg(help = "Array-of-tables name (e.g. rollback_events)")]
        array: String,
        #[arg(
            long,
            conflicts_with = "ndjson",
            help = "JSON object for a single record; pass `-` to read from stdin or `@<path>` to read a file. Field flags merge over it."
        )]
        json: Option<String>,
        #[arg(
            long = "ndjson",
            conflicts_with_all = ["json", "set", "set_json", "set_file"],
            help = "NDJSON source: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: Option<String>,
        #[command(flatten)]
        fields: crate::fields::FieldArgs,
        /// Preview the operation without writing. Emits a `would_change`
        /// summary on stdout and leaves the file + sidecar byte-identical.
        #[arg(
            long = "dry-run",
            help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
        #[command(flatten)]
        stamp: StampArgs,
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

    /// Per-flow task DAG over `.claude/flows/<slug>/tasks.toml` — import a
    /// plan's `## Tasks` section, add and update rows, query them with the
    /// shared `--where` surface, and compute ready-sets, Kahn batches and
    /// checkpoint closures from the stored `needs` / `coupling` edges.
    Tasks {
        #[command(subcommand)]
        op: TasksOp,
    },

    /// Agent lifecycle records over `.claude/flows/<slug>/agents.toml`,
    /// written by harness hooks as subagents start, idle and stop.
    Agents {
        #[command(subcommand)]
        op: AgentsOp,
    },

    /// The user-input store `.claude/inputs.toml` — captures, change requests
    /// and notes filed against the ledgers, agent questions and their answers.
    Inputs {
        #[command(subcommand)]
        op: InputsOp,
    },

    /// Regex hits over the repo's tracked files, as sorted `file:line`
    /// sites. Files come from `git ls-files` at the repo root (a tagged
    /// error outside a git tree); `.claude/**` and `docs/plans/**` are
    /// excluded by default because ledgers and plans quote patterns
    /// verbatim. The binary has no Unicode tables, so `\b` / `\w` / `\d`
    /// need the `(?-u:…)` form. Output carries `hits`, `files_scanned`,
    /// `skipped` counts, `truncated` and `coverage_complete`.
    Sweep {
        /// Pattern to search for; repeatable. Each hit records the index of
        /// the first pattern that matched its line.
        #[arg(short = 'e', long = "pattern", required = true, value_name = "REGEX")]
        pattern: Vec<String>,
        /// Files larger than this are skipped and counted under
        /// `skipped.oversize`.
        #[arg(long, default_value_t = DEFAULT_MAX_FILE_BYTES, value_name = "BYTES")]
        max_file_bytes: u64,
        /// Stop after this many distinct `file:line` sites and set
        /// `truncated: true` rather than erroring.
        #[arg(long, default_value_t = DEFAULT_MAX_HITS, value_name = "N")]
        max_hits: usize,
        /// Extra glob to exclude, on top of the defaults; repeatable.
        #[arg(long, value_name = "GLOB")]
        exclude: Vec<String>,
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

/// `agents` subcommand cluster. Each op resolves the flow's `agents.toml`
/// itself and emits JSON.
#[derive(Subcommand)]
pub(crate) enum AgentsOp {
    /// Record one hook payload: an agent starting, idling or stopping.
    Record {
        /// Harness that emitted the payload: claude-code|codex|manual.
        // Free-form rather than a `value_enum`, so an unknown name exits 1
        // through the `--error-format` envelope instead of clap usage prose.
        #[arg(long = "harness", value_name = "HARNESS")]
        harness: String,
        /// Hook payload JSON, or `-` for stdin.
        #[arg(value_name = "PAYLOAD", default_value = "-")]
        payload: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Print the flow's agent records as a JSON array.
    List {
        /// Flow slug whose `agents.toml` is read.
        #[arg(long = "slug", value_name = "SLUG")]
        slug: String,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
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

#[cfg(test)]
mod tests {
    use super::Cmd;
    use crate::cli::Cli;
    use clap::Parser;

    #[test]
    fn read_alias_parses_as_parse() {
        let parsed = crate::test_support::on_cli_stack(|| {
            Cli::try_parse_from(["tomlctl", "read", "f.toml"])
        });
        let Ok(cli) = parsed else {
            panic!("`tomlctl read f.toml` must parse");
        };
        assert!(matches!(cli.cmd, Cmd::Parse { ref file, .. } if file.as_os_str() == "f.toml"));
    }
}
