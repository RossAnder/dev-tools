//! `plan stage`.

use anyhow::Result;

use crate::cli::PlanStageArgs;
use crate::errors::{ErrorKind, tagged_err};

pub(super) fn run(_args: PlanStageArgs) -> Result<()> {
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        "`plan stage` is not implemented",
    ))
}
