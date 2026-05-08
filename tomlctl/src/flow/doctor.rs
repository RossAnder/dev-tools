//! T1 stub. Real implementation lands in T11 per
//! `docs/plans/flow-tracking-overhaul.md`.

use anyhow::{Result, bail};

use crate::cli::WriteIntegrityArgs;

pub(crate) fn dispatch(
    _slug: Option<String>,
    _fix: bool,
    _json: bool,
    _dry_run: bool,
    _integrity: WriteIntegrityArgs,
) -> Result<()> {
    bail!("flow doctor: unimplemented")
}
