//! The `update` write verb.
//!
//! Every assignment is compared against what the row already holds, so the
//! returned field list is what actually moved rather than what was passed —
//! a re-issued `--status done` reports no change.
//!
//! `ref` is reachable only through `--ref`, never `--set ref=…`: it is the key
//! the execution record's `task_ref` and the store's `last_import_refs` both
//! join on, and a rename orphans a row in both. `import-plan` derives that key
//! back off the title, so a `--set title=` deriving a different ref would make
//! the next import mint a second row for the same task: it is refused unless
//! `--ref` moves the key in the same command.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

use super::add::reject_undeclared_checkpoint;
use super::graph::{Graph, Node};
use super::schema::{Effort, Status, Store, TaskRow};
use super::{slug, store};
use crate::cli::WriteIntegrityArgs;
use crate::errors::{ErrorKind, tagged_err};

/// Row fields `--set` reaches, in the order the refusal message lists them.
const SETTABLE: [&str; 11] = [
    "acceptance",
    "action",
    "agent",
    "checkpoint",
    "commit",
    "coupling",
    "deps_note",
    "detail",
    "effort",
    "status",
    "title",
];

/// Fields `tasks import-plan` rewrites from the plan on every run, so a value
/// set here would not survive the next import.
const IMPORTED: [&str; 2] = ["files", "needs"];

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

    // `coupling` is the one settable field validated against the whole store
    // rather than the row alone, so it leaves the row-local assignment list.
    let coupling = assignments
        .iter()
        .rev()
        .find(|(field, _)| *field == "coupling")
        .map(|(_, value)| *value);
    assignments.retain(|(field, _)| *field != "coupling");

    for (field, value) in &assignments {
        if *field == "checkpoint" {
            reject_undeclared_checkpoint(store, value, &format!("task {id}"))?;
        }
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

    if let Some(value) = coupling {
        let parsed = parse_ids(value)?;
        if store.items[index].coupling != parsed {
            reject_broken_graph(store, index, &parsed)?;
            store.items[index].coupling = parsed;
            changed.insert("coupling");
        }
    }

    let previous_derived = assignments
        .iter()
        .any(|(field, _)| *field == "title")
        .then(|| slug::derive_ref(&store.items[index].title));

    let row = &mut store.items[index];
    for (field, value) in assignments {
        if assign(row, field, value)? {
            changed.insert(field);
        }
    }

    if let (Some(previous), None) = (previous_derived, task_ref) {
        let derived = slug::derive_ref(&store.items[index].title);
        if derived != previous {
            return Err(retitle_err(id, &derived));
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

/// An empty value clears the edge set; anything else is the comma-separated id
/// list `tasks add --coupling` takes.
fn parse_ids(value: &str) -> Result<Vec<u32>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|part| {
            part.trim().parse::<u32>().map_err(|_| {
                tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!("`coupling` takes comma-separated task ids, got `{part}`"),
                )
            })
        })
        .collect()
}

/// Validates the store as it would be, so an edge naming an absent task or
/// closing a cycle is refused before the row moves — the guard `tasks add`
/// runs over its own edges.
fn reject_broken_graph(store: &Store, index: usize, coupling: &[u32]) -> Result<()> {
    let nodes: Vec<Node> = store
        .items
        .iter()
        .enumerate()
        .map(|(position, row)| {
            let mut node = Node::from(row);
            if position == index {
                node.coupling = coupling.to_vec();
            }
            node
        })
        .collect();
    let graph = Graph::build(&nodes)
        .map_err(|err| tagged_err(ErrorKind::Validation, None, err.to_string()))?;

    let cycle = graph.cycle_members();
    if cycle.is_empty() {
        return Ok(());
    }
    let members: Vec<String> = cycle.iter().map(u32::to_string).collect();
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "refusing to update: the dependency graph would contain a cycle through tasks {}",
            members.join(", ")
        ),
    ))
}

fn retitle_err(id: u32, derived: &str) -> anyhow::Error {
    let hint = if derived.is_empty() {
        format!(
            "task {id}: this title carries no slug characters, so it derives no ref — pass `--ref <SLUG>` alongside it"
        )
    } else {
        format!(
            "task {id}: this title derives ref `{derived}` — pass `--ref {derived}` alongside it to move the row's key deliberately"
        )
    };
    tagged_err(ErrorKind::Validation, None, hint)
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
        add_row(&path, "Seed the store");
        path
    }

    fn add_row(path: &Path, title: &str) -> u32 {
        add::add(
            path,
            &write_args(),
            NewTask {
                title: title.to_string(),
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
        .expect("the row lands")
        .id
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
    fn set_coupling_rewrites_the_edge_set_and_clears_it() {
        with_root(|root| {
            let path = seeded(root);
            let second = add_row(&path, "Arm the acceptance");

            let changed = update(
                &path,
                &write_args(),
                second,
                UpdateFields {
                    set: vec!["coupling=1".to_string()],
                    ..fields()
                },
            )
            .expect("the edge lands");
            assert_eq!(changed, vec!["coupling"]);
            assert_eq!(reload(&path).items[1].coupling, vec![1]);

            let changed = update(
                &path,
                &write_args(),
                second,
                UpdateFields {
                    set: vec!["coupling=".to_string()],
                    ..fields()
                },
            )
            .expect("the edge clears");
            assert_eq!(changed, vec!["coupling"]);
            assert!(reload(&path).items[1].coupling.is_empty());
        });
    }

    #[test]
    fn a_coupling_edge_naming_an_absent_task_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    set: vec!["coupling=99".to_string()],
                    ..fields()
                },
            )
            .expect_err("99 does not exist")
            .to_string();
            assert!(message.contains("99"), "{message}");
            assert!(reload(&path).items[0].coupling.is_empty());
        });
    }

    #[test]
    fn a_coupling_edge_that_would_close_a_cycle_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let second = add_row(&path, "Arm the acceptance");
            update(
                &path,
                &write_args(),
                second,
                UpdateFields {
                    set: vec!["coupling=1".to_string()],
                    ..fields()
                },
            )
            .expect("the first edge lands");

            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    set: vec![format!("coupling={second}")],
                    ..fields()
                },
            )
            .expect_err("the pair would cycle")
            .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(reload(&path).items[0].coupling.is_empty());
        });
    }

    #[test]
    fn a_retitle_that_moves_the_derived_ref_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    set: vec!["title=Reseed the store".to_string()],
                    ..fields()
                },
            )
            .expect_err("the derived ref would move")
            .to_string();
            assert!(message.contains("reseed-the-store"), "{message}");
            assert!(message.contains("--ref"), "{message}");

            let row = reload(&path).items.remove(0);
            assert_eq!(row.title, "Seed the store");
            assert_eq!(row.r#ref, "seed-the-store");
        });
    }

    #[test]
    fn a_retitle_holding_the_derived_ref_still_lands() {
        with_root(|root| {
            let path = seeded(root);
            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    set: vec!["title=Seed the (store)".to_string()],
                    ..fields()
                },
            )
            .expect("the derivation is unchanged");
            assert_eq!(changed, vec!["title"]);
            assert_eq!(reload(&path).items[0].title, "Seed the (store)");
        });
    }

    #[test]
    fn a_retitle_carrying_the_ref_flag_rewrites_both() {
        with_root(|root| {
            let path = seeded(root);
            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    task_ref: Some("reseed-the-store".to_string()),
                    set: vec!["title=Reseed the store".to_string()],
                    ..fields()
                },
            )
            .expect("the explicit ref wins");
            assert_eq!(changed, vec!["ref", "title"]);

            let row = reload(&path).items.remove(0);
            assert_eq!(row.title, "Reseed the store");
            assert_eq!(row.r#ref, "reseed-the-store");
        });
    }

    #[test]
    fn a_checkpoint_group_the_store_does_not_declare_is_refused() {
        with_root(|root| {
            let path = seeded(root);
            let message = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    checkpoint: Some("Z".to_string()),
                    ..fields()
                },
            )
            .expect_err("`Z` is declared nowhere")
            .to_string();
            assert!(message.contains('Z'), "{message}");
            assert_eq!(reload(&path).items[0].checkpoint, "");
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
