//! `plan view`.

use anyhow::Result;

use crate::cli::PlanViewArgs;
use crate::errors::{ErrorKind, tagged_err};

pub(super) fn run(_args: PlanViewArgs) -> Result<()> {
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        "`plan view` is not implemented",
    ))
}
