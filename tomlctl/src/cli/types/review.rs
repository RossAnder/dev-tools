//! The `review` verb group's store flags. The verb enum and each verb's
//! arguments live in `crate::review`, beside the code that runs them.

use clap::Args;

pub(crate) use crate::review::ReviewOp;

/// The store a verb reads or writes: `.claude/flows/<slug>/review-v2.toml`
/// or `.claude/review-v2/<scope>.toml`.
#[derive(Args, Clone)]
#[command(next_help_heading = "Store")]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewStore {
    #[arg(long = "slug", value_name = "SLUG", help = "Flow whose store is used")]
    pub(crate) slug: Option<String>,
    #[arg(
        long = "scope",
        value_name = "SCOPE",
        help = "Flow-less store `.claude/review-v2/<SCOPE>.toml`"
    )]
    pub(crate) scope: Option<String>,
}
