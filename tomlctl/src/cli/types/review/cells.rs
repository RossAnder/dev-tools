//! Arguments of the verbs that build and fill a run's cells: `survey`,
//! `plan`, `cover` and `add`.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::WriteIntegrityArgs;

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
    #[arg(
        long = "ruled-out",
        value_name = "SRC",
        help = "Sites the cell judged deliberate, one JSON object per line (`file`, optional `symbol` or `line`, `reason`), replacing the cell's earlier ones: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ruled_out: Option<String>,
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
        help = "The cell's full finding set, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}
