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
//!
//! `--unlock-import-fields` opens `files` and `needs` and records the value
//! the plan stated as the patch's base. The import keeps the hand-patched
//! value only while the plan still states that base.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;

use super::add::reject_undeclared_checkpoint;
use super::graph::{Graph, Node};
use super::schema::{Effort, ImportOverride, Status, Store, TaskRow};
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
/// set here does not survive the next import unless the row is stamped.
const IMPORTED: [&str; 2] = ["files", "needs"];

/// Fields taking a comma-separated list rather than a scalar. They leave the
/// row-local assignment path: two are edges the whole store validates, and
/// all three are compared against the row's value before anything moves.
const LISTS: [&str; 3] = ["coupling", "files", "needs"];

/// `changed[]` entry for a move of the row's import-override stamp. The
/// values it guards are reported under their own names.
const OVERRIDE: &str = "import_override";

pub(crate) struct UpdateFields {
    pub(crate) status: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) commit: Option<String>,
    pub(crate) checkpoint: Option<String>,
    pub(crate) task_ref: Option<String>,
    /// Permit `--set files=` / `--set needs=`, stamping the row with the plan
    /// values the patch replaces.
    pub(crate) unlock: bool,
    /// Drop the stamp, handing both fields back to the plan: the next import
    /// restores whatever the plan states.
    pub(crate) relock: bool,
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
        unlock,
        relock,
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
        let field = settable(key, *unlock)?;
        if flagged.contains(field) {
            return Err(tagged_err(
                ErrorKind::Validation,
                None,
                format!("`{field}` was given both as --{field} and as --set {field}=…"),
            ));
        }
        assignments.push((field, value));
    }

    // Last assignment wins, matching the scalar path, where a repeated
    // `--set` simply overwrites what the earlier one wrote.
    let mut lists: BTreeMap<&'static str, &str> = BTreeMap::new();
    for (field, value) in &assignments {
        if LISTS.contains(field) {
            lists.insert(field, value);
        }
    }
    assignments.retain(|(field, _)| !LISTS.contains(field));

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
            let previous = store.items[index].r#ref.clone();
            // The stamp is keyed on `ref` like `last_import_refs`, so it
            // follows the row rather than being stranded under the old key.
            if let Some(entry) = store
                .import_overrides
                .iter_mut()
                .find(|entry| entry.r#ref == previous)
            {
                entry.r#ref.clone_from(new_ref);
            }
            store.items[index].r#ref.clone_from(new_ref);
            changed.insert("ref");
        }
    }

    let coupling = lists
        .get("coupling")
        .map(|value| parse_ids(value))
        .transpose()?;
    let needs = lists
        .get("needs")
        .map(|value| parse_ids(value))
        .transpose()?;
    let files = lists.get("files").map(|value| parse_paths(value));

    let row = &store.items[index];
    let coupling = coupling.filter(|parsed| *parsed != row.coupling);
    let needs = needs.filter(|parsed| *parsed != row.needs);
    let files = files.filter(|parsed| *parsed != row.files);
    if coupling.is_some() || needs.is_some() {
        reject_broken_graph(store, index, coupling.as_deref(), needs.as_deref())?;
    }

    // The plan's own values, which is what the stamp records: the row still
    // holds them until the assignments below land.
    let base_files = store.items[index].files.clone();
    let base_needs = store.items[index].needs.clone();

    if let Some(parsed) = coupling {
        store.items[index].coupling = parsed;
        changed.insert("coupling");
    }
    if let Some(parsed) = needs {
        store.items[index].needs = parsed;
        changed.insert("needs");
    }
    if let Some(parsed) = files {
        store.items[index].files = parsed;
        changed.insert("files");
    }

    if *unlock && stamp(store, index, &changed, base_files, base_needs) {
        changed.insert(OVERRIDE);
    }
    if *relock && drop_stamp(store, index) {
        changed.insert(OVERRIDE);
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

fn settable(key: &str, unlock: bool) -> Result<&'static str> {
    let open = SETTABLE
        .iter()
        .chain(unlock.then_some(IMPORTED.iter()).into_iter().flatten());
    if let Some(field) = open.copied().find(|field| *field == key) {
        return Ok(field);
    }
    let hint = match key {
        "ref" => {
            "`ref` is immutable here — pass `--ref <SLUG>` to rewrite it deliberately".to_string()
        }
        "id" => "`id` is minted by the store and cannot be reassigned".to_string(),
        key if IMPORTED.contains(&key) => format!(
            "`{key}` is owned by `tasks import-plan` — pass `--unlock-import-fields` to patch \
             it by hand and stamp the row against the plan value it replaces"
        ),
        key => format!(
            "unknown field `{key}` — settable fields are {}",
            SETTABLE.join(", ")
        ),
    };
    Err(tagged_err(ErrorKind::Validation, None, hint))
}

/// Comma-separated repo-relative paths, matching `tasks add --files`. An
/// empty value clears the list.
fn parse_paths(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

/// Records the plan values the patch replaced. An existing base is kept —
/// overwriting it with a first patch's value would leave the import
/// comparing the plan against something it never stated — and a field
/// patched back to its base leaves the stamp.
fn stamp(
    store: &mut Store,
    index: usize,
    changed: &BTreeSet<&'static str>,
    base_files: Vec<String>,
    base_needs: Vec<u32>,
) -> bool {
    if !changed.contains("files") && !changed.contains("needs") {
        return false;
    }
    let row = store.items[index].clone();
    let position = store
        .import_overrides
        .iter()
        .position(|entry| entry.r#ref == row.r#ref);
    let mut entry = match position {
        Some(position) => store.import_overrides[position].clone(),
        None => ImportOverride::new(&row.r#ref),
    };
    let before = entry.clone();

    if changed.contains("files") && entry.files.is_none() {
        entry.files = Some(base_files);
    }
    if changed.contains("needs") && entry.needs.is_none() {
        entry.needs = Some(base_needs);
    }
    if entry.files.as_ref() == Some(&row.files) {
        entry.files = None;
    }
    if entry.needs.as_ref() == Some(&row.needs) {
        entry.needs = None;
    }

    if entry == before {
        return false;
    }
    match (position, entry.is_empty()) {
        (Some(position), true) => {
            store.import_overrides.remove(position);
        }
        (Some(position), false) => store.import_overrides[position] = entry,
        (None, true) => return false,
        (None, false) => store.import_overrides.push(entry),
    }
    true
}

/// Hands `files` and `needs` back to the plan. The values stay as they are
/// until the next import restores whatever the plan states.
fn drop_stamp(store: &mut Store, index: usize) -> bool {
    let r#ref = store.items[index].r#ref.clone();
    let before = store.import_overrides.len();
    store.import_overrides.retain(|entry| entry.r#ref != r#ref);
    store.import_overrides.len() != before
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
fn reject_broken_graph(
    store: &Store,
    index: usize,
    coupling: Option<&[u32]>,
    needs: Option<&[u32]>,
) -> Result<()> {
    let nodes: Vec<Node> = store
        .items
        .iter()
        .enumerate()
        .map(|(position, row)| {
            let mut node = Node::from(row);
            if position == index {
                if let Some(coupling) = coupling {
                    node.coupling = coupling.to_vec();
                }
                if let Some(needs) = needs {
                    node.needs = needs.to_vec();
                }
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
            unlock: false,
            relock: false,
            set: Vec::new(),
        }
    }

    fn unlocked(set: &[&str]) -> UpdateFields {
        UpdateFields {
            unlock: true,
            set: set.iter().map(|pair| (*pair).to_string()).collect(),
            ..fields()
        }
    }

    fn seeded(root: &Path) -> PathBuf {
        seeded_with(root, &[])
    }

    fn seeded_with(root: &Path, files: &[&str]) -> PathBuf {
        let path = root
            .join(".claude")
            .join("flows")
            .join("whimsical-hugging-puppy")
            .join("tasks.toml");
        add_row_with(&path, "Seed the store", files, &[]);
        path
    }

    fn add_row(path: &Path, title: &str) -> u32 {
        add_row_with(path, title, &[], &[])
    }

    /// A row carrying the values an import would have written, which is what
    /// the unlock stamp records as its base.
    fn add_row_with(path: &Path, title: &str, files: &[&str], needs: &[u32]) -> u32 {
        add::add(
            path,
            &write_args(),
            NewTask {
                title: title.to_string(),
                effort: "S".to_string(),
                files: files.iter().map(|file| (*file).to_string()).collect(),
                needs: needs.to_vec(),
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

    /// The refusal is the default, and it has to name the way out: a
    /// plan-review merge that cannot find one goes back to editing the
    /// markdown by hand.
    #[test]
    fn an_import_owned_field_is_refused_and_names_the_unlock() {
        with_root(|root| {
            let path = seeded(root);
            for pair in ["files=src/a.rs", "needs=1"] {
                let message = update(
                    &path,
                    &write_args(),
                    1,
                    UpdateFields {
                        set: vec![pair.to_string()],
                        ..fields()
                    },
                )
                .expect_err("import-owned without the flag")
                .to_string();
                assert!(message.contains("--unlock-import-fields"), "{message}");
            }
            assert!(reload(&path).items[0].files.is_empty());
        });
    }

    #[test]
    fn an_unlocked_patch_stamps_the_row_with_the_value_it_replaced() {
        with_root(|root| {
            let path = seeded(root);
            add_row_with(&path, "Wire the renderer", &["src/b.rs"], &[1]);

            let changed = update(
                &path,
                &write_args(),
                2,
                unlocked(&["files=src/b.rs,src/c.rs"]),
            )
            .expect("the patch lands");
            assert_eq!(changed, vec!["files", "import_override"]);

            let store = reload(&path);
            assert_eq!(store.items[1].files, vec!["src/b.rs", "src/c.rs"]);
            let stamp = store
                .find_override("wire-the-renderer")
                .expect("the row is stamped");
            assert_eq!(stamp.files.as_deref(), Some(&["src/b.rs".to_string()][..]));
            assert_eq!(stamp.needs, None, "only the patched field is stamped");
        });
    }

    /// The base is the plan's value, so a second hand patch must not move it
    /// to the first patch's — the import would then compare the plan against
    /// a value it never held and hold the override open for good.
    #[test]
    fn a_second_patch_keeps_the_first_base_and_a_patch_back_to_it_clears_the_stamp() {
        with_root(|root| {
            let path = seeded_with(root, &["src/a.rs"]);
            update(
                &path,
                &write_args(),
                1,
                unlocked(&["files=src/a.rs,src/b.rs"]),
            )
            .expect("first patch");
            update(
                &path,
                &write_args(),
                1,
                unlocked(&["files=src/a.rs,src/b.rs,src/c.rs"]),
            )
            .expect("second patch");
            assert_eq!(
                reload(&path)
                    .find_override("seed-the-store")
                    .and_then(|stamp| stamp.files.clone()),
                Some(vec!["src/a.rs".to_string()])
            );

            let changed =
                update(&path, &write_args(), 1, unlocked(&["files=src/a.rs"])).expect("reverted");
            assert!(changed.contains(&"import_override"), "{changed:?}");
            assert_eq!(reload(&path).import_overrides, vec![]);
        });
    }

    #[test]
    fn relocking_drops_the_stamp_and_leaves_the_values_alone() {
        with_root(|root| {
            let path = seeded_with(root, &["src/a.rs"]);
            update(&path, &write_args(), 1, unlocked(&["files=src/z.rs"])).expect("the patch");

            let changed = update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    relock: true,
                    ..fields()
                },
            )
            .expect("the stamp drops");
            assert_eq!(changed, vec!["import_override"]);

            let store = reload(&path);
            assert_eq!(store.import_overrides, vec![]);
            assert_eq!(
                store.items[0].files,
                vec!["src/z.rs"],
                "relocking hands the field back to the plan, it does not restore a value"
            );
        });
    }

    /// `needs` is an edge like `coupling`, so an unlocked patch is held to the
    /// same whole-store validation rather than the row alone.
    #[test]
    fn an_unlocked_needs_patch_is_validated_against_the_graph() {
        with_root(|root| {
            let path = seeded(root);
            add_row(&path, "Wire the renderer");
            let message = update(&path, &write_args(), 2, unlocked(&["needs=99"]))
                .expect_err("99 is not a task")
                .to_string();
            assert!(message.contains("99"), "{message}");

            update(&path, &write_args(), 2, unlocked(&["needs=1"])).expect("1 exists");
            let message = update(&path, &write_args(), 1, unlocked(&["needs=2"]))
                .expect_err("1 → 2 → 1 is a cycle")
                .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(reload(&path).items[0].needs.is_empty());
        });
    }

    /// The stamp is keyed on `ref`, so a rename has to carry it: an entry
    /// left under the old key pins nothing and the row loses its patch at the
    /// next import.
    #[test]
    fn renaming_the_ref_carries_the_stamp_with_the_row() {
        with_root(|root| {
            let path = seeded_with(root, &["src/a.rs"]);
            update(&path, &write_args(), 1, unlocked(&["files=src/z.rs"])).expect("the patch");
            update(
                &path,
                &write_args(),
                1,
                UpdateFields {
                    task_ref: Some("reseed-the-store".to_string()),
                    ..fields()
                },
            )
            .expect("the rename lands");

            let store = reload(&path);
            assert!(store.find_override("seed-the-store").is_none());
            assert_eq!(
                store
                    .find_override("reseed-the-store")
                    .and_then(|stamp| stamp.files.clone()),
                Some(vec!["src/a.rs".to_string()])
            );
        });
    }
}
