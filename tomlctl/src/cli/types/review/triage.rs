//! Arguments of `review triage`, `review settle` and `review apply`.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::WriteIntegrityArgs;

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
        help = "The converted finding as one JSON object: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ndjson: Option<String>,
    #[arg(
        long = "run",
        value_name = "RUN",
        conflicts_with_all = NOT_CONVERT,
        help = "Companion to --convert: the open run the replacement is minted in when the advisory's run is finished"
    )]
    pub(crate) run: Option<String>,
    #[arg(
        long = "targets",
        value_name = "TARGET,...",
        value_delimiter = ',',
        conflicts_with_all = NOT_ADDRESSED,
        help = "Companion to --addressed: the plan targets the hand edit changed; defaults to each finding's own target"
    )]
    pub(crate) targets: Vec<String>,
    #[arg(
        long = "expect-status",
        value_name = "STATUS",
        help = "Move only the ids still at this status; the rest land under `skipped_stale`"
    )]
    pub(crate) expect_status: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewSettleArgs {
    #[arg(value_name = "ID", help = "The finding blocked on evidence")]
    pub(crate) id: String,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[command(flatten)]
    pub(crate) outcome: ReviewSettleOutcome,
    #[arg(
        long = "tier",
        value_name = "TIER",
        conflicts_with_all = ["refute", "defer"],
        help = "Companion to --accept: mechanical|cited|user"
    )]
    pub(crate) tier: Option<String>,
    #[arg(
        long = "fix-file",
        value_name = "PATH",
        conflicts_with_all = ["refute", "defer"],
        help = "Companion to --accept: the fix the proposal settled on, recorded as `fix_sketch`"
    )]
    pub(crate) fix_file: Option<PathBuf>,
    #[arg(
        long = "citation",
        value_name = "TEXT",
        conflicts_with_all = ["accept", "defer"],
        help = "Companion to --refute: what shows the finding false"
    )]
    pub(crate) citation: Option<String>,
    #[arg(
        long = "proposal",
        value_name = "PATH",
        conflicts_with_all = ["accept", "refute"],
        help = "Companion to --defer: the proposal note, a file in the repository"
    )]
    pub(crate) proposal: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

/// The `review settle` outcomes, exactly one per call.
#[derive(Args, Clone)]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewSettleOutcome {
    #[arg(
        long = "accept",
        requires_all = ["tier", "fix_file"],
        help = "Unblock the finding with the proposal's fix; status → open"
    )]
    pub(crate) accept: bool,
    #[arg(
        long = "refute",
        requires = "citation",
        help = "Refute the finding through an orchestrator verdict; status → refuted"
    )]
    pub(crate) refute: bool,
    #[arg(
        long = "defer",
        requires = "proposal",
        help = "Record the proposal note and leave the finding blocked; status → open"
    )]
    pub(crate) defer: bool,
}

#[derive(Args)]
pub(crate) struct ReviewApplyArgs {
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
    #[arg(
        long = "user-accepted",
        help = "The user accepted these edits: admits user-tier rows and an approved plan, and records `by = user`"
    )]
    pub(crate) user_accepted: bool,
    #[arg(
        long = "dry-run",
        help = "Preview the operation without writing. Emits the envelope with dry_run: true; no file or sidecar touch."
    )]
    pub(crate) dry_run: bool,
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
    "answer_file",
    "addressed",
    "addressed_file",
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
    "answer_file",
    "addressed",
    "addressed_file",
];
const NOT_ADDRESSED: &[&str] = &[
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
    "convert",
    "answer_file",
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
    #[arg(
        long = "answer-file",
        value_name = "PATH",
        help = "Answer an open finding blocked on a decision with the text of a file; clears the block, keeping its options"
    )]
    pub(crate) answer_file: Option<PathBuf>,
    #[arg(
        long = "addressed",
        value_name = "TEXT",
        help = "Status → resolved: the author fixed the plan finding by hand; records --targets as `addressed`"
    )]
    pub(crate) addressed: Option<String>,
    #[arg(
        long = "addressed-file",
        value_name = "PATH",
        help = "--addressed with the text read from a file"
    )]
    pub(crate) addressed_file: Option<PathBuf>,
}
