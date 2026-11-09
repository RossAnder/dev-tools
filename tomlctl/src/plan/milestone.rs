//! `plan milestone add` and `plan milestone update`.
//!
//! A milestone is a `[[checkpoints]]` row (the id and its goal, as `rationale`)
//! plus a `[[milestones]]` row (gates and elaboration), written under one lock
//! so the id `tasks add --checkpoint` validates against always has both. A
//! review edit sets one field through the same write as `update`.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::records::{unwritten_outcome, written_only};
use super::store;
use crate::cli::{
    PlanMilestoneAddArgs, PlanMilestoneUpdateArgs, ReadIntegrityArgs, WriteIntegrityArgs,
};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json_compact;
use crate::tasks::schema::{
    Change, Checkpoint, DEFAULT_ACTOR, ELABORATION_DEFERRED, ELABORATION_ELABORATED,
    ELABORATION_UPFRONT, MILESTONE_REF_PREFIX, MilestoneSpec, Store,
};
use crate::tasks::{ReviewEditOutcome, load_store, mutate_store, resolve_store_path};

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

fn trimmed(raw: Option<&str>) -> &str {
    raw.map(str::trim).unwrap_or_default()
}

fn change(
    id: &str,
    field: &str,
    old: String,
    new: String,
    actor: &str,
    reason: &str,
) -> Result<Change> {
    Ok(Change {
        at: Some(crate::time::now_toml_datetime()?),
        r#ref: format!("{MILESTONE_REF_PREFIX}{id}"),
        field: field.to_string(),
        old,
        new,
        actor: actor.to_string(),
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
            store.changes.push(change(
                &id,
                "*",
                String::new(),
                args.goal.clone(),
                DEFAULT_ACTOR,
                &reason,
            )?);
        }
        Ok(())
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}

pub(super) fn update(args: PlanMilestoneUpdateArgs) -> Result<()> {
    let id = args.id.trim().to_string();
    if args.goal.is_none()
        && args.gate.is_empty()
        && !args.clear_gate
        && !args.defer
        && !args.elaborated
    {
        return Err(refuse(
            "nothing to update: pass `--goal`, `--gate`, `--clear-gate`, `--defer` or `--elaborated`"
                .to_string(),
        ));
    }
    if args.clear_gate && !args.gate.is_empty() {
        return Err(refuse(
            "`--clear-gate` and `--gate` set the gate list two different ways".to_string(),
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
    let patch = MilestonePatch {
        goal: args.goal.clone(),
        gates: (!args.gate.is_empty() || args.clear_gate).then(|| args.gate.clone()),
        elaboration: match (args.defer, args.elaborated) {
            (true, _) => Some(ELABORATION_DEFERRED.to_string()),
            (_, true) => Some(ELABORATION_ELABORATED.to_string()),
            _ => None,
        },
        defer_reason: args.defer.then(|| reason.clone()),
    };
    mutate_store(&path, &args.integrity, |store: &mut Store| {
        patch_milestone(store, &id, &patch, DEFAULT_ACTOR, &reason)
    })?;
    print_json_compact(&json!({ "ok": true, "id": id }))
}

/// The fields one milestone write sets; `None` leaves a field as it is.
#[derive(Default)]
struct MilestonePatch {
    goal: Option<String>,
    gates: Option<Vec<String>>,
    elaboration: Option<String>,
    defer_reason: Option<String>,
}

/// Writes `patch` to milestone `id`, adding its `[[milestones]]` row when only
/// the checkpoint exists, and audits each field that changed under `actor`.
/// Only a deferred milestone can become `elaborated`, and a milestone left
/// deferred by the patch needs a `defer_reason`.
fn patch_milestone(
    store: &mut Store,
    id: &str,
    patch: &MilestonePatch,
    actor: &str,
    reason: &str,
) -> Result<()> {
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
    if let Some(goal) = &patch.goal {
        let old = std::mem::replace(&mut checkpoint.rationale, goal.clone());
        changes.push(("goal", old, goal.clone()));
    }
    let index = match store.milestones.iter().position(|entry| entry.id == id) {
        Some(index) => index,
        None => {
            store.milestones.push(MilestoneSpec {
                id: id.to_string(),
                gates: Vec::new(),
                elaboration: ELABORATION_UPFRONT.to_string(),
                defer_reason: String::new(),
            });
            store.milestones.len() - 1
        }
    };
    let spec = &mut store.milestones[index];
    if let Some(gates) = &patch.gates {
        let old = std::mem::replace(&mut spec.gates, gates.clone());
        changes.push(("gates", json!(old).to_string(), json!(gates).to_string()));
    }
    if let Some(elaboration) = &patch.elaboration {
        if !ELABORATIONS.contains(&elaboration.as_str()) {
            return Err(refuse(format!(
                "unknown elaboration `{elaboration}` — expected one of {}",
                ELABORATIONS.join(", ")
            )));
        }
        if elaboration == ELABORATION_ELABORATED && !spec.is_deferred() {
            return Err(refuse(format!(
                "milestone `{id}` is `{}`, and only a deferred milestone can be elaborated",
                spec.elaboration
            )));
        }
        let old = std::mem::replace(&mut spec.elaboration, elaboration.clone());
        changes.push(("elaboration", old, elaboration.clone()));
    }
    if let Some(defer_reason) = &patch.defer_reason {
        let old = std::mem::replace(&mut spec.defer_reason, defer_reason.clone());
        changes.push(("defer_reason", old, defer_reason.clone()));
    }
    if (patch.elaboration.is_some() || patch.defer_reason.is_some())
        && spec.is_deferred()
        && spec.defer_reason.trim().is_empty()
    {
        return Err(refuse(format!(
            "milestone `{id}` is deferred and needs a `defer_reason`"
        )));
    }
    if store.audit {
        for (field, old, new) in changes.into_iter().filter(|(_, old, new)| old != new) {
            store
                .changes
                .push(change(id, field, old, new, actor, reason)?);
        }
    }
    Ok(())
}

const ELABORATIONS: [&str; 3] = [
    ELABORATION_UPFRONT,
    ELABORATION_DEFERRED,
    ELABORATION_ELABORATED,
];

const REVIEW_FIELDS: [&str; 4] = ["goal", "gates", "elaboration", "defer_reason"];

/// One compare-and-set edit to a milestone field. `expect` is required and
/// `new` is the field's whole value: a string, or an array of strings for
/// `gates`.
pub(crate) struct MilestoneEdit<'a> {
    pub(crate) id: &'a str,
    pub(crate) field: &'a str,
    pub(crate) expect: Option<&'a JsonValue>,
    pub(crate) new: &'a JsonValue,
    pub(crate) actor: &'a str,
    pub(crate) reason: &'a str,
}

/// Applies `edit` to the flow's task store, re-reading the field and checking
/// `expect` inside the store lock, through the write `plan milestone update`
/// makes. Only `Applied` writes; a refusal is an error and writes nothing.
pub(crate) fn apply_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &MilestoneEdit<'_>,
) -> Result<ReviewEditOutcome> {
    store::require_open(&store::resolve(slug)?)?;
    let path = resolve_store_path(Some(slug), None)?;
    let integrity = WriteIntegrityArgs {
        no_create: true,
        ..integrity.clone()
    };
    unwritten_outcome(mutate_store(&path, &integrity, |store: &mut Store| {
        written_only(milestone_edit(store, edit)?)
    }))
}

/// What `apply_review_edit` would return against the store as it stands,
/// writing nothing.
pub(crate) fn validate_review_edit(
    slug: &str,
    integrity: &WriteIntegrityArgs,
    edit: &MilestoneEdit<'_>,
) -> Result<ReviewEditOutcome> {
    store::require_open(&store::resolve(slug)?)?;
    let path = resolve_store_path(Some(slug), None)?;
    let mut store = load_store(
        &path,
        &ReadIntegrityArgs {
            verify_integrity: integrity.verify_integrity,
            strict_read: true,
        },
    )?;
    milestone_edit(&mut store, edit)
}

fn milestone_edit(store: &mut Store, edit: &MilestoneEdit<'_>) -> Result<ReviewEditOutcome> {
    let id = edit.id;
    let refuse_edit =
        |message: &str| refuse(format!("milestone `{id}` `{}`: {message}", edit.field));
    if !REVIEW_FIELDS.contains(&edit.field) {
        return Err(refuse_edit(&format!(
            "a review edit targets one of {}",
            REVIEW_FIELDS.join(", ")
        )));
    }
    let Some(expect) = edit.expect else {
        return Err(refuse_edit("the edit needs `expect`"));
    };
    let mut patch = MilestonePatch::default();
    match edit.field {
        "gates" => {
            let gates = edit
                .new
                .as_array()
                .and_then(|entries| {
                    entries
                        .iter()
                        .map(|entry| entry.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                })
                .ok_or_else(|| refuse_edit("`new` must be an array of strings"))?;
            patch.gates = Some(gates);
        }
        field => {
            let text = edit
                .new
                .as_str()
                .ok_or_else(|| refuse_edit("`new` must be a string"))?
                .to_string();
            match field {
                "goal" => patch.goal = Some(text),
                "elaboration" => patch.elaboration = Some(text),
                _ => patch.defer_reason = Some(text),
            }
        }
    }
    let current = milestone_value(store, id, edit.field).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no milestone `{id}` in the store"),
        )
    })?;
    if &current == edit.new {
        return Ok(ReviewEditOutcome::Already);
    }
    if expect != &current {
        return Ok(ReviewEditOutcome::Stale { found: current });
    }
    patch_milestone(store, id, &patch, edit.actor, edit.reason)?;
    Ok(ReviewEditOutcome::Applied)
}

/// The field as a review edit sees it; a milestone with no `[[milestones]]`
/// row reads as the row `patch_milestone` would add.
fn milestone_value(store: &Store, id: &str, field: &str) -> Option<JsonValue> {
    let checkpoint = store.checkpoints.iter().find(|entry| entry.id == id)?;
    let spec = store.milestones.iter().find(|entry| entry.id == id);
    Some(match field {
        "goal" => json!(checkpoint.rationale),
        "gates" => json!(spec.map(|s| s.gates.clone()).unwrap_or_default()),
        "elaboration" => json!(spec.map_or(ELABORATION_UPFRONT, |s| s.elaboration.as_str())),
        "defer_reason" => json!(spec.map(|s| s.defer_reason.as_str()).unwrap_or_default()),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Store {
        let mut store = Store {
            audit: true,
            ..Store::default()
        };
        store.checkpoints.push(Checkpoint {
            id: "A".to_string(),
            rationale: "old goal".to_string(),
        });
        store
    }

    fn edit<'a>(
        field: &'a str,
        expect: Option<&'a JsonValue>,
        new: &'a JsonValue,
    ) -> MilestoneEdit<'a> {
        MilestoneEdit {
            id: "A",
            field,
            expect,
            new,
            actor: "review",
            reason: "review r1 F2",
        }
    }

    #[test]
    fn a_milestone_edit_sets_audits_then_reports_already_and_stale() {
        let mut store = seeded();
        let (old, new) = (json!([]), json!(["cargo test"]));
        assert_eq!(
            milestone_edit(&mut store, &edit("gates", Some(&old), &new)).expect("applies"),
            ReviewEditOutcome::Applied
        );
        assert_eq!(store.milestones[0].gates, vec!["cargo test".to_string()]);
        let audit = store.changes.last().expect("an audit row");
        assert_eq!(
            (
                audit.r#ref.as_str(),
                audit.field.as_str(),
                audit.actor.as_str()
            ),
            ("milestone:A", "gates", "review")
        );
        assert_eq!(
            milestone_edit(&mut store, &edit("gates", Some(&old), &new)).expect("no error"),
            ReviewEditOutcome::Already
        );
        let (stale_expect, goal) = (json!("not the goal"), json!("new goal"));
        assert_eq!(
            milestone_edit(&mut store, &edit("goal", Some(&stale_expect), &goal))
                .expect("no error"),
            ReviewEditOutcome::Stale {
                found: json!("old goal")
            }
        );
        assert_eq!(store.changes.len(), 1);
    }

    #[test]
    fn a_milestone_edit_refuses_what_the_update_path_refuses() {
        let mut store = seeded();
        let upfront = json!("upfront");
        let values = [
            json!("elaborated"),
            json!("deferred"),
            json!("later"),
            json!("cargo test"),
            json!("new goal"),
            json!("B"),
        ];
        let cases = [
            edit("elaboration", Some(&upfront), &values[0]),
            edit("elaboration", Some(&upfront), &values[1]),
            edit("elaboration", Some(&upfront), &values[2]),
            edit("gates", Some(&upfront), &values[3]),
            edit("goal", None, &values[4]),
            edit("id", Some(&upfront), &values[5]),
        ];
        for case in &cases {
            assert!(
                milestone_edit(&mut store.clone(), case).is_err(),
                "{} {} should be refused",
                case.field,
                case.new
            );
        }
        store.checkpoints.clear();
        let (goal, new) = (json!("old goal"), json!("x"));
        assert!(milestone_edit(&mut store, &edit("goal", Some(&goal), &new)).is_err());
    }
}
