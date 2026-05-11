//! Central FlowOp dispatch. Each variant delegates to its leaf module's
//! `dispatch` fn so Phase A+B leaf tasks can fill in their own leaf file
//! without ever editing this file (parallel-safety guarantee per
//! `docs/plans/flow-tracking-overhaul.md` Phase A1/B1/B2 batches). Leaf
//! stubs return `bail!("unimplemented")` until their owning task lands.

use anyhow::Result;

use crate::cli::FlowOp;

pub(crate) fn dispatch(op: FlowOp) -> Result<()> {
    match op {
        FlowOp::Active { op } => crate::flow::active::dispatch(op),
        FlowOp::FindPlans { dirs, integrity } => {
            crate::flow::find_plans::dispatch(dirs, integrity)
        }
        FlowOp::Stale {
            slug,
            threshold,
            json,
            integrity,
        } => crate::flow::stale::dispatch(slug, threshold, json, integrity),
        FlowOp::Init {
            slug,
            plan,
            branch,
            worktree,
            scope,
            dry_run,
            integrity,
        } => crate::flow::init::dispatch(slug, plan, branch, worktree, scope, dry_run, integrity),
        FlowOp::EnsureArtifact {
            slug,
            kind,
            bootstrap,
            dry_run,
            integrity,
        } => crate::flow::ensure_artifact::dispatch(slug, kind, bootstrap, dry_run, integrity),
        FlowOp::Resolve {
            flow,
            path,
            branch,
            worktree,
            with_staleness,
            json,
            integrity,
        } => crate::flow::resolve::dispatch(
            flow,
            path,
            branch,
            worktree,
            with_staleness,
            json,
            integrity,
        ),
        FlowOp::Doctor {
            slug,
            fix,
            _json: _,
            dry_run,
            integrity,
        } => crate::flow::doctor::dispatch(slug, fix, dry_run, integrity),
        FlowOp::List {
            status,
            branch,
            active_only,
            integrity,
        } => crate::flow::list::dispatch(status, branch, active_only, integrity),
    }
}
