//! `ReviewOp` fan-out to the verb modules, one entry point per verb.
//!
//! The store target is resolved here for every verb that names one, so each
//! verb receives the resolved [`Target`] beside its arguments. `stats` takes
//! `--all` and resolves its own stores; `catalogue` and `schema` have none.

use anyhow::Result;

use super::store::{self, Target};
use super::{
    add, anchor, apply, catalogue_cmd, cells, cover, digest, export, query, recheck, reconcile,
    reproduce, resolve, run_show, runs, schedule, schema_out, settle, stats, survey, sync, triage,
    verdict,
};
use crate::cli::{ReviewOp, ReviewRunOp, ReviewStore};

pub(crate) fn dispatch(op: ReviewOp) -> Result<()> {
    match op {
        ReviewOp::SyncV1(args) => sync::run(&target(&args.store)?, args),
        ReviewOp::Run { op } => match op {
            ReviewRunOp::Start(args) => runs::start(&target(&args.store)?, args),
            ReviewRunOp::Update(args) => runs::update(&target(&args.store)?, args),
            ReviewRunOp::Finish(args) => runs::finish(&target(&args.store)?, args),
            ReviewRunOp::List(args) => runs::list(&target(&args.store)?, args),
            ReviewRunOp::Show(args) => run_show::show(&target(&args.store)?, args),
        },
        ReviewOp::Survey(args) => survey::run(&target(&args.store)?, args),
        ReviewOp::Plan(args) => cells::plan(&target(&args.store)?, args),
        ReviewOp::Cover(args) => cover::cover(&target(&args.store)?, args),
        ReviewOp::Add(args) => add::run(&target(&args.store)?, args),
        ReviewOp::Verdict(args) => verdict::run(&target(&args.store)?, args),
        ReviewOp::Resolve(args) => resolve::run(&target(&args.store)?, args),
        ReviewOp::Reproduce(args) => {
            reproduce::record(&target(&args.store)?, &args.ndjson, &args.integrity)
        }
        ReviewOp::ReproduceStatus(args) => reproduce::status(
            &target(&args.store)?,
            &args.ids,
            &args.expect,
            &args.integrity,
        ),
        ReviewOp::List(args) => query::list(&target(&args.store)?, args),
        ReviewOp::Show(args) => query::show(&target(&args.store)?, args),
        ReviewOp::Anchor(args) => {
            anchor::run(&target(&args.store)?, &args.id, args.gone, &args.integrity)
        }
        ReviewOp::Digest(args) => digest::digest(&target(&args.store)?, args),
        ReviewOp::RecheckTargets(args) => {
            recheck::run(&target(&args.store)?, &args.run, &args.integrity)
        }
        ReviewOp::Report(args) => digest::report(&target(&args.store)?, args),
        ReviewOp::Triage(args) => triage::run(&target(&args.store)?, args),
        ReviewOp::Settle(args) => settle::run(
            &target(&args.store)?,
            settle::Request {
                id: &args.id,
                accept: args.outcome.accept,
                refute: args.outcome.refute,
                defer: args.outcome.defer,
                tier: args.tier.as_deref(),
                fix_file: args.fix_file.as_deref(),
                citation: args.citation.as_deref(),
                proposal: args.proposal.as_deref(),
            },
            &args.integrity,
        ),
        ReviewOp::Apply(args) => apply::run(&target(&args.store)?, args),
        ReviewOp::Stats(args) => stats::run(args),
        ReviewOp::ExportV1(args) => export::run(&target(&args.store)?, args),
        ReviewOp::Schedule(args) => schedule::run(
            &target(&args.store)?,
            &args.store,
            schedule::Request {
                ids: &args.ids,
                all: args.all,
                flow: args.flow.as_deref().unwrap_or_default(),
                checkpoint: args.checkpoint.as_deref().unwrap_or_default(),
                settle_checkpoint: args.settle_checkpoint.as_deref(),
                dry_run: args.dry_run,
            },
            &args.integrity,
        ),
        ReviewOp::Reconcile(args) => {
            reconcile::run(&target(&args.store)?, &args.flow, &args.integrity)
        }
        ReviewOp::Catalogue(args) => catalogue_cmd::run(args),
        ReviewOp::Schema(args) => schema_out::run(args),
    }
}

fn target(store: &ReviewStore) -> Result<Target> {
    store::resolve(store.slug.as_deref(), store.scope.as_deref())
}
