//! T1 stub. Real implementation lands in T9 per
//! `docs/plans/flow-tracking-overhaul.md`.

use anyhow::{Result, bail};

use crate::cli::ReadIntegrityArgs;

pub(crate) fn dispatch(
    _status: Option<String>,
    _branch: Option<String>,
    _active_only: bool,
    _json: bool,
    _integrity: ReadIntegrityArgs,
) -> Result<()> {
    bail!("flow list: unimplemented")
}
