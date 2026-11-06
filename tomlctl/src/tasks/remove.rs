//! The `remove` write verb — the only path that takes a row out of the store.
//!
//! `import-plan` keeps every row it no longer produces, so a task deleted from
//! the plan leaves a row nothing retires; without this verb the store is
//! hand-edited, and the next import re-appends from whatever the hand left.
//!
//! Two removals cost more than they look, and each is refused unless `--force`:
//! a row past `pending` is what the execution record's `task_ref` and the
//! commit train's SHA join on, and a row other rows depend on carries an
//! ordering constraint they still need. A forced removal splices the row's own
//! `needs` into every dependent rather than dropping the edge — a dependent
//! left with one fewer edge would dispatch ahead of work it still waits on,
//! and one left pointing at the removed id would make the store `dag/`
//! error-class.

use std::path::Path;

use anyhow::Result;
use serde_json::json;

use super::finding::{NO_TASK, task_list};
use super::schema::{Change, DEFAULT_ACTOR, ImportOverride, Status, Store};
use super::store;
use crate::cli::WriteIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct RemoveOutcome {
    pub(crate) id: u32,
    pub(crate) r#ref: String,
    /// Rows whose `needs`/`coupling` were re-pointed at the removed row's own
    /// dependencies, ascending.
    pub(crate) rewired: Vec<u32>,
    /// Fields the removed row's import-override stamp was holding against the
    /// plan, named as `tasks update --unlock-import-fields` takes them. Empty
    /// when the row carried no stamp.
    pub(crate) pruned_override_fields: Vec<&'static str>,
}

pub(crate) fn remove(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    id: u32,
    force: bool,
    reason: Option<&str>,
) -> Result<RemoveOutcome> {
    let v2_flow = store::is_v2_flow(path);
    store::mutate(path, integrity, |store| {
        retire(store, id, force, reason, v2_flow)
    })
}

/// An audited store records every removal as a tombstone, so it refuses one
/// with no `reason`; a forced removal also records each dependent it rewires.
///
/// A v2 store raises its id mark past the removed row. That is a store in a
/// flow with a `plan.toml` or one already holding a v2 key, whose premises can
/// name ids with no `plan.toml` present.
fn retire(
    store: &mut Store,
    id: u32,
    force: bool,
    reason: Option<&str>,
    v2_flow: bool,
) -> Result<RemoveOutcome> {
    let reason = reason.map(str::trim).unwrap_or_default();
    if store.audit && reason.is_empty() {
        return Err(refuse(
            "this store is audited: pass --reason <text> to remove a task".to_string(),
        ));
    }
    let index = store
        .items
        .iter()
        .position(|row| row.id == id)
        .ok_or_else(|| refuse(format!("no task {id} in the store")))?;

    let status = store.items[index].status;
    if status != Status::Pending && !force {
        return Err(refuse(format!(
            "task {id} is `{status}`, not pending: the execution record joins on its `ref` \
             `{}` and would name a row the store no longer holds — pass --force to remove it \
             anyway",
            store.items[index].r#ref
        )));
    }

    let dependents: Vec<u32> = store
        .items
        .iter()
        .filter(|row| row.id != id && (row.needs.contains(&id) || row.coupling.contains(&id)))
        .map(|row| row.id)
        .collect();
    if !dependents.is_empty() && !force {
        return Err(refuse(format!(
            "task {id} is a dependency of {}: re-point them first, or pass --force to splice \
             its own dependencies into theirs",
            task_list(&dependents, NO_TASK)
        )));
    }

    let inherited = store.items[index].needs.clone();
    let mut rewired: Vec<u32> = Vec::new();
    let mut needs_changes: Vec<Change> = Vec::new();
    let at = if store.audit {
        Some(crate::time::now_toml_datetime()?)
    } else {
        None
    };
    for row in &mut store.items {
        if row.id == id {
            continue;
        }
        let before = (row.needs.clone(), row.coupling.clone());
        row.coupling.retain(|target| *target != id);
        if row.needs.contains(&id) {
            row.needs.retain(|target| *target != id);
            for target in &inherited {
                if *target != row.id && !row.needs.contains(target) {
                    row.needs.push(*target);
                }
            }
        }
        if before != (row.needs.clone(), row.coupling.clone()) {
            rewired.push(row.id);
            if at.is_some() && before.0 != row.needs {
                needs_changes.push(Change {
                    at,
                    r#ref: row.r#ref.clone(),
                    field: "needs".to_string(),
                    old: json!(before.0).to_string(),
                    new: json!(row.needs).to_string(),
                    actor: DEFAULT_ACTOR.to_string(),
                    reason: reason.to_string(),
                });
            }
        }
    }

    let v2 = v2_flow || store.holds_v2_keys();
    if v2 {
        store.id_high_water = store.id_high_water.max(id);
    }
    let row = store.items.remove(index);
    // A v2 flow never imports, so nothing later would clear these. Elsewhere the
    // next import rebuilds the notes and links, and a store holding a spec is v2.
    if v2 {
        store.drop_ref_entries(&row.r#ref);
    }
    if at.is_some() {
        store.changes.push(Change {
            at,
            r#ref: row.r#ref.clone(),
            field: "*".to_string(),
            old: row.title.clone(),
            new: String::new(),
            actor: DEFAULT_ACTOR.to_string(),
            reason: reason.to_string(),
        });
        store.changes.extend(needs_changes);
    }
    // The next import would drop it anyway; leaving it until then holds an
    // entry keyed on a `ref` no row carries.
    let pruned = store
        .import_overrides
        .iter()
        .position(|entry| entry.r#ref == row.r#ref)
        .map(|position| store.import_overrides.remove(position));
    Ok(RemoveOutcome {
        id,
        r#ref: row.r#ref,
        rewired,
        pruned_override_fields: stamped_fields(pruned.as_ref()),
    })
}

/// A stamped field is one the row held against the plan, so naming it tells a
/// caller which value a re-imported row would take from the plan instead.
fn stamped_fields(entry: Option<&ImportOverride>) -> Vec<&'static str> {
    let Some(entry) = entry else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    if entry.files.is_some() {
        fields.push("files");
    }
    if entry.needs.is_some() {
        fields.push("needs");
    }
    fields
}

fn refuse(message: String) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{
        BacklogLink, DEFAULT_HEADING_DEPTH, Effort, FileNote, TaskRow, TaskSpec,
    };

    fn row(id: u32, needs: &[u32], coupling: &[u32]) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: String::new(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: DEFAULT_HEADING_DEPTH,
            files: Vec::new(),
            needs: needs.to_vec(),
            coupling: coupling.to_vec(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: String::new(),
            commit: String::new(),
        }
    }

    fn retire(store: &mut Store, id: u32, force: bool) -> Result<RemoveOutcome> {
        super::retire(store, id, force, None, false)
    }

    fn retire_with_reason(store: &mut Store, id: u32, force: bool, reason: &str) {
        super::retire(store, id, force, Some(reason), false).expect("the row retires");
    }

    fn spec(r#ref: &str) -> TaskSpec {
        TaskSpec {
            r#ref: r#ref.to_string(),
            outcome: String::new(),
            constraints: Vec::new(),
            accept: Vec::new(),
            premises: Vec::new(),
            means: Vec::new(),
            design: Vec::new(),
            precedent: Vec::new(),
            executor: String::new(),
        }
    }

    fn audited(items: Vec<TaskRow>) -> Store {
        Store {
            audit: true,
            ..store(items)
        }
    }

    fn store(items: Vec<TaskRow>) -> Store {
        Store {
            last_import_refs: items.iter().map(|row| row.r#ref.clone()).collect(),
            items,
            ..Store::default()
        }
    }

    #[test]
    fn an_unclaimed_pending_row_leaves_without_a_force_flag() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);
        let outcome = retire(&mut store, 2, false).expect("the row retires");

        assert_eq!(
            outcome,
            RemoveOutcome {
                id: 2,
                r#ref: "task-2".to_string(),
                rewired: Vec::new(),
                pruned_override_fields: Vec::new(),
            }
        );
        assert_eq!(
            store.items.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![1]
        );
    }

    #[test]
    fn a_settled_row_needs_the_force_flag() {
        let mut store = store(vec![row(1, &[], &[])]);
        store.items[0].status = Status::Done;

        let message = retire(&mut store, 1, false)
            .expect_err("a done row is refused")
            .to_string();
        assert!(message.contains("task-1"), "{message}");
        assert!(message.contains("--force"), "{message}");
        assert_eq!(store.items.len(), 1, "a refusal removes nothing");

        retire(&mut store, 1, true).expect("--force removes it");
        assert!(store.items.is_empty());
    }

    /// The ordering the dependent held through the removed row is what the
    /// splice preserves: dropping the edge alone would make task 3 dispatchable
    /// before task 1.
    #[test]
    fn a_forced_removal_splices_its_dependencies_into_every_dependent() {
        let mut store = store(vec![
            row(1, &[], &[]),
            row(2, &[1], &[]),
            row(3, &[2], &[2]),
            row(4, &[1], &[]),
        ]);

        let message = retire(&mut store, 2, false)
            .expect_err("a depended-on row is refused")
            .to_string();
        assert!(message.contains("task 3"), "{message}");

        let outcome = retire(&mut store, 2, true).expect("--force removes it");
        assert_eq!(outcome.rewired, vec![3]);
        assert_eq!(store.find(3).expect("task 3 survives").needs, vec![1]);
        assert!(store.find(3).expect("task 3 survives").coupling.is_empty());
        assert_eq!(store.find(4).expect("task 4 survives").needs, vec![1]);
        assert_eq!(store.find(2), None);
    }

    /// An entry keyed on a `ref` no row carries is an inconsistency the store
    /// otherwise never holds, so the removal takes the stamp with it and
    /// leaves every other row's alone. The pruned fields are reported, or the
    /// hand-patch the stamp was holding open goes silently.
    #[test]
    fn a_removal_prunes_the_rows_own_import_override_and_names_its_fields() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);
        store.import_overrides = vec![
            ImportOverride {
                r#ref: "task-1".to_string(),
                files: Some(vec!["src/a.rs".to_string()]),
                needs: None,
            },
            ImportOverride {
                r#ref: "task-2".to_string(),
                files: None,
                needs: Some(vec![1]),
            },
        ];

        let outcome = retire(&mut store, 2, false).expect("the row retires");
        assert_eq!(outcome.pruned_override_fields, vec!["needs"]);
        assert_eq!(
            store
                .import_overrides
                .iter()
                .map(|entry| entry.r#ref.as_str())
                .collect::<Vec<_>>(),
            vec!["task-1"]
        );
    }

    #[test]
    fn a_stamp_holding_both_fields_reports_both() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[1], &[])]);
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-2".to_string(),
            files: Some(vec!["src/b.rs".to_string()]),
            needs: Some(Vec::new()),
        }];

        let outcome = retire(&mut store, 2, false).expect("the row retires");
        assert_eq!(outcome.pruned_override_fields, vec!["files", "needs"]);
        assert_eq!(store.import_overrides, vec![]);
    }

    /// The empty report is the common case, and it must not pick up a sibling
    /// row's stamp: the key is `ref`, not position.
    #[test]
    fn a_removal_touching_no_stamp_reports_no_fields() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-1".to_string(),
            files: Some(vec!["src/a.rs".to_string()]),
            needs: None,
        }];

        let outcome = retire(&mut store, 2, false).expect("the row retires");
        assert!(
            outcome.pruned_override_fields.is_empty(),
            "{:?}",
            outcome.pruned_override_fields
        );
        assert_eq!(store.import_overrides.len(), 1);
    }

    #[test]
    fn a_refused_removal_leaves_the_stamp() {
        let mut store = store(vec![row(1, &[], &[])]);
        store.items[0].status = Status::Done;
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-1".to_string(),
            files: Some(Vec::new()),
            needs: None,
        }];

        retire(&mut store, 1, false).expect_err("a done row is refused");
        assert_eq!(store.import_overrides.len(), 1);
    }

    #[test]
    fn an_audited_removal_needs_a_reason() {
        let mut store = audited(vec![row(1, &[], &[])]);
        for reason in [None, Some("  ")] {
            let message = super::retire(&mut store, 1, false, reason, false)
                .expect_err("no reason is refused")
                .to_string();
            assert!(message.contains("--reason"), "{message}");
        }
        assert_eq!(store.items.len(), 1);
        assert!(store.changes.is_empty());
    }

    #[test]
    fn an_audited_removal_writes_a_tombstone_and_prunes_the_spec() {
        let mut store = audited(vec![row(1, &[], &[]), row(2, &[], &[])]);
        store.specs = vec![spec("task-1"), spec("task-2")];

        retire_with_reason(&mut store, 2, false, "dropped from scope");
        assert_eq!(store.changes.len(), 1);
        let change = &store.changes[0];
        assert_eq!(change.r#ref, "task-2");
        assert_eq!(change.field, "*");
        assert_eq!(change.old, "Task 2");
        assert_eq!(change.new, "");
        assert_eq!(change.reason, "dropped from scope");
        assert!(change.at.is_some());
        assert_eq!(
            store
                .specs
                .iter()
                .map(|s| s.r#ref.as_str())
                .collect::<Vec<_>>(),
            vec!["task-1"]
        );
    }

    #[test]
    fn an_audited_forced_removal_records_each_rewired_needs() {
        let mut store = audited(vec![row(1, &[], &[]), row(2, &[1], &[]), row(3, &[2], &[])]);
        retire_with_reason(&mut store, 2, true, "merged into 1");

        let needs: Vec<_> = store
            .changes
            .iter()
            .filter(|c| c.field == "needs")
            .collect();
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].r#ref, "task-3");
        assert_eq!(needs[0].old, "[2]");
        assert_eq!(needs[0].new, "[1]");
    }

    #[test]
    fn an_unaudited_removal_writes_no_change_but_still_prunes_the_spec() {
        let mut store = store(vec![row(1, &[], &[])]);
        store.specs = vec![spec("task-1")];
        retire(&mut store, 1, false).expect("retires");
        assert!(store.changes.is_empty());
        assert!(store.specs.is_empty());
    }

    #[test]
    fn a_v2_removal_retires_its_id_and_a_lower_one_keeps_the_mark() {
        let mut store = audited(vec![row(1, &[], &[]), row(2, &[], &[]), row(3, &[], &[])]);

        retire_with_reason(&mut store, 3, false, "dropped");
        assert_eq!(store.id_high_water, 3);
        assert_eq!(store.next_id(), 4);

        retire_with_reason(&mut store, 1, false, "dropped");
        assert_eq!(store.id_high_water, 3);
        assert_eq!(store.next_id(), 4);
    }

    /// A spec's premise can name an id with no `plan.toml` in the flow, and the
    /// row's own spec counts even though this removal drops it.
    #[test]
    fn a_spec_alone_makes_the_store_v2() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);
        store.specs = vec![spec("task-2")];

        retire(&mut store, 2, false).expect("the row retires");
        assert_eq!(store.next_id(), 3);
    }

    #[test]
    fn a_plan_toml_alone_makes_the_store_v2() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);

        super::retire(&mut store, 2, false, None, true).expect("the row retires");
        assert_eq!(store.id_high_water, 2);
        assert_eq!(store.next_id(), 3);
    }

    #[test]
    fn a_v1_removal_keeps_no_mark() {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);

        retire(&mut store, 2, false).expect("the row retires");
        assert_eq!(store.id_high_water, 0);
        assert_eq!(store.next_id(), 2);
    }

    /// Each of the two rows carries one note and one link.
    fn store_with_side_tables() -> Store {
        let mut store = store(vec![row(1, &[], &[]), row(2, &[], &[])]);
        for r#ref in ["task-1", "task-2"] {
            store.file_notes.push(FileNote {
                r#ref: r#ref.to_string(),
                file: "src/a.rs".to_string(),
                note: "(new)".to_string(),
            });
            store.backlog_links.push(BacklogLink {
                r#ref: r#ref.to_string(),
                closes: vec!["B1".to_string()],
                refs: Vec::new(),
            });
        }
        store
    }

    #[test]
    fn a_v2_removal_prunes_the_rows_own_file_notes_and_backlog_links() {
        let mut store = store_with_side_tables();

        super::retire(&mut store, 2, false, None, true).expect("the row retires");

        let note_refs: Vec<&str> = store.file_notes.iter().map(|n| n.r#ref.as_str()).collect();
        let link_refs: Vec<&str> = store
            .backlog_links
            .iter()
            .map(|l| l.r#ref.as_str())
            .collect();
        assert_eq!(note_refs, ["task-1"]);
        assert_eq!(link_refs, ["task-1"]);
    }

    #[test]
    fn a_v1_removal_leaves_file_notes_and_backlog_links_for_the_next_import() {
        let mut store = store_with_side_tables();

        retire(&mut store, 2, false).expect("the row retires");

        assert_eq!(store.file_notes.len(), 2);
        assert_eq!(store.backlog_links.len(), 2);
    }

    #[test]
    fn a_refused_removal_leaves_the_mark() {
        let mut store = audited(vec![row(1, &[], &[])]);
        store.items[0].status = Status::Done;

        super::retire(&mut store, 1, false, Some("dropped"), false).expect_err("needs --force");
        assert_eq!(store.id_high_water, 0);
    }

    #[test]
    fn an_absent_id_names_itself() {
        let mut store = store(vec![row(1, &[], &[])]);
        let message = retire(&mut store, 9, true)
            .expect_err("an absent id is refused")
            .to_string();
        assert!(message.contains("no task 9"), "{message}");
    }
}
