//! `ReviewOp` fan-out to the verb modules, one entry point per verb.
//!
//! The store target is resolved here for every verb that names one, so each
//! verb receives the resolved [`Target`] beside its arguments. `stats` takes
//! `--all` and resolves its own stores; `catalogue` and `schema` have none.

use anyhow::Result;

use super::store::{self, Target};
use super::{
    add, catalogue_cmd, cells, digest, export, query, resolve, runs, schema_out, stats, survey,
    sync, triage, verdict,
};
use crate::cli::{ReviewOp, ReviewRunOp, ReviewStore};

pub(crate) fn dispatch(op: ReviewOp) -> Result<()> {
    match op {
        ReviewOp::SyncV1(args) => sync::run(&target(&args.store)?, args),
        ReviewOp::Run { op } => match op {
            ReviewRunOp::Start(args) => runs::start(&target(&args.store)?, args),
            ReviewRunOp::Update(args) => runs::update(&target(&args.store)?, args),
            ReviewRunOp::Finish(args) => runs::finish(&target(&args.store)?, args),
        },
        ReviewOp::Survey(args) => survey::run(&target(&args.store)?, args),
        ReviewOp::Plan(args) => cells::plan(&target(&args.store)?, args),
        ReviewOp::Cover(args) => cells::cover(&target(&args.store)?, args),
        ReviewOp::Add(args) => add::run(&target(&args.store)?, args),
        ReviewOp::Verdict(args) => verdict::run(&target(&args.store)?, args),
        ReviewOp::Resolve(args) => resolve::run(&target(&args.store)?, args),
        ReviewOp::List(args) => query::list(&target(&args.store)?, args),
        ReviewOp::Show(args) => query::show(&target(&args.store)?, args),
        ReviewOp::Digest(args) => digest::digest(&target(&args.store)?, args),
        ReviewOp::Report(args) => digest::report(&target(&args.store)?, args),
        ReviewOp::Triage(args) => triage::run(&target(&args.store)?, args),
        ReviewOp::Stats(args) => stats::run(args),
        ReviewOp::ExportV1(args) => export::run(&target(&args.store)?, args),
        ReviewOp::Catalogue(args) => catalogue_cmd::run(args),
        ReviewOp::Schema(args) => schema_out::run(args),
    }
}

fn target(store: &ReviewStore) -> Result<Target> {
    store::resolve(store.slug.as_deref(), store.scope.as_deref())
}
