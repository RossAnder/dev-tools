//! The `inputs` verb group.

use clap::Subcommand;

use super::{ReadIntegrityArgs, WriteIntegrityArgs};

/// `inputs` subcommand cluster. Every op resolves `.claude/inputs.toml` under
/// the repo root and emits JSON; a write never touches any other file.
#[derive(Subcommand)]
pub(crate) enum InputsOp {
    /// Print `{path, revision, inputs}`; every given filter must hold.
    List {
        #[arg(long, help = "Keep only `new` and `acknowledged` records")]
        pending: bool,
        #[arg(
            long = "kind",
            value_name = "KIND",
            help = "capture|request|note|question|answer (repeatable; any may match)"
        )]
        kind: Vec<String>,
        #[arg(long, help = "review|optimise|plan-review|backlog")]
        ledger: Option<String>,
        #[arg(long, value_name = "SLUG")]
        flow: Option<String>,
        #[arg(long, value_name = "SLUG")]
        scope: Option<String>,
        #[arg(long, value_name = "ID", help = "Keep records targeting this item id")]
        item: Option<String>,
        #[command(flatten)]
        integrity: ReadIntegrityArgs,
    },

    /// Append one record as `new` and print its assigned `{id}`.
    Add {
        #[arg(
            long,
            value_name = "JSON",
            help = "The record as a JSON object; pass `-` to read from stdin or `@<path>` to read a file"
        )]
        json: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Move `new` records to `acknowledged`; records past `new` are skipped.
    Ack {
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        #[arg(
            long,
            value_name = "COMMAND",
            help = "Command acknowledging the records"
        )]
        by: String,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Move `new` or `acknowledged` records to `handled`: positional ids
    /// sharing one `--note`, or `--ndjson` rows each carrying its own.
    Handle {
        #[arg(
            value_name = "ID",
            required_unless_present = "ndjson",
            conflicts_with = "ndjson"
        )]
        ids: Vec<String>,
        #[arg(
            long,
            value_name = "COMMAND",
            help = "Command that acted on the records"
        )]
        by: String,
        #[arg(
            long,
            required_unless_present = "ndjson",
            conflicts_with = "ndjson",
            help = "What was done, or why it was declined"
        )]
        note: Option<String>,
        #[arg(
            long = "ndjson",
            value_name = "SRC",
            help = "One {\"id\",\"note\"} object per line, applied in one write: `-` for stdin, otherwise a file path (a leading `@` is accepted)"
        )]
        ndjson: Option<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Withdraw `new` records, all or none; withdrawing an answer reopens
    /// the question it closed.
    Withdraw {
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },

    /// Answer a `new` question and mark it handled.
    Answer {
        #[arg(value_name = "QUESTION_ID")]
        question: String,
        #[arg(
            long = "pick",
            value_name = "OPTION",
            help = "Chosen option (repeatable)"
        )]
        pick: Vec<String>,
        #[arg(long, help = "Free-text answer")]
        text: Option<String>,
        #[command(flatten)]
        integrity: WriteIntegrityArgs,
    },
}
