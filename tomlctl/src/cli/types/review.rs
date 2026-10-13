//! The `review` verb group over the v2 review store.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use super::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::fields::FieldArgs;

/// `review` subcommand cluster. Every verb but `catalogue` and `schema`
/// names its store with exactly one of `--slug` or `--scope`; the store
/// stamps its own `last_updated`, so no verb takes `--no-stamp`.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ReviewOp {
    /// Read every v1 ledger row carrying a `v2_ref` back into this store as a
    /// `sync` verdict and the status it maps to.
    #[command(name = "sync-v1")]
    SyncV1(ReviewSyncArgs),

    /// Start, patch or finish a review run.
    Run {
        #[command(subcommand)]
        op: ReviewRunOp,
    },

    /// Evaluate the vocabulary's stack, diff and health detectors and record
    /// each true signal with the directories it matched.
    Survey(ReviewSurveyArgs),

    /// Propose a coverage map (`--propose`), or append cells from NDJSON.
    Plan(ReviewPlanArgs),

    /// Record a cell's coverage status and note, sealing the cell.
    Cover(ReviewCoverArgs),

    /// Write one cell's full finding set, superseding its earlier unverified rows.
    Add(ReviewAddArgs),

    /// Upsert a verdict on (finding, `--by`, `--seat`), or a batch from NDJSON.
    Verdict(ReviewVerdictArgs),

    /// Resolve every finding whose `same_as` has complete verdicts.
    Resolve(ReviewResolveArgs),

    /// List findings, filtered, through a role projection.
    List(ReviewListArgs),

    /// Print findings or cells through a role projection.
    Show(ReviewShowArgs),

    /// A run's counts, coverage gaps, unverified ids and items awaiting a decision.
    Digest(ReviewDigestArgs),

    /// Write a run's markdown report and print its path and a counts line.
    Report(ReviewReportArgs),

    /// Disposition findings, moving only those still at `--expect-status`.
    Triage(ReviewTriageArgs),

    /// Yield, refute, audit and outcome rates per lens, probe or run. Writes nothing.
    Stats(ReviewStatsArgs),

    /// Export open defect and cleanup findings into a v1 ledger.
    #[command(name = "export-v1")]
    ExportV1(ReviewExportArgs),

    /// Merge the lens catalogue's layers and print its index.
    Catalogue(ReviewCatalogueArgs),

    /// Print a JSON Schema built from the field catalogue.
    Schema(ReviewSchemaArgs),
}

/// `review run` leaves.
#[derive(Subcommand)]
pub(crate) enum ReviewRunOp {
    /// Mint a run, validate the merged catalogue and create the run directory.
    Start(ReviewRunStartArgs),
    /// Patch a whitelist of run fields.
    Update(ReviewRunUpdateArgs),
    /// Stamp `finished` and compute `counts`, refusing an incomplete run.
    Finish(ReviewRunFinishArgs),
}

/// The store a verb reads or writes: `.claude/flows/<slug>/review-v2.toml`
/// or `.claude/review-v2/<scope>.toml`.
#[derive(Args, Clone)]
#[command(next_help_heading = "Store")]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewStore {
    #[arg(long = "slug", value_name = "SLUG", help = "Flow whose store is used")]
    pub(crate) slug: Option<String>,
    #[arg(
        long = "scope",
        value_name = "SCOPE",
        help = "Flow-less store `.claude/review-v2/<SCOPE>.toml`"
    )]
    pub(crate) scope: Option<String>,
}

/// `review stats`'s store choice, which adds `--all`.
#[derive(Args, Clone)]
#[command(next_help_heading = "Store")]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewStatsStore {
    #[arg(long = "slug", value_name = "SLUG", help = "Flow whose store is read")]
    pub(crate) slug: Option<String>,
    #[arg(
        long = "scope",
        value_name = "SCOPE",
        help = "Flow-less store `.claude/review-v2/<SCOPE>.toml`"
    )]
    pub(crate) scope: Option<String>,
    #[arg(long = "all", help = "Every flow store and every flow-less store")]
    pub(crate) all: bool,
}

#[derive(Args)]
pub(crate) struct ReviewSyncArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewRunStartArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "command",
        value_name = "COMMAND",
        help = "review-v2|optimise-v2|review-plan-v2"
    )]
    pub(crate) command: String,
    #[arg(long = "family", value_name = "FAMILY", help = "code|perf|plan")]
    pub(crate) family: String,
    #[arg(long = "depth", value_name = "DEPTH", help = "quick|standard|deep")]
    pub(crate) depth: String,
    #[arg(
        long = "base",
        value_name = "REV",
        help = "Commit the change is measured from; stored as its sha"
    )]
    pub(crate) base: String,
    #[arg(
        long = "builtin",
        value_name = "DIR",
        help = "Built-in catalogue root; defaults to <claude dir>/skills/review-v2-lenses"
    )]
    pub(crate) builtin: Option<PathBuf>,
    #[arg(
        long = "project",
        value_name = "DIR",
        help = "Project catalogue layer; defaults to .claude/review-lenses when present"
    )]
    pub(crate) project: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) fields: FieldArgs,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewRunUpdateArgs {
    #[arg(value_name = "RUN")]
    pub(crate) run: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[command(flatten)]
    pub(crate) fields: FieldArgs,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewRunFinishArgs {
    #[arg(value_name = "RUN")]
    pub(crate) run: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "incomplete",
        value_name = "REASON",
        conflicts_with = "incomplete_file",
        help = "Finish despite open gaps, recording why"
    )]
    pub(crate) incomplete: Option<String>,
    #[arg(
        long = "incomplete-file",
        value_name = "PATH",
        conflicts_with = "incomplete",
        help = "Read the --incomplete reason from a file"
    )]
    pub(crate) incomplete_file: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewSurveyArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

// The proposal-only flags conflict with `--ndjson` instead of requiring
// `--propose`: clap leaves a `requires` on a member of the required
// `plan_mode` group unenforced.
#[derive(Args)]
#[command(group(clap::ArgGroup::new("plan_mode").required(true).multiple(false).args(["propose", "ndjson"])))]
pub(crate) struct ReviewPlanArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[arg(long = "propose", help = "Print the default coverage map as NDJSON")]
    pub(crate) propose: bool,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "Cells to append, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: Option<String>,
    #[arg(
        long = "accept-signals",
        value_name = "A,B,...",
        value_delimiter = ',',
        conflicts_with = "ndjson",
        help = "Judged signals accepted for the proposal"
    )]
    pub(crate) accept_signals: Vec<String>,
    #[arg(
        long = "lens",
        value_name = "+ID|-ID",
        allow_hyphen_values = true,
        conflicts_with = "ndjson",
        help = "Force a lens into (+id) or out of (-id) the proposal (repeatable)"
    )]
    pub(crate) lens: Vec<String>,
    #[arg(
        long = "out",
        value_name = "PATH",
        conflicts_with = "ndjson",
        help = "Write the proposal to PATH instead of stdout"
    )]
    pub(crate) out: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewCoverArgs {
    #[arg(value_name = "CELL")]
    pub(crate) cell: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "status",
        value_name = "STATUS",
        help = "covered|thin|uncovered"
    )]
    pub(crate) status: String,
    #[arg(
        long = "note",
        value_name = "TEXT",
        required_unless_present = "note_file",
        conflicts_with = "note_file",
        help = "The coverage line; `-` reads stdin"
    )]
    pub(crate) note: Option<String>,
    #[arg(
        long = "note-file",
        value_name = "PATH",
        conflicts_with = "note",
        help = "Read the coverage line from a file"
    )]
    pub(crate) note_file: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewAddArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[arg(long = "cell", value_name = "CELL")]
    pub(crate) cell: String,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "The cell's full finding set, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

// The single-verdict flags conflict with `--ndjson`, whose rows carry their
// own. Citation and rationale are left to the verb, so a missing one is a
// `kind=validation` refusal rather than usage prose.
#[derive(Args)]
pub(crate) struct ReviewVerdictArgs {
    #[arg(
        value_name = "FINDING",
        required_unless_present = "ndjson",
        conflicts_with = "ndjson"
    )]
    pub(crate) finding: Option<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "verdict",
        value_name = "VERDICT",
        required_unless_present = "ndjson",
        conflicts_with = "ndjson",
        help = "confirmed|plausible|refuted|unsettled"
    )]
    pub(crate) verdict: Option<String>,
    #[arg(
        long = "by",
        value_name = "BY",
        required_unless_present = "ndjson",
        conflicts_with = "ndjson",
        help = "verifier|panel|checker|audit|orchestrator|sync"
    )]
    pub(crate) by: Option<String>,
    #[arg(
        long = "seat",
        value_name = "N",
        conflicts_with = "ndjson",
        help = "Panel vote 1-3"
    )]
    pub(crate) seat: Option<u32>,
    #[arg(
        long = "severity",
        value_name = "SEVERITY",
        conflicts_with = "ndjson",
        help = "Reassessed severity: critical|major|minor"
    )]
    pub(crate) severity: Option<String>,
    #[arg(
        long = "citation",
        value_name = "CITE",
        conflicts_with = "ndjson",
        help = "`file:line` or command output backing the verdict"
    )]
    pub(crate) citation: Option<String>,
    #[arg(
        long = "rationale",
        value_name = "TEXT",
        conflicts_with_all = ["rationale_file", "ndjson"],
        help = "Why; `-` reads stdin"
    )]
    pub(crate) rationale: Option<String>,
    #[arg(
        long = "rationale-file",
        value_name = "PATH",
        conflicts_with_all = ["rationale", "ndjson"],
        help = "Read the rationale from a file"
    )]
    pub(crate) rationale_file: Option<PathBuf>,
    #[arg(
        long = "strike-same-as",
        conflicts_with = "ndjson",
        help = "Clear the finding's `same_as`, so it resolves on its own verdict"
    )]
    pub(crate) strike_same_as: bool,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        help = "A batch of verdicts, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewResolveArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[arg(
        long = "wave",
        value_name = "N",
        help = "Only findings from cells of this wave"
    )]
    pub(crate) wave: Option<u32>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewListArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "kind", value_name = "KIND", help = "defect|cleanup|advisory")]
    pub(crate) kind: Option<String>,
    #[arg(long = "lens", value_name = "LENS", help = "Lens id or `probe:<slug>`")]
    pub(crate) lens: Option<String>,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: Option<String>,
    #[arg(long = "cell", value_name = "CELL")]
    pub(crate) cell: Option<String>,
    #[arg(
        long = "status",
        value_name = "STATUS",
        help = "unverified|superseded|open|refuted|merged|parked|dismissed|deferred|backlogged|converted|resolved|exported (default: every status except superseded)"
    )]
    pub(crate) status: Option<String>,
    #[arg(
        long = "route",
        value_name = "ROUTE",
        help = "verifier|panel|checker|none"
    )]
    pub(crate) route: Option<String>,
    #[arg(
        long = "in",
        value_name = "GLOB",
        help = "Keep findings whose file matches GLOB"
    )]
    pub(crate) in_glob: Option<String>,
    #[arg(
        long = "for",
        value_name = "ROLE",
        help = "finder|verifier|checker|implementer|orchestrator|audit|backlog|report (default: orchestrator)"
    )]
    pub(crate) for_role: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewShowArgs {
    #[arg(
        value_name = "ID,...",
        required = true,
        num_args = 1..,
        value_delimiter = ',',
        help = "Finding (F<n>) or cell (C<n>) ids, comma- or space-separated"
    )]
    pub(crate) ids: Vec<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "for",
        value_name = "ROLE",
        help = "finder|verifier|checker|implementer|orchestrator|audit|backlog|report"
    )]
    pub(crate) for_role: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewDigestArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[arg(
        long = "since-wave",
        value_name = "N",
        help = "Only rows from this wave onward"
    )]
    pub(crate) since_wave: Option<u32>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

// The read bundle, like `tasks render`: the report is derived markdown with
// no sidecar.
#[derive(Args)]
pub(crate) struct ReviewReportArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(long = "run", value_name = "RUN")]
    pub(crate) run: String,
    #[arg(
        long = "out",
        value_name = "PATH",
        help = "Where the report is written"
    )]
    pub(crate) out: PathBuf,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewTriageArgs {
    #[arg(
        value_name = "ID,...",
        required = true,
        num_args = 1..,
        value_delimiter = ',',
        help = "Finding ids, comma- or space-separated"
    )]
    pub(crate) ids: Vec<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[command(flatten)]
    pub(crate) mode: ReviewTriageMode,
    #[arg(
        long = "trigger",
        value_name = "TEXT",
        group = "trigger_src",
        conflicts_with = "trigger_file",
        conflicts_with_all = NOT_DEFER,
        help = "Companion to --defer: what reopens the finding"
    )]
    pub(crate) trigger: Option<String>,
    #[arg(
        long = "trigger-file",
        value_name = "PATH",
        group = "trigger_src",
        conflicts_with = "trigger",
        conflicts_with_all = NOT_DEFER,
        help = "Read the --defer trigger from a file"
    )]
    pub(crate) trigger_file: Option<PathBuf>,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        conflicts_with_all = NOT_CONVERT,
        help = "The converted finding as one JSON object: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: Option<String>,
    #[arg(
        long = "expect-status",
        value_name = "STATUS",
        help = "Move only the ids still at this status; the rest land under `skipped_stale`"
    )]
    pub(crate) expect_status: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

// A companion flag's `requires` on a member of the required mode group goes
// unenforced, so each companion instead conflicts with every other mode.
const NOT_DEFER: &[&str] = &[
    "dismiss",
    "dismiss_file",
    "reopen",
    "reopen_file",
    "park",
    "park_file",
    "resolve",
    "resolve_file",
    "backlog_ref",
    "convert",
];
const NOT_CONVERT: &[&str] = &[
    "dismiss",
    "dismiss_file",
    "defer",
    "defer_file",
    "reopen",
    "reopen_file",
    "park",
    "park_file",
    "resolve",
    "resolve_file",
    "backlog_ref",
];

/// The `review triage` dispositions, exactly one per call. Each prose
/// disposition has a `-file` twin so user text is staged in a file rather
/// than quoted through a shell.
#[derive(Args, Clone)]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewTriageMode {
    #[arg(long = "dismiss", value_name = "REASON", help = "Status → dismissed")]
    pub(crate) dismiss: Option<String>,
    #[arg(
        long = "dismiss-file",
        value_name = "PATH",
        help = "--dismiss with the reason read from a file"
    )]
    pub(crate) dismiss_file: Option<PathBuf>,
    #[arg(
        long = "defer",
        value_name = "REASON",
        requires = "trigger_src",
        help = "Status → deferred; takes --trigger"
    )]
    pub(crate) defer: Option<String>,
    #[arg(
        long = "defer-file",
        value_name = "PATH",
        requires = "trigger_src",
        help = "--defer with the reason read from a file"
    )]
    pub(crate) defer_file: Option<PathBuf>,
    #[arg(long = "reopen", value_name = "REASON", help = "Status → open")]
    pub(crate) reopen: Option<String>,
    #[arg(
        long = "reopen-file",
        value_name = "PATH",
        help = "--reopen with the reason read from a file"
    )]
    pub(crate) reopen_file: Option<PathBuf>,
    #[arg(long = "park", value_name = "REASON", help = "Status → parked")]
    pub(crate) park: Option<String>,
    #[arg(
        long = "park-file",
        value_name = "PATH",
        help = "--park with the reason read from a file"
    )]
    pub(crate) park_file: Option<PathBuf>,
    #[arg(
        long = "resolve",
        value_name = "RESOLUTION",
        help = "Status → resolved"
    )]
    pub(crate) resolve: Option<String>,
    #[arg(
        long = "resolve-file",
        value_name = "PATH",
        help = "--resolve with the resolution read from a file"
    )]
    pub(crate) resolve_file: Option<PathBuf>,
    #[arg(
        long = "backlog-ref",
        value_name = "B-ID",
        help = "Status → backlogged, recording the backlog item id"
    )]
    pub(crate) backlog_ref: Option<String>,
    #[arg(
        long = "convert",
        value_name = "KIND",
        requires = "ndjson",
        help = "cleanup|defect: mint the advisory's replacement from --ndjson; status → converted"
    )]
    pub(crate) convert: Option<String>,
}

#[derive(Args)]
pub(crate) struct ReviewStatsArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStatsStore,
    #[arg(long = "family", value_name = "FAMILY", help = "code|perf|plan")]
    pub(crate) family: Option<String>,
    #[arg(long = "by", value_name = "AXIS", help = "lens|probe|run")]
    pub(crate) by: Option<String>,
    #[arg(
        long = "outcomes",
        help = "Join exported findings to their v1 status through `v2_ref`"
    )]
    pub(crate) outcomes: bool,
    #[arg(
        long = "escapes",
        help = "Count findings minted where an earlier run's cell sealed `covered`"
    )]
    pub(crate) escapes: bool,
    #[arg(
        long = "calibration",
        help = "Exemplars from user dismissals, v1 wontfix reasons and audit overturns"
    )]
    pub(crate) calibration: bool,
    #[arg(
        long = "exemplars",
        value_name = "N",
        requires = "calibration",
        help = "Exemplars per area"
    )]
    pub(crate) exemplars: Option<usize>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewExportArgs {
    #[arg(
        value_name = "ID,...",
        num_args = 1..,
        value_delimiter = ',',
        required_unless_present = "all",
        conflicts_with = "all",
        help = "Finding ids, comma- or space-separated"
    )]
    pub(crate) ids: Vec<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "all",
        help = "Every open defect and cleanup finding, less needs-plan, test removals and plausible cleanup"
    )]
    pub(crate) all: bool,
    #[arg(long = "to", value_name = "LEDGER", help = "The v1 ledger written")]
    pub(crate) to: PathBuf,
    #[arg(
        long = "dry-run",
        help = "Preview the operation without writing. Emits a would_change summary; no file or sidecar touch."
    )]
    pub(crate) dry_run: bool,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewCatalogueArgs {
    #[arg(
        long = "builtin",
        value_name = "DIR",
        help = "Built-in catalogue root; defaults to <claude dir>/skills/review-v2-lenses"
    )]
    pub(crate) builtin: Option<PathBuf>,
    #[arg(
        long = "project",
        value_name = "DIR",
        help = "Project catalogue layer; defaults to .claude/review-lenses when present"
    )]
    pub(crate) project: Option<PathBuf>,
    #[arg(long = "family", value_name = "FAMILY", help = "code|perf|plan")]
    pub(crate) family: Option<String>,
    #[arg(
        long = "signals",
        value_name = "A,B,...",
        value_delimiter = ',',
        help = "Mark the entries these signals match"
    )]
    pub(crate) signals: Vec<String>,
    #[arg(long = "vocabulary", help = "Print the signals with their detectors")]
    pub(crate) vocabulary: bool,
    #[arg(long = "owners", help = "Print the concern-ownership map")]
    pub(crate) owners: bool,
    #[arg(long = "check", help = "Exit 1 on any validation error")]
    pub(crate) check: bool,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewSchemaArgs {
    #[arg(value_name = "TARGET", help = "finding|cell-return|verdict-return")]
    pub(crate) target: String,
    #[arg(long = "kind", value_name = "KIND", help = "defect|cleanup|advisory")]
    pub(crate) kind: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}
