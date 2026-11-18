//! The `review plan` flags and the dispatch between `--propose` and `--ndjson`.

use anyhow::Result;
use std::path::PathBuf;

use clap::Args;

use super::append::append;
use super::propose::propose;
use crate::cli::{ReviewStore, WriteIntegrityArgs};
use crate::review::invalid;
use crate::review::store::Target;

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
        help = "Cells to append, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
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
    #[arg(
        long = "context-free",
        conflicts_with = "ndjson",
        help = "Propose the context-free correctness cells too (standard and deep only)"
    )]
    pub(crate) context_free: bool,
    #[arg(
        long = "reserve",
        value_name = "N",
        default_value_t = 0,
        conflicts_with = "ndjson",
        help = "Leave N cells of the ceiling free for cells added after the proposal"
    )]
    pub(crate) reserve: usize,
    #[arg(
        long = "over-ceiling-file",
        value_name = "PATH",
        conflicts_with = "propose",
        help = "Append a wave past the run's ceiling, recording the user's reason read from PATH"
    )]
    pub(crate) over_ceiling_file: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

impl crate::review::Verb for ReviewPlanArgs {
    fn run(self) -> Result<()> {
        plan(&crate::review::store_target(&self.store)?, self)
    }
}

pub(crate) fn plan(target: &Target, args: ReviewPlanArgs) -> Result<()> {
    match (args.propose, args.ndjson.is_some()) {
        (true, false) => propose(target, &args),
        (false, true) => append(target, &args),
        _ => Err(invalid(
            "review plan: pass exactly one of --propose or --ndjson <SRC>",
        )),
    }
}
