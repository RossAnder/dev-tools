//! `BacklogOp` fan-out to the per-verb leaf modules.

use anyhow::{Result, bail};

use crate::cli::BacklogOp;

/// Stub. The parser surface lands ahead of the leaves so `run`'s
/// catch-all-free match over `Cmd` stays exhaustive; the real fan-out
/// replaces this body and destructures `op` per verb.
pub(crate) fn dispatch(op: BacklogOp) -> Result<()> {
    let verb = match op {
        BacklogOp::Add { .. } => "add",
        BacklogOp::Check { .. } => "check",
        BacklogOp::List { .. } => "list",
        BacklogOp::Show { .. } => "show",
        BacklogOp::Relate { .. } => "relate",
        BacklogOp::Triage { .. } => "triage",
        BacklogOp::Cluster { .. } => "cluster",
        BacklogOp::Compact { .. } => "compact",
        BacklogOp::Evidence { .. } => "evidence",
    };
    bail!("`tomlctl backlog {verb}` is not yet implemented")
}
