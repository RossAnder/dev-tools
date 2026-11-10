//! Arguments of `review verdict` and `review resolve`.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::WriteIntegrityArgs;

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
        help = "A batch of verdicts, one JSON object per line: `-` for stdin, otherwise a file path (a leading `@` is accepted; a `.toml` path is a record file)"
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
    #[arg(
        long = "same-as",
        value_name = "DUP=TARGET",
        help = "Set the duplicate's `same_as` to the target and resolve the pair; repeatable"
    )]
    pub(crate) same_as: Vec<String>,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}
