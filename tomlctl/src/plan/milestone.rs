//! `plan milestone add` and `plan milestone update`.
//!
//! A milestone is a `[[checkpoints]]` row (the id and its goal, as `rationale`)
//! plus a `[[milestones]]` row (gates and elaboration), written under one lock
//! so the id `tasks add --checkpoint` validates against always has both.

use anyhow::Result;
use serde_json::json;

use super::store;
use crate::cli::{PlanMilestoneAddArgs, PlanMilestoneUpdateArgs};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_compact;
use crate::tasks::schema::{Change, Checkpoint, ELABORATION_UPFRONT, MilestoneSpec, Store};
use crate::tasks::{mutate_store, resolve_store_path};

const ELABORATION_DEFERRED: &str = "deferred";
const ELABORATION_ELABORATED: &str = "elaborated";
const AUDIT_ACTOR: &str = "orchestrator";

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn trimmed(raw: Option<&str>) -> &str {
    raw.map(str::trim).unwrap_or_default()
}

fn change(id: &str, field: &str, old: String, new: String, reason: &str) -> Result<Change> {
    Ok(Change {
        at: Some(crate::time::now_toml_datetime()?),
        r#ref: format!("milestone:{id}"),
        field: field.to_string(),
        old,
        new,
        actor: AUDIT_ACTOR.to_string(),
        reason: reason.to_string(),
    })
}

pub(super) fn add(args: PlanMilestoneAddArgs) -> Result<()> {
    let id = args.id.trim().to_string();
    if id.is_empty() {
        return Err(refuse("`--id` must not be empty".to_string()));
    }
    let reason = trimmed(args.reason.as_deref()).to_string();
    if args.defer && reason.is_empty() {
        return Err(refuse("`--defer` needs `--reason <text>`".to_string()));
    }
    store::require_open(&store::resolve(&args.target.slug)?)?;
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    mutate_store(&path, &args.integrity, |store: &mut Store| {
        if store.audit && reason.is_empty() {
            return Err(refuse(
                "this store is audited: pass --reason <text> to add a milestone".to_string(),
            ));
        }
        if store.checkpoints.iter().any(|entry| entry.id == id)
            || store.milestones.iter().any(|entry| entry.id == id)
        {
            return Err(refuse(format!("milestone `{id}` already exists")));
        }
        store.checkpoints.push(Checkpoint {
            id: id.clone(),
            rationale: args.goal.clone(),
        });
        store.milestones.push(MilestoneSpec {
            id: id.clone(),
            gates: args.gate.clone(),
            elaboration: match args.defer {
                true => ELABORATION_DEFERRED.to_string(),
                false => ELABORATION_UPFRONT.to_string(),
            },
            defer_reason: match args.defer {
                true => reason.clone(),
                false => String::new(),
            },
        });
        if store.audit {
            store
                .changes
                .push(change(&id, "*", String::new(), args.goal.clone(), &reason)?);
        }
        Ok(())
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}

pub(super) fn update(args: PlanMilestoneUpdateArgs) -> Result<()> {
    let id = args.id.trim().to_string();
    if args.goal.is_none() && args.gate.is_empty() && !args.defer && !args.elaborated {
        return Err(refuse(
            "nothing to update: pass `--goal`, `--gate`, `--defer` or `--elaborated`".to_string(),
        ));
    }
    if args.defer && args.elaborated {
        return Err(refuse(
            "`--defer` and `--elaborated` move the milestone in opposite directions".to_string(),
        ));
    }
    let reason = trimmed(args.reason.as_deref()).to_string();
    if args.defer && reason.is_empty() {
        return Err(refuse("`--defer` needs `--reason <text>`".to_string()));
    }
    if args.elaborated && reason.is_empty() {
        return Err(refuse("`--elaborated` needs `--reason <text>`".to_string()));
    }
    store::require_open(&store::resolve(&args.target.slug)?)?;
    let path = resolve_store_path(Some(&args.target.slug), None)?;
    mutate_store(&path, &args.integrity, |store: &mut Store| {
        if store.audit && reason.is_empty() {
            return Err(refuse(
                "this store is audited: pass --reason <text> to update a milestone".to_string(),
            ));
        }
        let Some(checkpoint) = store.checkpoints.iter_mut().find(|entry| entry.id == id) else {
            return Err(tagged_err(
                ErrorKind::NotFound,
                None,
                format!("no milestone `{id}` in the store"),
            ));
        };
        let mut changes: Vec<(&str, String, String)> = Vec::new();
        if let Some(goal) = &args.goal {
            let old = std::mem::replace(&mut checkpoint.rationale, goal.clone());
            changes.push(("goal", old, goal.clone()));
        }
        let index = match store.milestones.iter().position(|entry| entry.id == id) {
            Some(index) => index,
            None => {
                store.milestones.push(MilestoneSpec {
                    id: id.clone(),
                    gates: Vec::new(),
                    elaboration: ELABORATION_UPFRONT.to_string(),
                    defer_reason: String::new(),
                });
                store.milestones.len() - 1
            }
        };
        let spec = &mut store.milestones[index];
        if !args.gate.is_empty() {
            let old = std::mem::replace(&mut spec.gates, args.gate.clone());
            changes.push((
                "gates",
                json!(old).to_string(),
                json!(args.gate).to_string(),
            ));
        }
        if args.defer {
            let old = std::mem::replace(&mut spec.elaboration, ELABORATION_DEFERRED.to_string());
            changes.push(("elaboration", old, ELABORATION_DEFERRED.to_string()));
            let old = std::mem::replace(&mut spec.defer_reason, reason.clone());
            changes.push(("defer_reason", old, reason.clone()));
        }
        if args.elaborated {
            if spec.elaboration != ELABORATION_DEFERRED {
                return Err(refuse(format!(
                    "milestone `{id}` is `{}`, and only a deferred milestone can be elaborated",
                    spec.elaboration
                )));
            }
            let old = std::mem::replace(&mut spec.elaboration, ELABORATION_ELABORATED.to_string());
            changes.push(("elaboration", old, ELABORATION_ELABORATED.to_string()));
        }
        if store.audit {
            for (field, old, new) in changes {
                store.changes.push(change(&id, field, old, new, &reason)?);
            }
        }
        Ok(())
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}
