//! T1 stub. Real implementation lands in T3 per
//! `docs/plans/flow-tracking-overhaul.md`. The dispatch fn here is the
//! single entrypoint the central `flow/dispatch.rs` calls; leaf tasks fill
//! it in (and any helper fns) without re-touching the central file.

use anyhow::{Result, bail};

use crate::cli::ActiveOp;

pub(crate) fn dispatch(_op: ActiveOp) -> Result<()> {
    bail!("flow active: unimplemented")
}
