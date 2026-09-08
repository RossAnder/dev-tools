//! The `add` and `add-many` write verbs.
//!
//! Both funnel through `append`, which mints every row first, validates the
//! whole set against a `Graph` over the store *as it would be*, and only then
//! extends `Store::items`. A dangling target or a cycle therefore leaves
//! `store::mutate`'s closure in the `Err` arm, and nothing — file or sidecar —
//! is written.
//!
//! A batch is one candidate graph, so two rows in the same `add-many` may
//! reference each other; a cycle between them is still refused.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value as JsonValue;

use super::graph::{Graph, Node};
use super::schema::{DEFAULT_HEADING_DEPTH, Effort, Status, Store, TaskRow};
use super::{slug, store};
use crate::cli::WriteIntegrityArgs;
use crate::convert::json_type_name;
use crate::errors::{ErrorKind, tagged_err};

/// One row as the caller supplies it. `effort` is still the raw flag string:
/// the schema's vocabulary refusal is what the user should see, not clap's
/// usage prose.
pub(crate) struct NewTask {
    pub(crate) title: String,
    pub(crate) effort: String,
    pub(crate) files: Vec<String>,
    pub(crate) needs: Vec<u32>,
    pub(crate) coupling: Vec<u32>,
    pub(crate) deps_note: String,
    pub(crate) checkpoint: String,
    pub(crate) action: String,
    pub(crate) detail: String,
    pub(crate) acceptance: String,
}

#[derive(Debug)]
pub(crate) struct AddOutcome {
    pub(crate) id: u32,
    pub(crate) r#ref: String,
    /// Zero-based index into `Graph::kahn_rounds` — the round the row lands in
    /// once it is stored.
    pub(crate) batch: usize,
}

/// Row fields an NDJSON line may carry. Anything else is refused rather than
/// dropped: a mistyped `neds` would otherwise silently discard an edge.
const ROW_FIELDS: [&str; 10] = [
    "title",
    "effort",
    "files",
    "needs",
    "coupling",
    "deps_note",
    "checkpoint",
    "action",
    "detail",
    "acceptance",
];

pub(crate) fn add(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    task: NewTask,
) -> Result<AddOutcome> {
    let mut outcomes = store::mutate(path, integrity, |store| append(store, vec![task]))?;
    Ok(outcomes.remove(0))
}

pub(crate) fn add_many(
    path: &Path,
    integrity: &WriteIntegrityArgs,
    ndjson: &str,
) -> Result<Vec<AddOutcome>> {
    let rows = crate::items::parse_ndjson(ndjson)?;
    let tasks = rows
        .iter()
        .enumerate()
        .map(|(index, row)| task_from_json(row, index + 1))
        .collect::<Result<Vec<_>>>()?;
    store::mutate(path, integrity, |store| append(store, tasks))
}

/// Resolve a `--<body>` / `--<body>-file` pair. Both absent is a blank body,
/// which is what an incrementally-filled row starts as.
pub(crate) fn body(literal: Option<String>, file: Option<PathBuf>) -> Result<String> {
    match (literal, file) {
        (Some(text), _) => Ok(text),
        (None, Some(path)) => fs::read_to_string(&path)
            .with_context(|| format!("reading body file `{}`", path.display())),
        (None, None) => Ok(String::new()),
    }
}

fn append(store: &mut Store, tasks: Vec<NewTask>) -> Result<Vec<AddOutcome>> {
    let mut taken: BTreeSet<String> = store
        .items
        .iter()
        .map(|row| row.r#ref.clone())
        .collect::<BTreeSet<_>>();

    let mut next = store.next_id();
    let mut minted = Vec::with_capacity(tasks.len());
    for task in tasks {
        reject_undeclared_checkpoint(store, &task.checkpoint, &format!("task `{}`", task.title))?;
        let row = build_row(task, next, &mut taken)?;
        next = next.checked_add(1).ok_or_else(|| {
            tagged_err(
                ErrorKind::Validation,
                None,
                "task ids are exhausted".to_string(),
            )
        })?;
        minted.push(row);
    }

    let nodes: Vec<Node> = store
        .items
        .iter()
        .chain(minted.iter())
        .map(Node::from)
        .collect();
    // `Graph::build` is the dangling-target check: it names the referring task
    // and the absent one.
    let graph = Graph::build(&nodes)
        .map_err(|err| tagged_err(ErrorKind::Validation, None, err.to_string()))?;

    let cycle = graph.cycle_members();
    if !cycle.is_empty() {
        let members: Vec<String> = cycle.iter().map(u32::to_string).collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing to add: the dependency graph would contain a cycle through tasks {}",
                members.join(", ")
            ),
        ));
    }

    let rounds = graph.kahn_rounds();
    let outcomes = minted
        .iter()
        .map(|row| AddOutcome {
            id: row.id,
            r#ref: row.r#ref.clone(),
            batch: rounds
                .iter()
                .position(|round| round.contains(&row.id))
                .expect("an acyclic graph emits every task in a round"),
        })
        .collect();

    store.items.extend(minted);
    Ok(outcomes)
}

/// A grouping key must denote a group that exists. Every checkpoint product
/// selects members by exact match, so a key no `[[checkpoints]]` entry declares
/// puts the row in no closure and no drain, with nothing downstream reporting
/// it missing. An empty key is the "no group" spelling and stays legal.
pub(super) fn reject_undeclared_checkpoint(
    store: &Store,
    checkpoint: &str,
    subject: &str,
) -> Result<()> {
    if checkpoint.is_empty()
        || store
            .checkpoints
            .iter()
            .any(|declared| declared.id == checkpoint)
    {
        return Ok(());
    }
    let declared: Vec<&str> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    let known = if declared.is_empty() {
        "the store declares no checkpoint group".to_string()
    } else {
        format!("declared groups are {}", declared.join(", "))
    };
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!("{subject}: unknown checkpoint group `{checkpoint}` — {known}"),
    ))
}

fn build_row(task: NewTask, id: u32, taken: &mut BTreeSet<String>) -> Result<TaskRow> {
    let NewTask {
        title,
        effort,
        files,
        needs,
        coupling,
        deps_note,
        checkpoint,
        action,
        detail,
        acceptance,
    } = task;

    if title.trim().is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            "a task needs a non-empty `title`".to_string(),
        ));
    }
    let effort = Effort::parse(&effort).ok_or_else(|| {
        tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "task `{title}`: unknown effort `{effort}` — expected one of {}",
                Effort::VOCABULARY.join(", ")
            ),
        )
    })?;
    let derived = slug::derive_ref(&title);
    if derived.is_empty() {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("task `{title}`: the title carries no slug characters, so `ref` is empty"),
        ));
    }
    let r#ref = unique_ref(derived, taken);
    taken.insert(r#ref.clone());

    Ok(TaskRow {
        id,
        r#ref,
        title,
        effort,
        status: Status::default(),
        checkpoint,
        // Heading structure is authored in the plan and re-read by every
        // import, so a value set here would not outlive the next one.
        phase: String::new(),
        phase_depth: 0,
        heading_depth: DEFAULT_HEADING_DEPTH,
        files,
        needs,
        coupling,
        deps_note,
        action,
        detail,
        acceptance,
        agent: String::new(),
        commit: String::new(),
    })
}

/// `slug::dedupe_refs` numbers a whole document in one pass and so cannot see
/// what an append is landing beside — replaying it over `existing ++ new`
/// re-derives a suffix another row already holds. Scanning for the first free
/// suffix keeps that function's `<ref>-2` spelling without the collision.
fn unique_ref(derived: String, taken: &BTreeSet<String>) -> String {
    if !taken.contains(&derived) {
        return derived;
    }
    for n in 2u32.. {
        let candidate = format!("{derived}-{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("the suffix search is unbounded")
}

fn task_from_json(value: &JsonValue, row: usize) -> Result<NewTask> {
    let object = value.as_object().ok_or_else(|| {
        row_err(
            row,
            format!("expected a JSON object, got {}", json_type_name(value)),
        )
    })?;
    for key in object.keys() {
        if !ROW_FIELDS.contains(&key.as_str()) {
            return Err(row_err(
                row,
                format!(
                    "unknown field `{key}` — accepted fields are {}",
                    ROW_FIELDS.join(", ")
                ),
            ));
        }
    }

    Ok(NewTask {
        title: json_str(value, "title", row)?,
        effort: json_str(value, "effort", row)?,
        files: json_strings(value, "files", row)?,
        needs: json_ids(value, "needs", row)?,
        coupling: json_ids(value, "coupling", row)?,
        deps_note: json_str(value, "deps_note", row)?,
        checkpoint: json_str(value, "checkpoint", row)?,
        action: json_str(value, "action", row)?,
        detail: json_str(value, "detail", row)?,
        acceptance: json_str(value, "acceptance", row)?,
    })
}

fn json_str(value: &JsonValue, key: &str, row: usize) -> Result<String> {
    match value.get(key) {
        None | Some(JsonValue::Null) => Ok(String::new()),
        Some(JsonValue::String(text)) => Ok(text.clone()),
        Some(other) => Err(row_err(
            row,
            format!("`{key}` must be a string, got {}", json_type_name(other)),
        )),
    }
}

fn json_strings(value: &JsonValue, key: &str, row: usize) -> Result<Vec<String>> {
    let Some(raw) = value.get(key).filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let array = raw.as_array().ok_or_else(|| {
        row_err(
            row,
            format!("`{key}` must be an array, got {}", json_type_name(raw)),
        )
    })?;
    array
        .iter()
        .map(|entry| match entry {
            JsonValue::String(text) => Ok(text.clone()),
            other => Err(row_err(
                row,
                format!(
                    "`{key}` holds a non-string entry ({})",
                    json_type_name(other)
                ),
            )),
        })
        .collect()
}

fn json_ids(value: &JsonValue, key: &str, row: usize) -> Result<Vec<u32>> {
    let Some(raw) = value.get(key).filter(|v| !v.is_null()) else {
        return Ok(Vec::new());
    };
    let array = raw.as_array().ok_or_else(|| {
        row_err(
            row,
            format!("`{key}` must be an array, got {}", json_type_name(raw)),
        )
    })?;
    array
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| row_err(row, format!("`{key}` holds a non-id entry `{entry}`")))
        })
        .collect()
}

fn row_err(row: usize, message: String) -> anyhow::Error {
    tagged_err(
        ErrorKind::Validation,
        None,
        format!("ndjson row {row}: {message}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::with_root;

    fn write_args() -> WriteIntegrityArgs {
        WriteIntegrityArgs {
            allow_outside: false,
            no_write_integrity: false,
            verify_integrity: false,
            strict_integrity: false,
            no_create: false,
        }
    }

    fn store_path(root: &Path) -> PathBuf {
        root.join(".claude")
            .join("flows")
            .join("whimsical-hugging-puppy")
            .join("tasks.toml")
    }

    fn task(title: &str, needs: &[u32]) -> NewTask {
        NewTask {
            title: title.to_string(),
            effort: "S".to_string(),
            files: Vec::new(),
            needs: needs.to_vec(),
            coupling: Vec::new(),
            deps_note: String::new(),
            checkpoint: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
        }
    }

    /// Two rows, ids 1 and 2, with 2 depending on 1.
    fn seeded(path: &Path) {
        add(path, &write_args(), task("Seed the store", &[])).expect("first row lands");
        add(path, &write_args(), task("Wire the renderer", &[1])).expect("second row lands");
    }

    #[test]
    fn an_absent_dependency_target_is_named() {
        with_root(|root| {
            let path = store_path(root);
            let message = add(&path, &write_args(), task("Dangling", &[99]))
                .expect_err("99 does not exist")
                .to_string();
            assert!(message.contains("99"), "{message}");
            assert!(!path.exists(), "a refused add must persist nothing");
        });
    }

    #[test]
    fn a_cycle_is_refused_and_the_file_is_untouched() {
        with_root(|root| {
            let path = store_path(root);
            seeded(&path);
            let before = fs::read(&path).expect("the seeded store is on disk");

            // The next id is 3, so `--needs 3` closes a loop onto the row
            // being added.
            let message = add(&path, &write_args(), task("Self referential", &[3]))
                .expect_err("a self-edge is a cycle")
                .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(message.contains('3'), "{message}");

            assert_eq!(
                fs::read(&path).expect("the store is still on disk"),
                before,
                "validation must run before the write"
            );
        });
    }

    #[test]
    fn a_batch_closing_a_cycle_between_its_own_rows_is_refused() {
        with_root(|root| {
            let path = store_path(root);
            seeded(&path);
            let before = fs::read(&path).expect("the seeded store is on disk");

            let ndjson = "{\"title\":\"Alpha\",\"effort\":\"S\",\"needs\":[4]}\n\
                          {\"title\":\"Beta\",\"effort\":\"S\",\"needs\":[3]}\n";
            let message = add_many(&path, &write_args(), ndjson)
                .expect_err("3 and 4 depend on each other")
                .to_string();
            assert!(message.contains("cycle"), "{message}");
            assert!(message.contains("3, 4"), "{message}");

            assert_eq!(
                fs::read(&path).expect("the store is still on disk"),
                before,
                "an all-or-nothing batch must not half-land"
            );
        });
    }

    #[test]
    fn a_batch_lands_once_and_reports_each_rows_round() {
        with_root(|root| {
            let path = store_path(root);
            let ndjson = "{\"title\":\"Alpha\",\"effort\":\"S\"}\n\
                          {\"title\":\"Beta\",\"effort\":\"M\",\"needs\":[1]}\n";
            let outcomes = add_many(&path, &write_args(), ndjson).expect("the batch lands");

            assert_eq!(
                outcomes
                    .iter()
                    .map(|o| (o.id, o.r#ref.as_str(), o.batch))
                    .collect::<Vec<_>>(),
                vec![(1, "alpha", 0), (2, "beta", 1)]
            );

            let store = store::load(
                &path,
                &crate::cli::ReadIntegrityArgs {
                    verify_integrity: true,
                    strict_read: false,
                },
            )
            .expect("the store loads");
            assert_eq!(store.items.len(), 2);
            assert_eq!(store.items[1].effort, Effort::M);
            assert_eq!(store.items[1].status, Status::Pending);
        });
    }

    #[test]
    fn a_repeated_title_takes_the_next_free_suffix() {
        with_root(|root| {
            let path = store_path(root);
            let first = add(&path, &write_args(), task("Same title", &[])).expect("first lands");
            let second = add(&path, &write_args(), task("Same title", &[])).expect("second lands");
            let third = add(&path, &write_args(), task("Same title", &[])).expect("third lands");
            assert_eq!(
                (
                    first.r#ref.as_str(),
                    second.r#ref.as_str(),
                    third.r#ref.as_str()
                ),
                ("same-title", "same-title-2", "same-title-3")
            );
        });
    }

    #[test]
    fn an_unknown_ndjson_field_is_refused_by_line() {
        with_root(|root| {
            let path = store_path(root);
            let message = add_many(
                &path,
                &write_args(),
                "{\"title\":\"Alpha\",\"effort\":\"S\"}\n{\"title\":\"Beta\",\"effort\":\"S\",\"neds\":[1]}\n",
            )
            .expect_err("`neds` is not a row field")
            .to_string();
            assert!(message.contains("ndjson row 2"), "{message}");
            assert!(message.contains("neds"), "{message}");
            assert!(!path.exists(), "a refused batch must persist nothing");
        });
    }

    #[test]
    fn a_body_flag_wins_over_its_file_and_neither_is_blank() {
        let dir = tempfile::tempdir().expect("a scratch dir");
        let file = dir.path().join("action.md");
        fs::write(&file, "from the file").expect("the body file is written");

        assert_eq!(
            body(Some("literal".to_string()), Some(file.clone())).expect("the literal wins"),
            "literal"
        );
        assert_eq!(
            body(None, Some(file)).expect("the file is read"),
            "from the file"
        );
        assert_eq!(body(None, None).expect("both absent"), "");
        assert!(body(None, Some(dir.path().join("absent.md"))).is_err());
    }

    #[test]
    fn a_checkpoint_group_the_store_does_not_declare_is_refused() {
        with_root(|root| {
            let path = store_path(root);
            let mut stray = task("Into a group that is not there", &[]);
            stray.checkpoint = "Z".to_string();
            let message = add(&path, &write_args(), stray)
                .expect_err("`Z` is declared nowhere")
                .to_string();
            assert!(message.contains('Z'), "{message}");
            assert!(!path.exists(), "a refused add must persist nothing");
        });
    }

    #[test]
    fn an_unknown_effort_names_the_vocabulary() {
        with_root(|root| {
            let path = store_path(root);
            let mut bad = task("Lowercase effort", &[]);
            bad.effort = "s".to_string();
            let message = add(&path, &write_args(), bad)
                .expect_err("effort is case-sensitive")
                .to_string();
            assert!(message.contains("S, M, L"), "{message}");
        });
    }
}
