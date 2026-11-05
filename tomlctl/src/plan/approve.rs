//! `plan gate` and `plan approve`.

use anyhow::Result;

use crate::cli::{PlanApproveArgs, PlanGateArgs};
use crate::errors::{ErrorKind, tagged_err};

fn not_implemented(verb: &str) -> Result<()> {
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!("`plan {verb}` is not implemented"),
    ))
}

pub(super) fn gate(_args: PlanGateArgs) -> Result<()> {
    not_implemented("gate")
}

pub(super) fn approve(_args: PlanApproveArgs) -> Result<()> {
    not_implemented("approve")
}
