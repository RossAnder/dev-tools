//! T1 stub: dispatch entrypoint. Each branch returns `unimplemented` until
//! its owning task lands.
//!
//! Plan deviation: the plan instructed use of
//! `tagged_err(ErrorKind::Other, None, "unimplemented")`, but `ErrorKind::Other`
//! is `#[allow(dead_code)]` in `errors.rs` with a contract that it is "never
//! constructed directly — the JSON formatter defaults to the `\"other\"`
//! string when the downcast returns `None`." Using a plain `anyhow::bail!`
//! preserves that contract: the JSON formatter still emits `kind=\"other\"`
//! via the downcast-miss fallback. Leaf tasks T2..T11 will replace these
//! stubs with real logic and use the appropriate tagged kind.

use anyhow::{Result, bail};

use crate::cli::{ActiveOp, FlowOp};

pub(crate) fn dispatch(op: FlowOp) -> Result<()> {
    match op {
        FlowOp::Active { op } => match op {
            ActiveOp::List { .. } => bail!("flow active list: unimplemented"),
            ActiveOp::Add { .. } => bail!("flow active add: unimplemented"),
            ActiveOp::Remove { .. } => bail!("flow active remove: unimplemented"),
            ActiveOp::Touch { .. } => bail!("flow active touch: unimplemented"),
        },
        FlowOp::FindPlans { .. } => bail!("flow find-plans: unimplemented"),
        FlowOp::Stale { .. } => bail!("flow stale: unimplemented"),
        FlowOp::Init { .. } => bail!("flow init: unimplemented"),
        FlowOp::EnsureArtifact { .. } => bail!("flow ensure-artifact: unimplemented"),
        FlowOp::Resolve { .. } => bail!("flow resolve: unimplemented"),
        FlowOp::Doctor { .. } => bail!("flow doctor: unimplemented"),
        FlowOp::List { .. } => bail!("flow list: unimplemented"),
    }
}
