//! T1 stub for `tomlctl json {get,set,unset}` (implemented in T2 per
//! `docs/plans/flow-tracking-overhaul.md`).
//!
//! Plan deviation: the plan instructed use of
//! `tagged_err(ErrorKind::Other, None, "unimplemented")`, but
//! `ErrorKind::Other` is `#[allow(dead_code)]` in `errors.rs` with a
//! contract that it is never constructed directly — the JSON formatter
//! falls back to `kind=\"other\"` whenever the downcast misses. Using a
//! plain `anyhow::bail!` preserves that contract; T2 will replace these
//! stubs with real logic and use the appropriate tagged kind.

use anyhow::{Result, bail};

use crate::cli::JsonOp;

pub(crate) fn dispatch(op: JsonOp) -> Result<()> {
    match op {
        JsonOp::Get { .. } => bail!("json get: unimplemented"),
        JsonOp::Set { .. } => bail!("json set: unimplemented"),
        JsonOp::Unset { .. } => bail!("json unset: unimplemented"),
    }
}
