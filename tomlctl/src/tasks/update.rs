//! The `update` write verb.
//!
//! Every assignment is compared against what the row already holds, so the
//! returned field list is what actually moved rather than what was passed —
//! a re-issued `--status done` reports no change.
//!
//! `ref` is reachable only through `--ref`, never `--set ref=…`: it is the key
//! the execution record's `task_ref` and the store's `last_import_refs` both
//! join on, and a rename orphans a row in both.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

use super::schema::{Effort, Status, Store, TaskRow};
use super::store;
use crate::cli::WriteIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};

/// Row fields `--set` reaches, in the order the refusal message lists them.
const SETTABLE: [&str; 10] = [
    "acceptance",
    "action",
    "agent",
    "checkpoint",
    "commit",
    "deps_note",
    "detail",
    "effort",
    "status",
    "title",
];

/// Edge and file fields, which `tasks import-plan` owns.
const IMPORTED: [&str; 3] = ["coupling", "files", "needs"];

pub(crate) struct UpdateFields {
    pub(crate) status: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) checkpoint: Option<String>,
    pub(crate) task_ref: Option<String>,
    pub(crate) set: Vec<String>,
}

pub(crate) fn update(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    id: u32,
    fields: UpdateFields,
) -> Result<Vec<&'static str>> {
    store::mutate(path, integrity, |store| patch(store, id, &fields))
}

fn patch(store: &mut Store, id: u32, fields: &UpdateFields) -> Result<Vec<&'static str>> {
    let UpdateFields {
        status,
        agent,
        commit,
        checkpoint,
        task_ref,
        set,
    } = fields;

    let index = store
        .items
        .iter()
        .position(|row| row.id == id)
        .ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("no task {id} in the store"),
            )
        })?;

    let mut assignments: Vec<(&'static str, &str)> = Vec::new();
    let mut flagged: BTreeSet<&'static str> = BTreeSet::new();
    for (field, value) in [
        ("status", status),
        ("agent", agent),
        ("commit", commit),
        ("checkpoint", checkpoint),
    ] {
        if let Some(value) = value {
            flagged.insert(field);
            assignments.push((field, value.as_str()));
        }
    }

    for pair in set {
        let (key, value) = pair.split_once('=').ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("--set expects `KEY=VALUE`, got `{pair}`"),
            )
        })?;
        let field = settable(key)?;
        if flagged.contains(field) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("`{field}` was given both as --{field} and as --set {field}=…"),
            ));
        }
        assignments.push((field, value));
    }

    let mut changed: BTreeSet<&'static str> = BTreeSet::new();

    if let Some(new_ref) = task_ref {
        if new_ref.trim().is_empty() {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                "--ref needs a non-empty slug".to_string(),
            ));
        }
        if store
            .items
            .iter()
            .any(|row| row.id != id && row.r#ref == *new_ref)
        {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("ref `{new_ref}` already belongs to another task"),
            ));
        }
        if store.items[index].r#ref != *new_ref {
            store.items[index].r#ref.clone_from(new_ref);
            changed.insert("ref");
        }
    }

    let row = &mut store.items[index];
    for (field, value) in assignments {
        if assign(row, field, value)? {
            changed.insert(field);
        }
    }

    Ok(changed.into_iter().collect())
}

fn settable(key: &str) -> Result<&'static str> {
    if let Some(field) = SETTABLE.iter().copied().find(|field| *field == key) {
        return Ok(field);
    }
    let hint = match key {
        "ref" => {
            "`ref` is immutable here — pass `--ref <SLUG>` to rewrite it deliberately".to_string()
        }
        "id" => "`id` is minted by the store and cannot be reassigned".to_string(),
        key if IMPORTED.contains(&key) => {
            format!("`{key}` is owned by `tasks import-plan`, not `--set`")
        }
        key => format!(
            "unknown field `{key}` — settable fields are {}",
            SETTABLE.join(", ")
        ),
    };
    Err(tagged_err(ErrorKind::Validation, None, hint))
}

fn assign(row: &mut TaskRow, field: &'static str, value: &str) -> Result<bool> {
    let id = row.id;
    match field {
        "status" => {
            let parsed = Status::parse(value)
                .ok_or_else(|| vocabulary_err(id, "status", value, Status::VOCABULARY))?;
            Ok(replace(&mut row.status, parsed))
        }
        "effort" => {
            let parsed = Effort::parse(value)
                .ok_or_else(|| vocabulary_err(id, "effort", value, Effort::VOCABULARY))?;
            Ok(replace(&mut row.effort, parsed))
        }
        "title" => {
            if value.trim().is_empty() {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!("task {id}: `title` may not be blank"),
                ));
            }
            Ok(replace_str(&mut row.title, value))
        }
        "acceptance" => Ok(replace_str(&mut row.acceptance, value)),
        "action" => Ok(replace_str(&mut row.action, value)),
        "agent" => Ok(replace_str(&mut row.agent, value)),
        "checkpoint" => Ok(replace_str(&mut row.checkpoint, value)),
        "commit" => Ok(replace_str(&mut row.commit, value)),
        "deps_note" => Ok(replace_str(&mut row.deps_note, value)),
        "detail" => Ok(replace_str(&mut row.detail, value)),
        other => Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("field `{other}` has no assignment rule"),
        )),
    }
}

fn vocabulary_err(id: u32, field: &str, value: &str, vocabulary: &[&str]) -> anyhow::Error {
    tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "task {id}: unknown {field} `{value}` — expected one of {}",
            vocabulary.join(", ")
        ),
    )
}

fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        return false;
    }
    *slot = value;
    true
}

fn replace_str(slot: &mut String, value: &str) -> bool {
    if slot == value {
        return false;
    }
    value.clone_into(slot);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ReadIntegrityArgs;
    use crate::tasks::add::{self, NewTask};
    use crate::test_support::with_root;
    use std::path::PathBuf;

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn fields() -> UpdateFields {
        UpdateFields {
            status: None,
            agent: None,
            commit: None,
            checkpoint: None,
            task_ref: None,
            set: Vec::new(),
        }
    }

    fn seeded(root: &Path) -> PathBuf {
        let path = root
            .join(".claude")
            .join("flows")
            .join("whimsical-hugging-puppy")
            .join("tasks.toml");
        add::add(
            &path,
            &write_args(),
            NewTask {
                title: "Seed the store".to_string(),
                effort: "S".to_string(),
                files: Vec::new(),
                needs: Vec::new(),
                coupling: Vec::new(),
                deps_note: String::new(),
                checkpoint: String::new(),
                action: String::new(),
                detail: String::new(),
                acceptance: String::new(),
            },
        )
        .expect("the seed row lands");
        path
    }

    fn reload(path: &Path) -> Store {
        store::load(
            path,
            &ReadIntegrityArgs {
                verify_integrity: true,
                strict_read: false,
            },
        )
        .expect("the store loads")
    }

    #[test]
    fn a_status_change_reports_only_status() {
        with_root(|root| {
            let path = seeded(root);
            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    status: Some("done".to_string()),
                    ..fields()
                },
            )
            .expect("the row is patched");

            assert_eq!(changed, vec!["status"]);
            assert_eq!(reload(&path).items[0].status, Status::Done);
        });
    }

    #[test]
    fn re_issuing_the_same_value_reports_nothing() {
        with_root(|root| {
            let path = seeded(root);
            let args = || UpdateFields {
                status: Some("done".to_string()),
                ..fields()
            };
            update(&path, &write_args(), 1, args()).expect("first patch");
            let changed = update(&path, &write_args(), 1, args()).expect("second patch");
            assert!(changed.is_empty(), "{changed:?}");
        });
    }

    #[test]
    fn set_ref_is_refused_and_points_at_the_flag() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    set: vec!["ref=renamed".to_string()],
                    ..fields()
                },
            )
            .expect_err("`ref` is not settable")
            .to_string();
            assert!(message.contains("--ref"), "{message}");
            assert_eq!(
                reload(&path).items[0].r#ref,
                "seed-the-store",
                "a refused update must leave the ref alone"
            );
        });
    }

    #[test]
    fn the_ref_flag_rewrites_it_and_reports_ref() {
        with_root(|root| {
            let path = seeded(root);
            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    task_ref: Some("renamed".to_string()),
                    ..fields()
                },
            )
            .expect("--ref rewrites");
            assert_eq!(changed, vec!["ref"]);
            assert_eq!(reload(&path).items[0].r#ref, "renamed");
        });
    }

    #[test]
    fn a_flag_and_a_set_naming_one_field_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    agent: Some("implement-deep".to_string()),
                    set: vec!["agent=implement-lite".to_string()],
                    ..fields()
                },
            )
            .expect_err("the two spellings disagree")
            .to_string();
            assert!(message.contains("agent"), "{message}");
            assert_eq!(reload(&path).items[0].agent, "");
        });
    }

    #[test]
    fn set_covers_the_prose_fields_and_sorts_the_report() {
        with_root(|root| {
            let path = seeded(root);
            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    commit: Some("0d1bf49".to_string()),
                    set: vec![
                        "detail=Rewrite in place.".to_string(),
                        "effort=L".to_string(),
                    ],
                    ..fields()
                },
            )
            .expect("the row is patched");
            assert_eq!(changed, vec!["commit", "detail", "effort"]);

            let row = reload(&path).items.remove(0);
            assert_eq!(row.effort, Effort::L);
            assert_eq!(row.detail, "Rewrite in place.");
            assert_eq!(row.commit, "0d1bf49");
        });
    }

    #[test]
    fn an_out_of_vocabulary_status_names_the_row_and_the_vocabulary() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    status: Some("Done".to_string()),
                    ..fields()
                },
            )
            .expect_err("status is case-sensitive")
            .to_string();
            assert!(message.contains("task 1"), "{message}");
            assert!(message.contains("in-progress"), "{message}");
            assert_eq!(reload(&path).items[0].status, Status::Pending);
        });
    }

    #[test]
    fn an_absent_id_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                99,
                UpdateFields {
                    status: Some("done".to_string()),
                    ..fields()
                },
            )
            .expect_err("99 does not exist")
            .to_string();
            assert!(message.contains("99"), "{message}");
        });
    }
}
