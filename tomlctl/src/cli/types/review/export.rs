//! Arguments of `review sync-v1`, `review export-v1`, `review schedule`,
//! `review reconcile`, `review catalogue` and `review schema`.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::{ReadIntegrityArgs, WriteIntegrityArgs};

#[derive(Args)]
pub(crate) struct ReviewSyncArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
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
        help = "Every open defect and cleanup finding, less blocked findings, test removals and plausible cleanup"
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
pub(crate) struct ReviewScheduleArgs {
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
        help = "Every open code or perf defect and cleanup, less blocked findings, test removals and plausible cleanup"
    )]
    pub(crate) all: bool,
    #[arg(
        long = "flow",
        value_name = "SLUG",
        help = "The v2 flow whose tasks.toml receives the fix tasks"
    )]
    pub(crate) flow: String,
    #[arg(
        long = "checkpoint",
        value_name = "ID",
        help = "The checkpoint group every minted task joins"
    )]
    pub(crate) checkpoint: String,
    #[arg(
        long = "settle-checkpoint",
        value_name = "ID",
        help = "The checkpoint group a settle task joins; without it an evidence-blocked finding is refused"
    )]
    pub(crate) settle_checkpoint: Option<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewReconcileArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "flow",
        value_name = "SLUG",
        help = "The v2 flow whose tasks.toml holds the scheduled findings' tasks"
    )]
    pub(crate) flow: String,
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
