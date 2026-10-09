//! The `backlog` verb group and its value enums.

use std::path::PathBuf;

use clap::{Args, Subcommand, ValueEnum};

use super::tasks::ids_help;
use super::{QueryArgs, ReadIntegrityArgs, WriteIntegrityArgs};

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
            long = "base-sha",
            value_name = "SHA",
            help = "Commit this capture was made against"
        )]
        base_sha: Option<String>,
        #[arg(
            long = "auto-base-sha",
            conflicts_with = "base_sha",
            // Intentionally compatible with --json: it resolves provenance from
            // the environment rather than supplying item content.
            help = "Resolve the base commit from `git rev-parse HEAD`"
        )]
        auto_base_sha: bool,
        #[arg(
            long = "on-duplicate",
            value_enum,
            default_value_t = OnDuplicate::Bump,
            help = "Behaviour when the computed dedup_id already exists"
        )]
        on_duplicate: OnDuplicate,
        #[arg(
            long,
            help = "Whole-item JSON payload instead of the field flags; pass `-` to read from stdin or `@<path>` to read a file"
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

    /// Capture a batch of discoveries, one `add --json` payload per NDJSON
    /// line, under one lock and one write. All-or-nothing: a malformed line,
    /// an unknown key or a refused row aborts before the store is touched.
    AddMany {
        #[arg(
            long = "ndjson",
            value_name = "SRC",
            help = "NDJSON source: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: String,
        #[arg(
            long = "auto-base-sha",
            help = "Resolve `base_sha` from `git rev-parse HEAD` for every row that carries none"
        )]
        auto_base_sha: bool,
        #[arg(
            long = "on-duplicate",
            value_enum,
            default_value_t = OnDuplicate::Bump,
            help = "Behaviour when a row's computed dedup_id already exists, including one an earlier row minted"
        )]
        on_duplicate: OnDuplicate,
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
    /// context, at most `--limit` candidates (default 5). With `--ndjson`,
    /// one verdict row per probe line under `results`, each capped at 5.
    Check {
        #[arg(
            long,
            required_unless_present = "ndjson",
            conflicts_with = "ndjson",
            help = "The discovery being weighed; pass `-` to read it from stdin"
        )]
        summary: Option<String>,
        /// Each line is `{"summary":…,"kind":…,"area":…,"tags":[…]}`, only
        /// `summary` required; the store is read once for the whole batch.
        #[arg(
            long = "ndjson",
            value_name = "SRC",
            help = "Batch of probes, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: Option<String>,
        #[arg(long, conflicts_with = "ndjson")]
        area: Option<String>,
        #[arg(long, conflicts_with = "ndjson")]
        kind: Option<String>,
        #[arg(
            long = "tag",
            value_name = "TAG",
            conflicts_with = "ndjson",
            help = "Free-form tag (repeatable)"
        )]
        tag: Vec<String>,
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
        #[arg(
            long,
            conflicts_with_all = ["open", "status"],
            help = "Only live rows: open or promoted"
        )]
        live: bool,
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
    /// evidence-directory listing, or an array of those objects in the order
    /// given; an unknown id fails the whole call.
    #[command(mut_args(ids_help("Item ids to print, comma- or space-separated")))]
    Show {
        #[command(flatten)]
        ids: ItemIds,
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
    #[command(mut_args(ids_help("Item ids to transition, comma- or space-separated")))]
    Triage {
        #[command(flatten)]
        ids: ItemIds,
        #[command(flatten)]
        mode: TriageMode,
        /// Flow slug or repo-relative plan path. Must name an existing flow
        /// or plan; a plan some flow binds is stored as that flow's slug.
        #[arg(long = "to", value_name = "REF")]
        to: Option<String>,
        #[arg(
            long,
            requires = "promote",
            help = "Store --to as `external:<REF>` without resolving it"
        )]
        external: bool,
        #[arg(
            long = "allow-closed",
            requires = "promote",
            help = "Accept a --to flow at `review` or `complete`"
        )]
        allow_closed: bool,
        #[arg(
            long,
            conflicts_with = "reason_file",
            value_name = "TEXT",
            help = "Companion to --dismiss"
        )]
        reason: Option<String>,
        #[arg(
            long = "reason-file",
            conflicts_with = "reason",
            value_name = "PATH",
            help = "Read the --dismiss companion from a file"
        )]
        reason_file: Option<PathBuf>,
        #[arg(
            long,
            conflicts_with = "resolution_file",
            value_name = "TEXT",
            help = "Companion to --resolve"
        )]
        resolution: Option<String>,
        #[arg(
            long = "resolution-file",
            conflicts_with = "resolution",
            value_name = "PATH",
            help = "Read the --resolve companion from a file"
        )]
        resolution_file: Option<PathBuf>,
        #[arg(
            long,
            group = "rationale_src",
            conflicts_with = "rationale_file",
            value_name = "TEXT",
            help = "Companion to --reopen"
        )]
        rationale: Option<String>,
        #[arg(
            long = "rationale-file",
            group = "rationale_src",
            conflicts_with = "rationale",
            value_name = "PATH",
            help = "Read the --reopen companion from a file"
        )]
        rationale_file: Option<PathBuf>,
        /// Move only the ids still at this status; the rest are reported
        /// under `skipped_stale` and left untouched.
        #[arg(long = "expect-status", value_name = "STATUS")]
        expect_status: Option<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Join each `promoted` item to the tasks that close it in its target
    /// flow's store, and bucket it as ready, in-progress, stalled, unlinked,
    /// orphaned, dangling or external. Writes nothing unless `--adopt` or
    /// `--apply` is given.
    Reconcile {
        #[arg(
            long,
            value_name = "SLUG",
            help = "Only items whose target resolves to this flow"
        )]
        flow: Option<String>,
        #[arg(
            long,
            help = "Resolve every ready item, recording its flow, tasks and commits"
        )]
        apply: bool,
        /// Runs before bucketing, so `--apply` in the same run sees the new
        /// links. They live only in `tasks.toml` until `tasks render` writes
        /// them into the plan, and the next `tasks import-plan` drops them
        /// otherwise; `render_needed` names the flows to render.
        #[arg(
            long,
            help = "Link each item no task links yet to every task in its flow whose prose names its id"
        )]
        adopt: bool,
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
            long = "per-tag",
            help = "Key the tags view on one tag per group instead of merging shared-tag groups transitively; an item may appear in several"
        )]
        per_tag: bool,
        #[arg(
            long = "all-statuses",
            help = "Cluster every item, not just `open` ones"
        )]
        all_statuses: bool,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Age resolved and dismissed items out of `[[backlog]]` into
    /// `[[compacted]]`. `open` and `promoted` items are never touched
    /// regardless of age.
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

/// Item ids taken positionally or through the hidden `--id` spelling. Read
/// them only through [`ItemIds::into_ids`]: the positional alone is empty on
/// an `--id`-only call. Each verb sets the positional's help via `ids_help`.
#[derive(Args, Clone)]
pub(crate) struct ItemIds {
    #[arg(
        required_unless_present = "id",
        num_args = 1..,
        value_delimiter = ',',
        value_name = "ID,..."
    )]
    ids: Vec<String>,
    /// Recovers the guessed flag spelling; appended to the positional ids.
    #[arg(
        long = "id",
        alias = "ids",
        hide = true,
        value_delimiter = ',',
        value_name = "ID,..."
    )]
    id: Vec<String>,
}

impl ItemIds {
    pub(crate) fn into_ids(self) -> Vec<String> {
        let mut ids = self.ids;
        ids.extend(self.id);
        ids
    }
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
    /// Clears the terminal date and companion. `--rationale` (or its file
    /// form) is enforced at the parser because `reopen_rationale` is the only
    /// companion field an `open` item is allowed to carry, so a bare
    /// `--reopen` would write an item the validator then rejects.
    #[arg(
        long,
        requires = "rationale_src",
        help = "Status → open; requires --rationale or --rationale-file"
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
    #[value(alias = "related")]
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
