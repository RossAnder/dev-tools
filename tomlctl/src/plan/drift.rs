//! `plan drift`.

use anyhow::Result;

use crate::cli::PlanDriftArgs;
use crate::errors::{ErrorKind, tagged_err};

pub(super) fn run(_args: PlanDriftArgs) -> Result<()> {
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        "`plan drift` is not implemented",
    ))
}
