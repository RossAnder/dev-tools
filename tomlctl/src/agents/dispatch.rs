//! Entry points for the `agents` verbs. They take plain arguments rather than
//! a clap variant, so the CLI layer only destructures and forwards.

use anyhow::{Context, Result, anyhow};

use super::schema::{self, Harness};
use crate::cli::{LinesArgs, ReadIntegrityArgs, WriteIntegrityArgs};
use crate::io;
use crate::output::{Rows, print_json_compact, print_report};

/// Record one hook payload and print the outcome on a single line, the form a
/// hook's log captures whole.
pub(crate) fn dispatch_record(
    harness: &str,
    payload_arg: &str,
    write_args: &WriteIntegrityArgs,
) -> Result<()> {
    let payload = io::read_json_value_from_arg(payload_arg).context("parsing PAYLOAD")?;
    print_json_compact(&record_value(harness, &payload, write_args)?)
}

/// Record one already-parsed hook payload under the named harness and return
/// the outcome the CLI prints. Shared by `agents record` and the library's
/// `record_agent`, so both reject an unknown harness with the same message.
pub(crate) fn record_value(
    harness: &str,
    payload: &serde_json::Value,
    write_args: &WriteIntegrityArgs,
) -> Result<serde_json::Value> {
    let harness = Harness::parse(harness).ok_or_else(|| {
        anyhow!(
            "agents record: unknown harness `{harness}` — expected one of {}",
            Harness::VOCABULARY.join(", ")
        )
    })?;
    super::record::record(harness, payload, write_args)
}

/// Print the flow's agent records as a JSON array.
pub(crate) fn dispatch_list(
    slug: &str,
    lines: LinesArgs,
    read_args: &ReadIntegrityArgs,
) -> Result<()> {
    let path = schema::agents_path(slug)?;
    print_report(super::list::rows(&path, read_args)?, lines.lines, Rows::Top)
}
