//! The `flow` verb group: the registry, envelope and artifact subcommands.

use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;

use super::{ReadIntegrityArgs, StampArgs, WriteIntegrityArgs};
use crate::fields::FieldArgs;

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
    /// Append a validated entry to a flow's execution record.
    ///
    /// The record is the one `context.toml` `[artifacts].execution_record`
    /// names, else `.claude/flows/<slug>/execution-record.toml`. `date`
    /// defaults to today (UTC), `--task` fills `task_ref` from the task store,
    /// and the id is minted as `E<n>`. Every entry is checked against the
    /// execution-record contract: over-cap text is truncated, unsafe `files`
    /// entries are dropped, and both are listed in the output.
    Record {
        /// Flow slug whose execution record is written.
        #[arg(long = "slug")]
        slug: String,
        /// Entry type. Required unless `--ndjson` rows carry their own.
        #[arg(
            long = "type",
            value_name = "TYPE",
            value_enum,
            required_unless_present = "ndjson"
        )]
        record_type: Option<RecordType>,
        /// Task id in the flow's `tasks.toml`; its `ref` becomes `task_ref`.
        #[arg(long = "task", value_name = "ID")]
        task: Option<u32>,
        /// Base payload as a JSON object (`-` reads stdin, `@path` a file).
        /// Field flags are laid over it.
        #[arg(long = "json", value_name = "JSON")]
        json: Option<String>,
        #[command(flatten)]
        fields: FieldArgs,
        /// Append one entry per JSON line from SRC (`-` for stdin, or a
        /// path), all or nothing. The other payload flags apply to every row
        /// and each row's own keys win.
        #[arg(long = "ndjson", value_name = "SRC")]
        ndjson: Option<String>,
        /// Validate and report the ids that would be minted without writing.
        #[arg(long = "dry-run")]
        dry_run: bool,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
        #[command(flatten)]
        stamp: StampArgs,
    },
}

/// The execution-record entry types `flow record --type` accepts.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum RecordType {
    TaskCompletion,
    Verification,
    Deviation,
    Deferral,
    Reconcile,
    StatusTransition,
    Checkpoint,
}

impl RecordType {
    /// The `type` value written to the record, which is also the CLI spelling.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TaskCompletion => "task-completion",
            Self::Verification => "verification",
            Self::Deviation => "deviation",
            Self::Deferral => "deferral",
            Self::Reconcile => "reconcile",
            Self::StatusTransition => "status-transition",
            Self::Checkpoint => "checkpoint",
        }
    }
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
        /// optimise_findings, execution_record, plan_review_findings,
        /// tasks).
        #[arg(long = "require-artifact")]
        require_artifact: Vec<String>,
        /// Staleness threshold (default "7d").
        #[arg(long = "staleness-threshold", default_value = "7d")]
        staleness_threshold: String,
    },
}

/// Flow-artifact kinds surfaced by `flow ensure-artifact`. Variants are
/// rendered by clap's default `value_enum` casing as kebab-case.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ArtifactKind {
    Context,
    ExecutionRecord,
    ReviewLedger,
    OptimiseFindings,
    PlanReviewFindings,
    Tasks,
}
