//! `plan emit schedule`: each milestone's state and the milestone to run next.
//!
//! The state rules live in `tasks::milestones`; this leaf gathers the task
//! store and the milestones an open `elaborate:<id>` run holds, and prints.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::json;

use super::store;
use crate::cli::{PlanEmitScheduleArgs, ReadIntegrityArgs};
use crate::output::print_json_line;
use crate::tasks::{load_store, resolve_store_path};

const ELABORATE_PREFIX: &str = "elaborate:";

pub(super) fn run(args: PlanEmitScheduleArgs) -> Result<()> {
    let PlanEmitScheduleArgs { target, integrity } = args;
    let slug = target.slug.as_str();
    let plan_path = store::resolve(slug)?;
    let plan = store::load(
        &plan_path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: false,
        },
    )?;
    let elaborating: BTreeSet<String> = store::open_runs(&plan)
        .into_iter()
        .filter_map(|run| run.step.strip_prefix(ELABORATE_PREFIX))
        .map(str::to_string)
        .collect();

    let path = resolve_store_path(Some(slug), None)?;
    let tasks = load_store(
        &path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: true,
        },
    )?;
    let schedule = crate::tasks::milestones::schedule(&tasks, &elaborating)?;

    let milestones: Vec<_> = schedule
        .milestones
        .iter()
        .map(|m| {
            json!({
                "id": m.id,
                "state": m.state,
                "on": m.on,
                "left": m.left,
            })
        })
        .collect();
    print_json_line(&json!({
        "pick": schedule.pick.unwrap_or_default(),
        "milestones": milestones,
    }))
}
