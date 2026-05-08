//! T1 stub. Real implementation lands in T5 per
//! `docs/plans/flow-tracking-overhaul.md`.

use anyhow::{Result, bail};

use crate::cli::ReadIntegrityArgs;

pub(crate) fn dispatch(
    _slug: String,
    _threshold: String,
    _json: bool,
    _integrity: ReadIntegrityArgs,
) -> Result<()> {
    bail!("flow stale: unimplemented")
}
