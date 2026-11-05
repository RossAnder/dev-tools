//! `PlanOp` fan-out to the verb modules, one entry point per verb. Each verb
//! receives its parsed arguments and resolves the stores it needs from `--slug`:
//! a verb may touch `plan.toml`, `tasks.toml`, `brief.md` or `context.toml`.

use anyhow::Result;

use super::{approve, check, drift, milestone, open, probes, records, stage, view};
use crate::cli::{PlanHandoverOp, PlanMilestoneOp, PlanOp, PlanRunOp};

pub(crate) fn dispatch(op: PlanOp) -> Result<()> {
    match op {
        PlanOp::Open(args) => open::run(args),
        PlanOp::Decide(args) => records::decide(args),
        PlanOp::Contract(args) => records::contract(args),
        PlanOp::Handover { op } => match op {
            PlanHandoverOp::Add(args) => records::handover_add(args),
            PlanHandoverOp::Done(args) => records::handover_done(args),
        },
        PlanOp::Run { op } => match op {
            PlanRunOp::Start(args) => records::run_start(args),
            PlanRunOp::Finish(args) => records::run_finish(args),
            PlanRunOp::Skip(args) => records::run_skip(args),
        },
        PlanOp::Milestone { op } => match op {
            PlanMilestoneOp::Add(args) => milestone::add(args),
            PlanMilestoneOp::Update(args) => milestone::update(args),
        },
        PlanOp::Check(args) => check::run(args),
        PlanOp::View(args) => view::run(args),
        PlanOp::Probes(args) => probes::run(args),
        PlanOp::Stage(args) => stage::run(args),
        PlanOp::Gate(args) => approve::gate(args),
        PlanOp::Approve(args) => approve::approve(args),
        PlanOp::Drift(args) => drift::run(args),
    }
}
