//! Arguments of `review verdict`, `review resolve`, `review reproduce` and
//! `review reproduce-status`.

use std::path::PathBuf;

use clap::Args;

use super::ReviewStore;
use crate::cli::types::{ReadIntegrityArgs, WriteIntegrityArgs};

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
        long = "unblocks",
        conflicts_with = "ndjson",
        help = "A verifier's answer to a blocked finding: clear its block and give it the fix sketch"
    )]
    pub(crate) unblocks: bool,
    #[arg(
        long = "fix-sketch",
        value_name = "TEXT",
        conflicts_with_all = ["fix_sketch_file", "ndjson"],
        help = "How to fix the unblocked finding; `-` reads stdin"
    )]
    pub(crate) fix_sketch: Option<String>,
    #[arg(
        long = "fix-sketch-file",
        value_name = "PATH",
        conflicts_with_all = ["fix_sketch", "ndjson"],
        help = "Read the fix sketch from a file"
    )]
    pub(crate) fix_sketch_file: Option<PathBuf>,
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
        help = "Only findings from cells of this wave; a `--same-as` duplicate resolves whatever its wave"
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

#[derive(Args)]
pub(crate) struct ReviewReproduceArgs {
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "ndjson",
        value_name = "SRC",
        required = true,
        help = "Reproductions, one JSON object per line (finding, flow, milestone, round, task, verdict, by, citation): `-` for stdin, otherwise a file path (a leading `@` is accepted)"
    )]
    pub(crate) ndjson: String,
    #[command(flatten)]
    pub(crate) integrity: WriteIntegrityArgs,
}

#[derive(Args)]
pub(crate) struct ReviewReproduceStatusArgs {
    #[arg(
        value_name = "ID,...",
        required_unless_present = "flow",
        num_args = 1..,
        value_delimiter = ',',
        help = "Finding ids, comma- or space-separated"
    )]
    pub(crate) ids: Vec<String>,
    #[command(flatten)]
    pub(crate) store: ReviewStore,
    #[arg(
        long = "flow",
        value_name = "SLUG",
        conflicts_with = "ids",
        requires = "milestone",
        help = "Select the findings the flow's rows of --milestone fix, from its [[finding_links]] for this store, in place of ids"
    )]
    pub(crate) flow: Option<String>,
    #[arg(
        long = "milestone",
        value_name = "ID",
        requires = "flow",
        help = "The checkpoint whose rows' fixed findings --flow selects"
    )]
    pub(crate) milestone: Option<String>,
    #[arg(
        long = "expect",
        value_name = "VERDICT",
        required = true,
        help = "gone|reproduces|unsettled"
    )]
    pub(crate) expect: String,
    #[command(flatten)]
    pub(crate) integrity: ReadIntegrityArgs,
}
