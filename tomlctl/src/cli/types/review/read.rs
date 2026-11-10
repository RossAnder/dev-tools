//! Arguments of the read verbs: `list`, `show`, `digest`, `report` and
//! `stats`.

use std::path::PathBuf;

use clap::Args;

use super::{ReviewStatsStore, ReviewStore};
use crate::cli::types::ReadIntegrityArgs;

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
    #[arg(long = "fix-shape", value_name = "SHAPE", help = "edits|needs-plan")]
    pub(crate) fix_shape: Option<String>,
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
// no sidecar. `--allow-outside` is the one write-side flag it takes, for
// `--out`'s containment.
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
    #[arg(
        long = "allow-outside",
        help = "Write --out outside .claude/ and .review-v2-runs/"
    )]
    pub(crate) allow_outside: bool,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewStatsArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStatsStore,
    #[arg(long = "family", value_name = "FAMILY", help = "code|perf|plan")]
    pub(crate) family: Option<String>,
    #[arg(
        long = "by",
        value_name = "AXIS",
        help = "lens|probe|run|model; `model` groups audit overturns by the verifying role's model in the run's `models`"
    )]
    pub(crate) by: Option<String>,
    #[arg(
        long = "plan-outcomes",
        help = "Per plan run of a flow: tier by disposition, user-veto rate, edit growth, re-check yield and cost"
    )]
    pub(crate) plan_outcomes: bool,
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
