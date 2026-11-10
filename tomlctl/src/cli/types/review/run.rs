//! `review run` leaf arguments.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::fields::FieldArgs;

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
        long = "head",
        value_name = "REV",
        help = "Commit the change is measured to, a descendant of --base; stored as its sha. Default: HEAD"
    )]
    pub(crate) head: Option<String>,
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
pub(crate) struct ReviewRunListArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "open",
        conflicts_with = "finished",
        help = "Only runs without `finished`"
    )]
    pub(crate) open: bool,
    #[arg(
        long = "finished",
        conflicts_with = "open",
        help = "Only runs with `finished`"
    )]
    pub(crate) finished: bool,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}
