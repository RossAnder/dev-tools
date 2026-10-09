//! The `items` verb group over ledger arrays-of-tables.

use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

use crate::dedup::DupTier;
use crate::fields::FieldArgs;
use crate::sweep::{DEFAULT_MAX_FILE_BYTES, DEFAULT_MAX_HITS};

use super::{QueryArgs, ReadIntegrityArgs, StampArgs, WriteIntegrityArgs};

/// What `items apply` does with an op whose `expect` precondition no longer
/// matches its row.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum OnStale {
    /// Fail the whole batch, naming every stale op; nothing is written.
    Abort,
    /// Drop the stale ops, apply the rest, and report the dropped ones.
    Skip,
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

    /// Append a new item. --json is the JSON object payload; the field flags
    /// merge over it and stand in for it when it is absent.
    Add {
        file: PathBuf,
        #[arg(
            long,
            required_unless_present_any = ["set", "set_json", "set_file"],
            help = "JSON object for the new item; pass `-` to read from stdin or `@<path>` to read a file. Optional when a field flag is given; field flags merge over it."
        )]
        json: Option<String>,
        #[command(flatten)]
        fields: FieldArgs,
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
        /// Mint the item's id as `<P><n+1>` inside the locked write and
        /// report it as `id`. The payload must not carry an `id`. On a
        /// `--dedupe-by` skip nothing is minted and `id` is the matched row's.
        #[arg(
            long = "id-prefix",
            value_name = "P",
            help = "Mint the id as <P><next> under the write lock and report it as `id`; the payload must not carry an id"
        )]
        id_prefix: Option<String>,
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

    /// Append many items in one batch from NDJSON. `--defaults-json` stamps
    /// common fields on every row (per-row keys win on conflict). One parse,
    /// one lock, one rewrite. On a malformed line N the batch aborts before
    /// mutating the file. Output: `{"ok":true,"added":N}`.
    AddMany {
        file: PathBuf,
        #[arg(
            long = "ndjson",
            help = "NDJSON source: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: String,
        #[arg(
            long = "defaults-json",
            help = "JSON object of default field values; pass `-` to read from stdin or `@<path>` to read a file"
        )]
        defaults_json: Option<String>,
        /// Target array-of-tables name. See `List --array`.
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
        /// Mint each added row's id as `<P><n+1>`, in input order, inside the
        /// locked write, and report them as `ids`. No row may carry an `id`;
        /// a `--dedupe-by` skip mints nothing.
        #[arg(
            long = "id-prefix",
            value_name = "P",
            help = "Mint each added row's id as <P><next> under the write lock and report them as `ids`; no row may carry an id"
        )]
        id_prefix: Option<String>,
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

    /// Merge fields into an existing item (matched by `id`). --json is a patch object.
    ///
    /// `--json`, the field flags and `--unset` are independently optional but
    /// at least one is required: any alone is valid and they compose (the
    /// field flags merge over `--json`, and the unset runs after the merge).
    /// Omitting both `--json` and the field flags defaults the patch to `{}`,
    /// whose merge loop runs zero times, so an unset-only update needs no
    /// placeholder patch object. Naming neither is refused in dispatch —
    /// it would otherwise be a no-op that still rewrote the file and its
    /// integrity sidecar.
    ///
    /// The "at least one" check is hand-rolled at the dispatch site rather
    /// than declared as a `required` `ArgGroup`, matching `array-append`'s
    /// `--json` / `--ndjson` pair: the `bail!` surfaces as an
    /// `--error-format json` envelope, a clap refusal as exit-2 usage prose.
    Update {
        file: PathBuf,
        id: String,
        #[arg(
            long,
            help = "JSON patch object merged into the item; pass `-` to read from stdin or `@<path>` to read a file. Optional when --unset or a field flag is given; field flags merge over it."
        )]
        json: Option<String>,
        #[command(flatten)]
        fields: FieldArgs,
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
        #[command(flatten)]
        stamp: StampArgs,
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
        #[command(flatten)]
        stamp: StampArgs,
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
            help = "JSON array of ops, or NDJSON with one op object per line; each op is `{\"op\":\"add|update|remove\", ...}`; pass `-` to read from stdin or `@<path>` to read a file"
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
        #[arg(
            long = "on-stale",
            value_enum,
            default_value_t = OnStale::Abort,
            help = "What to do with an update/remove op whose `expect` no longer matches its row: abort the batch, or skip the op and list it under skipped_stale"
        )]
        on_stale: OnStale,
        /// Preview the batch without writing. Runs every validation
        /// gate (`--no-remove`, op-shape, missing-id, dedup_id auto-populate)
        /// so an agent can rehearse the batch shape before committing.
        /// Emits `{"ok":true,"dry_run":true,"would_change":{...},"skipped_stale":[...]}`.
        #[arg(
            long = "dry-run",
            help = "Preview the batch without writing. Emits a would_change summary; no file or sidecar touch."
        )]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
        #[command(flatten)]
        stamp: StampArgs,
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
        #[arg(long, value_enum, ignore_case = true, default_value_t = DupTier::A)]
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

    /// Print the tier-B `dedup_id` of one stored item, alongside the five
    /// fingerprinted field values that fed it.
    ///
    /// `find-duplicates --tier B` drops every group of fewer than two
    /// members, so it reports nothing for a unique row; this is the path
    /// that observes such a row's digest. Read-only — the ledger is never
    /// rewritten, so a stored `dedup_id` that has gone stale still shows
    /// its recomputed value here.
    Fingerprint {
        file: PathBuf,
        id: String,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Surface items whose file or symbol has drifted, or whose depends_on
    /// points at an id that isn't in the ledger. Classes: `missing-file`,
    /// `symbol-missing`, `io-error`, `outside-repo`, `dangling-dep`, and
    /// `instance-missing` for an `instances` anchor that does not resolve —
    /// its `reason` is one of `missing-file`, `symbol-missing`, `io-error`,
    /// `outside-repo`, `unparseable`.
    Orphans {
        file: PathBuf,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Re-run each item's stored `sweep` patterns over the tracked files
    /// and diff the hits against its `instances`. Per item: `new` sites
    /// (`file:line`), `gone` anchors with a `reason`, `kept`, `unverified`
    /// anchors with a `reason` (`unparseable`, `outside-repo`, `truncated`,
    /// `missing`, `skipped` or `excluded`), plus `recorded` / `found` file
    /// sets. Items without a `sweep` array land in `skipped_items`. The
    /// ledger itself is excluded from its own sweep. Read-only unless
    /// `--update` rewrites each `open` item's `instances`: entries keep
    /// their listed order, with `kept` and `excluded` anchors retained in
    /// place, then the `new` sites are appended as `file:line`;
    /// `enumeration` is never touched. `--update` refuses for a
    /// `truncated` run or an `unverified` anchor whose reason is not
    /// `excluded`.
    Sweep {
        file: PathBuf,
        /// Item ids to sweep. Omit for every item.
        #[arg(long, value_delimiter = ',', value_name = "R1,R7,...")]
        ids: Vec<String>,
        /// Rewrite each swept item's `instances` from the results. Without
        /// it nothing is written and the file is never created.
        #[arg(long)]
        update: bool,
        /// Preview the `--update` rewrite without writing. Emits a
        /// `would_change` summary; no file or sidecar touch.
        #[arg(long = "dry-run")]
        dry_run: bool,
        /// Files larger than this are skipped, which leaves their anchors
        /// `unverified`.
        #[arg(long, default_value_t = DEFAULT_MAX_FILE_BYTES, value_name = "BYTES")]
        max_file_bytes: u64,
        /// Distinct `file:line` sites after which the sweep stops and every
        /// item reports `truncated: true`.
        #[arg(long, default_value_t = DEFAULT_MAX_HITS, value_name = "N")]
        max_hits: usize,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
        #[command(flatten)]
        stamp: StampArgs,
    },

    /// Group selected items into file-disjoint clusters and order the
    /// clusters into dependency batches. An item's files are its `file`
    /// plus the files of its `instances`; items are layered over
    /// `depends_on` first, so two items sharing a file join one cluster
    /// only within a layer. `depends_on` targets outside the selection are
    /// dropped and listed under `dropped_deps`; a cycle is refused. Each
    /// cluster carries `lite_file_scope`: at most two files, or a single
    /// item whose `enumeration` is `complete`.
    Clusters {
        file: PathBuf,
        /// Item ids to cluster. Omit for every item not at a terminal
        /// status (an absent status reads as `open`).
        #[arg(long, value_delimiter = ',', value_name = "R1,R7,...")]
        ids: Vec<String>,
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
        #[command(flatten)]
        stamp: StampArgs,
    },
}
