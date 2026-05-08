//! T1 stub. Real implementation lands in T7 per
//! `docs/plans/flow-tracking-overhaul.md`.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::cli::WriteIntegrityArgs;

pub(crate) fn dispatch(
    _slug: String,
    _plan: PathBuf,
    _branch: Option<String>,
    _worktree: Option<PathBuf>,
    _scope: Vec<String>,
    _json: bool,
    _dry_run: bool,
    _integrity: WriteIntegrityArgs,
) -> Result<()> {
    bail!("flow init: unimplemented")
}
