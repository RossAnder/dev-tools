//! The `review` verb group over the v2 review store.

use clap::{Args, Subcommand};

mod cells;
mod export;
mod read;
mod run;
mod triage;
mod verdicts;

pub(crate) use cells::*;
pub(crate) use export::*;
pub(crate) use read::*;
pub(crate) use run::*;
pub(crate) use triage::*;
pub(crate) use verdicts::*;

/// `review` subcommand cluster. Every verb but `catalogue` and `schema`
/// names its store with exactly one of `--slug` or `--scope`; the store
/// stamps its own `last_updated`, so no verb takes `--no-stamp`.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ReviewOp {
    /// Read every v1 ledger row carrying a `v2_ref` back into this store as a
    /// `sync` verdict and the status it maps to.
    #[command(name = "sync-v1")]
    SyncV1(ReviewSyncArgs),

    /// Start, patch, finish, list or show review runs.
    Run {
        #[command(subcommand)]
        op: ReviewRunOp,
    },

    /// Evaluate the vocabulary's stack, diff and health detectors and record
    /// each true signal with the directories it matched.
    Survey(ReviewSurveyArgs),

    /// Propose a coverage map (`--propose`), or append cells from NDJSON.
    Plan(ReviewPlanArgs),

    /// Record a cell's coverage status and note, sealing the cell.
    Cover(ReviewCoverArgs),

    /// Write one cell's full finding set, superseding its earlier unverified rows.
    Add(ReviewAddArgs),

    /// Upsert a verdict on (finding, `--by`, `--seat`), or a batch from NDJSON.
    Verdict(ReviewVerdictArgs),

    /// Resolve every finding whose `same_as` has complete verdicts.
    Resolve(ReviewResolveArgs),

    /// List findings, filtered, through a role projection.
    List(ReviewListArgs),

    /// Print findings or cells through a role projection.
    Show(ReviewShowArgs),

    /// A run's counts, coverage gaps, unverified ids and items awaiting a decision.
    Digest(ReviewDigestArgs),

    /// Write a run's markdown report and print its path and a counts line.
    Report(ReviewReportArgs),

    /// Disposition findings, moving only those still at `--expect-status`.
    Triage(ReviewTriageArgs),

    /// Write the record edits open plan findings on a v2 flow carry, by tier,
    /// each compare-and-set against its `expect`.
    Apply(ReviewApplyArgs),

    /// Yield, refute, audit and outcome rates per lens, probe or run. Writes nothing.
    Stats(ReviewStatsArgs),

    /// Export open defect and cleanup findings into a v1 ledger.
    #[command(name = "export-v1")]
    ExportV1(ReviewExportArgs),

    /// Merge the lens catalogue's layers and print its index.
    Catalogue(ReviewCatalogueArgs),

    /// Print a JSON Schema built from the field catalogue.
    Schema(ReviewSchemaArgs),
}

/// `review run` leaves.
#[derive(Subcommand)]
pub(crate) enum ReviewRunOp {
    /// Mint a run, validate the merged catalogue and create the run directory.
    Start(ReviewRunStartArgs),
    /// Patch a whitelist of run fields.
    Update(ReviewRunUpdateArgs),
    /// Stamp `finished` and compute `counts`, refusing an incomplete run.
    Finish(ReviewRunFinishArgs),
    /// List the store's runs, ordered by id. Writes nothing.
    List(ReviewRunListArgs),
    /// Print one run's bounds, signals (roots limited to its scope),
    /// baselines, waves, agents and audit. Writes nothing.
    Show(ReviewRunShowArgs),
}

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

/// `review stats`'s store choice, which adds `--all`.
#[derive(Args, Clone)]
#[command(next_help_heading = "Store")]
#[group(required = true, multiple = false)]
pub(crate) struct ReviewStatsStore {
    #[arg(long = "slug", value_name = "SLUG", help = "Flow whose store is read")]
    pub(crate) slug: Option<String>,
    #[arg(
        long = "scope",
        value_name = "SCOPE",
        help = "Flow-less store `.claude/review-v2/<SCOPE>.toml`"
    )]
    pub(crate) scope: Option<String>,
    #[arg(long = "all", help = "Every flow store and every flow-less store")]
    pub(crate) all: bool,
}
