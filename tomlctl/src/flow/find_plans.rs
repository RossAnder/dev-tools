//! T1 stub. Real implementation lands in T4 per
//! `docs/plans/flow-tracking-overhaul.md`.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::cli::ReadIntegrityArgs;

pub(crate) fn dispatch(
    _dirs: Vec<PathBuf>,
    _json: bool,
    _integrity: ReadIntegrityArgs,
) -> Result<()> {
    bail!("flow find-plans: unimplemented")
}
