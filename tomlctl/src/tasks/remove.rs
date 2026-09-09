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

use super::schema::{ImportOverride, Status, Store};
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
) -> Result<RemoveOutcome> {
    store::mutate(path, integrity, |store| retire(store, id, force))
}

fn retire(store: &mut Store, id: u32, force: bool) -> Result<RemoveOutcome> {
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
            task_list(&dependents)
        )));
    }

    let inherited = store.items[index].needs.clone();
    let mut rewired: Vec<u32> = Vec::new();
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
        }
    }

    let row = store.items.remove(index);
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

fn task_list(ids: &[u32]) -> String {
    let joined = ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<String>>()
        .join(", ");
    if ids.len() == 1 {
        format!("task {joined}")
    } else {
        format!("tasks {joined}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{DEFAULT_HEADING_DEPTH, Effort, TaskRow};

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
    fn an_absent_id_names_itself() {
        let mut store = store(vec![row(1, &[], &[])]);
        let message = retire(&mut store, 9, true)
            .expect_err("an absent id is refused")
            .to_string();
        assert!(message.contains("no task 9"), "{message}");
    }
}
