//! `tomlctl review` — the v2 review store over `.claude/flows/<slug>/review-v2.toml`
//! or `.claude/review-v2/<scope>.toml`, one module per substrate or verb.
//!
//! A verb's clap arguments live in its own module beside the code that runs
//! them, and its args type implements [`Verb`]. Registering a verb is its
//! `mod` line, its [`ReviewOp`] variant and its one-line arm in [`dispatch`].

use anyhow::Result;
use clap::Subcommand;

use crate::cli::ReviewStore;

pub(crate) mod catalogue;
mod fields;
mod matching;
mod store;

mod add;
mod anchor;
mod apply;
mod catalogue_cmd;
mod cells;
mod cover;
mod digest;
mod export;
pub(crate) mod facade;
mod query;
mod recheck;
mod reconcile;
mod reproduce;
mod resolve;
mod run_show;
mod runs;
mod schedule;
mod schema_out;
mod select;
mod settle;
mod stats;
mod survey;
mod sync;
mod target;
mod tier;
mod triage;
mod verdict;

// The vocabularies review verbs check their flags against after parsing, for
// `capabilities` to publish.
pub(crate) use cover::SEAL_STATUSES;
pub(crate) use fields::{
    COMMANDS, Depth, FINDING_STATUSES, Family, Kind, ReproduceVerdict, Role, Route, SETTLE_TIERS,
    Severity, VERDICT_BY, VERDICTS,
};
pub(crate) use query::BLOCKED_ON_FILTERS;
pub(crate) use schema_out::TARGETS as SCHEMA_TARGETS;
pub(crate) use stats::AXES as STATS_AXES;
pub(crate) use triage::CONVERT_KINDS;
// The entry points for readers outside the review group, so that no field
// name of the store is spelled outside it.
pub(crate) use store::{implementer_findings, plan_family, reproduce_state};
pub(crate) use tier::Tier;

/// `review` subcommand cluster. Every verb but `catalogue` and `schema`
/// names its store with exactly one of `--slug` or `--scope`; the store
/// stamps its own `last_updated`, so no verb takes `--no-stamp`.
#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ReviewOp {
    /// Read every v1 ledger row carrying a `v2_ref` back into this store as a
    /// `sync` verdict and the status it maps to.
    #[command(name = "sync-v1")]
    SyncV1(sync::ReviewSyncArgs),

    /// Start, patch, finish, list or show review runs.
    Run {
        #[command(subcommand)]
        op: runs::ReviewRunOp,
    },

    /// Evaluate the vocabulary's stack, diff and health detectors and record
    /// each true signal with the directories it matched.
    Survey(survey::ReviewSurveyArgs),

    /// Propose a coverage map (`--propose`), or append cells from NDJSON.
    Plan(cells::ReviewPlanArgs),

    /// Record a cell's coverage status and note, sealing the cell.
    Cover(cover::ReviewCoverArgs),

    /// Write one cell's full finding set, superseding its earlier unverified rows.
    Add(add::ReviewAddArgs),

    /// Upsert a verdict on (finding, `--by`, `--seat`), or a batch from NDJSON.
    Verdict(verdict::ReviewVerdictArgs),

    /// Resolve every finding whose `same_as` has complete verdicts, after linking each `--same-as` pair.
    Resolve(resolve::ReviewResolveArgs),

    /// Record reproduce verdicts on findings from NDJSON, upserted on
    /// (finding, flow, milestone, round, by). Never changes a finding's status.
    Reproduce(reproduce::ReviewReproduceArgs),

    /// Exit 0 when each named finding's current reproduction has the
    /// `--expect` verdict, 1 otherwise. Writes nothing.
    #[command(name = "reproduce-status")]
    ReproduceStatus(reproduce::ReviewReproduceStatusArgs),

    /// List findings, filtered, through a role projection.
    List(query::ReviewListArgs),

    /// Print findings or cells through a role projection.
    Show(query::ReviewShowArgs),

    /// Exit 0 when a finding's anchor holds in the working tree, 1 when it
    /// does not. Writes nothing.
    Anchor(anchor::ReviewAnchorArgs),

    /// A run's counts, coverage gaps, unverified ids and items awaiting a decision.
    Digest(digest::ReviewDigestArgs),

    /// The sorted distinct plan targets a re-check of a run covers: every
    /// `applied.target` and `addressed` entry of its findings. Writes nothing.
    #[command(name = "recheck-targets")]
    RecheckTargets(recheck::ReviewRecheckTargetsArgs),

    /// Write a run's markdown report and print its path and a counts line.
    Report(digest::ReviewReportArgs),

    /// Disposition findings, moving only those still at `--expect-status`.
    Triage(triage::ReviewTriageArgs),

    /// Record what became of a settle proposal for a finding blocked on
    /// evidence: accepted with its tier and fix, refuted, or deferred.
    Settle(settle::ReviewSettleArgs),

    /// Write the record edits open plan findings on a v2 flow carry, by tier,
    /// each compare-and-set against its `expect`.
    Apply(apply::ReviewApplyArgs),

    /// Yield, refute, audit and outcome rates per lens, probe or run. Writes nothing.
    Stats(stats::ReviewStatsArgs),

    /// Export open defect and cleanup findings into a v1 ledger.
    #[command(name = "export-v1")]
    ExportV1(export::ReviewExportArgs),

    /// Mint fix tasks in a v2 flow's task store from open defect and cleanup
    /// findings, and mark each finding `scheduled` with its task.
    Schedule(schedule::ReviewScheduleArgs),

    /// Move each finding scheduled into a flow by its task's state: resolved
    /// once the task is committed, open again when the task is gone or deferred.
    Reconcile(reconcile::ReviewReconcileArgs),

    /// Merge the lens catalogue's layers and print its index.
    Catalogue(catalogue_cmd::ReviewCatalogueArgs),

    /// Print a JSON Schema built from the field catalogue.
    Schema(schema_out::ReviewSchemaArgs),
}

/// A review verb's entry point, implemented by its args type in the verb's
/// module. A verb that names a store resolves it with [`store_target`].
trait Verb {
    fn run(self) -> Result<()>;
}

pub(crate) fn dispatch(op: ReviewOp) -> Result<()> {
    match op {
        ReviewOp::SyncV1(args) => args.run(),
        ReviewOp::Run { op } => op.run(),
        ReviewOp::Survey(args) => args.run(),
        ReviewOp::Plan(args) => args.run(),
        ReviewOp::Cover(args) => args.run(),
        ReviewOp::Add(args) => args.run(),
        ReviewOp::Verdict(args) => args.run(),
        ReviewOp::Resolve(args) => args.run(),
        ReviewOp::Reproduce(args) => args.run(),
        ReviewOp::ReproduceStatus(args) => args.run(),
        ReviewOp::List(args) => args.run(),
        ReviewOp::Show(args) => args.run(),
        ReviewOp::Anchor(args) => args.run(),
        ReviewOp::Digest(args) => args.run(),
        ReviewOp::RecheckTargets(args) => args.run(),
        ReviewOp::Report(args) => args.run(),
        ReviewOp::Triage(args) => args.run(),
        ReviewOp::Settle(args) => args.run(),
        ReviewOp::Apply(args) => args.run(),
        ReviewOp::Stats(args) => args.run(),
        ReviewOp::ExportV1(args) => args.run(),
        ReviewOp::Schedule(args) => args.run(),
        ReviewOp::Reconcile(args) => args.run(),
        ReviewOp::Catalogue(args) => args.run(),
        ReviewOp::Schema(args) => args.run(),
    }
}

/// The store `--slug` or `--scope` names.
fn store_target(store: &ReviewStore) -> Result<store::Target> {
    store::resolve(store.slug.as_deref(), store.scope.as_deref())
}

/// A `validation`-kind error, for input a review verb refuses.
pub(super) fn invalid(msg: impl Into<String>) -> anyhow::Error {
    crate::errors::tagged_err(crate::errors::ErrorKind::Validation, None, msg)
}
