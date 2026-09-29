//! Entry points for the `agents` verbs. They take plain arguments rather than
//! a clap variant, so the CLI layer only destructures and forwards.

use anyhow::{Context, Result, anyhow};

use super::schema::{self, Harness};
use crate::cli::{ReadIntegrityArgs, WriteIntegrityArgs};
use crate::io;
use crate::output::{print_json, print_json_compact};

/// Record one hook payload and print the outcome on a single line, the form a
/// hook's log captures whole.
pub(crate) fn dispatch_record(
    harness: &str,
    payload_arg: &str,
    write_args: &WriteIntegrityArgs,
) -> Result<()> {
    let harness = Harness::parse(harness).ok_or_else(|| {
        anyhow!(
            "agents record: unknown harness `{harness}` — expected one of {}",
            Harness::VOCABULARY.join(", ")
        )
    })?;
    let payload = io::read_json_value_from_arg(payload_arg).context("parsing PAYLOAD")?;
    let outcome = super::record::record(harness, &payload, write_args)?;
    print_json_compact(&outcome)
}

/// Print the flow's agent records as a JSON array.
pub(crate) fn dispatch_list(slug: &str, read_args: &ReadIntegrityArgs) -> Result<()> {
    let path = schema::agents_path(slug)?;
    print_json(&super::list::rows(&path, read_args)?)
}
