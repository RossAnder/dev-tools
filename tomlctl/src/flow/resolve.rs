//! T1 stub. Real implementation lands in T10 per
//! `docs/plans/flow-tracking-overhaul.md`.

use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::cli::ReadIntegrityArgs;

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch(
    _flow: Option<String>,
    _path: Vec<PathBuf>,
    _branch: Option<String>,
    _worktree: Option<PathBuf>,
    _cwd: Option<PathBuf>,
    _with_staleness: bool,
    _json: bool,
    _integrity: ReadIntegrityArgs,
) -> Result<()> {
    bail!("flow resolve: unimplemented")
}
