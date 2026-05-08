//! T1 stub. Real implementation lands in T8 per
//! `docs/plans/flow-tracking-overhaul.md`.

use anyhow::{Result, bail};

use crate::cli::{ArtifactKind, WriteIntegrityArgs};

pub(crate) fn dispatch(
    _slug: String,
    _kind: ArtifactKind,
    _bootstrap: bool,
    _json: bool,
    _dry_run: bool,
    _integrity: WriteIntegrityArgs,
) -> Result<()> {
    bail!("flow ensure-artifact: unimplemented")
}
