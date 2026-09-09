//! The `show` read verb.
//!
//! `--with` picks sections and `id` is always emitted, so a row fetched by a
//! dispatching orchestrator can always be matched back to the id it asked for.
//!
//! The two edge parts are not symmetric: `deps` is the row's own
//! `needs ∪ coupling` targets — what it waits on — while `dependents` is the
//! transitive successor set.

use std::collections::BTreeSet;

use anyhow::{Result, anyhow};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use super::graph::{Graph, nodes_of};
use super::schema::{ImportOverride, Store, TaskRow};
use super::store;
use crate::cli::{ReadIntegrityArgs, ShowPart, TasksTarget};
use crate::errors::{ErrorKind, tagged_err};
use crate::output::print_json;

pub(crate) fn dispatch(
    id: u32,
    target: TasksTarget,
    with: Vec<ShowPart>,
    integrity: ReadIntegrityArgs,
) -> Result<()> {
    let path = store::resolve_store_path(target.slug.as_deref(), target.file.as_deref())?;
    let store = store::load(&path, &integrity)?;
    print_json(&show(&store, id, &with)?)
}

/// An empty `parts` is the summary shape. A cycle or a dangling edge is an
/// error only for `dependents`, which is the one part that needs the graph —
/// a broken DAG still shows its rows.
pub(crate) fn show(store: &Store, id: u32, parts: &[ShowPart]) -> Result<JsonValue> {
    let row = store.find(id).ok_or_else(|| {
        tagged_err(
            ErrorKind::NotFound,
            None,
            format!("no task {id} in the store"),
        )
    })?;
    let parts = if parts.is_empty() {
        &[ShowPart::Summary][..]
    } else {
        parts
    };

    let mut out = if parts.contains(&ShowPart::Summary) {
        summary(row)
    } else {
        JsonMap::from_iter([("id".to_string(), json!(row.id))])
    };
    if parts.contains(&ShowPart::Files) {
        out.insert("files".to_string(), json!(row.files));
    }
    if parts.contains(&ShowPart::Body) {
        out.insert("action".to_string(), json!(row.action));
        out.insert("detail".to_string(), json!(row.detail));
        out.insert("acceptance".to_string(), json!(row.acceptance));
    }
    if parts.contains(&ShowPart::Deps) {
        out.insert("deps".to_string(), JsonValue::Array(deps(store, row)?));
    }
    if parts.contains(&ShowPart::Dependents) {
        out.insert(
            "dependents".to_string(),
            JsonValue::Array(dependents(store, row.id)?),
        );
    }
    if let Some(entry) = store
        .find_override(&row.r#ref)
        .filter(|stamped| !stamped.is_empty())
    {
        out.insert("import_override".to_string(), stamp(entry));
    }
    Ok(JsonValue::Object(out))
}

/// The plan values a hand patch replaced, one key per stamped field — so the
/// row's own `files`/`needs` above are the patch and these are what the import
/// compares the plan against. Unstamped rows carry no key at all, and the
/// summaries nested under `deps`/`dependents` never carry one.
fn stamp(entry: &ImportOverride) -> JsonValue {
    let mut map = JsonMap::new();
    if let Some(files) = &entry.files {
        map.insert("files".to_string(), json!(files));
    }
    if let Some(needs) = &entry.needs {
        map.insert("needs".to_string(), json!(needs));
    }
    JsonValue::Object(map)
}

fn summary(row: &TaskRow) -> JsonMap<String, JsonValue> {
    let mut map = JsonMap::new();
    map.insert("id".to_string(), json!(row.id));
    map.insert("ref".to_string(), json!(row.r#ref));
    map.insert("title".to_string(), json!(row.title));
    map.insert("effort".to_string(), json!(row.effort.as_str()));
    map.insert("status".to_string(), json!(row.status.as_str()));
    map.insert("checkpoint".to_string(), json!(row.checkpoint));
    map.insert("files".to_string(), json!(row.files));
    map.insert("needs".to_string(), json!(row.needs));
    map.insert("coupling".to_string(), json!(row.coupling));
    map
}

fn deps(store: &Store, row: &TaskRow) -> Result<Vec<JsonValue>> {
    let targets: BTreeSet<u32> = row
        .needs
        .iter()
        .chain(row.coupling.iter())
        .copied()
        .collect();
    targets
        .into_iter()
        .map(|dep| {
            let target = store
                .find(dep)
                .ok_or_else(|| anyhow!("task {} depends on absent task {dep}", row.id))?;
            Ok(JsonValue::Object(summary(target)))
        })
        .collect()
}

fn dependents(store: &Store, id: u32) -> Result<Vec<JsonValue>> {
    let nodes = nodes_of(&store.items);
    let graph = Graph::build(&nodes)?;
    graph
        .closure_down(id)?
        .into_iter()
        .filter(|successor| *successor != id)
        .map(|successor| {
            let row = store
                .find(successor)
                .ok_or_else(|| anyhow!("no task {successor} in the store"))?;
            Ok(JsonValue::Object(summary(row)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{DEFAULT_HEADING_DEPTH, Effort, Status};

    fn row(id: u32, title: &str, needs: &[u32], coupling: &[u32]) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: title.to_string(),
            effort: Effort::M,
            status: Status::Pending,
            checkpoint: "A".to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: DEFAULT_HEADING_DEPTH,
            files: vec![format!("tomlctl/src/tasks/t{id}.rs")],
            needs: needs.to_vec(),
            coupling: coupling.to_vec(),
            deps_note: String::new(),
            action: format!("Do task {id}."),
            detail: format!("Detail for {id}."),
            acceptance: format!("Task {id} holds."),
            agent: String::new(),
            commit: String::new(),
        }
    }

    /// Task 12 waits on one `needs` and one `coupling` target and is waited on
    /// transitively by 13 through 14, so `deps` and `dependents` disagree about
    /// every id.
    fn fixture() -> Store {
        Store {
            items: vec![
                row(10, "Build the graph engine", &[], &[]),
                row(11, "Resolve the store", &[], &[]),
                row(12, "Implement the read verbs", &[10], &[11]),
                row(13, "Wire dispatch", &[12], &[]),
                row(14, "Black-box the verbs", &[13], &[]),
            ],
            ..Store::default()
        }
    }

    fn ids(value: &JsonValue, key: &str) -> Vec<u64> {
        value[key]
            .as_array()
            .unwrap_or_else(|| panic!("`{key}` is an array in {value}"))
            .iter()
            .map(|entry| entry["id"].as_u64().expect("each entry carries an id"))
            .collect()
    }

    #[test]
    fn deps_are_the_rows_own_needs_and_coupling_targets() {
        let out = show(&fixture(), 12, &[ShowPart::Deps]).expect("task 12 shows");
        assert_eq!(ids(&out, "deps"), vec![10, 11], "{out}");
        assert_eq!(
            out["deps"][0]["title"], "Build the graph engine",
            "each dep is a full summary, not a bare id: {out}"
        );
        assert_eq!(
            out["deps"][1]["ref"], "task-11",
            "a coupling target is a dep too: {out}"
        );
        assert_eq!(out["id"], 12, "the fetched id is always present: {out}");
    }

    #[test]
    fn dependents_are_transitive_and_exclude_the_row() {
        let out = show(&fixture(), 12, &[ShowPart::Dependents]).expect("task 12 shows");
        assert_eq!(ids(&out, "dependents"), vec![13, 14], "{out}");
    }

    #[test]
    fn an_absent_part_is_absent_from_the_output() {
        let out = show(&fixture(), 12, &[ShowPart::Body]).expect("task 12 shows");
        assert_eq!(out["action"], "Do task 12.", "{out}");
        assert!(out.get("deps").is_none(), "{out}");
        assert!(out.get("title").is_none(), "{out}");
    }

    #[test]
    fn no_with_flag_is_the_summary_shape() {
        let out = show(&fixture(), 12, &[]).expect("task 12 shows");
        let keys: Vec<&str> = out
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "id",
                "ref",
                "title",
                "effort",
                "status",
                "checkpoint",
                "files",
                "needs",
                "coupling"
            ],
            "{out}"
        );
        assert!(out.get("action").is_none(), "{out}");
    }

    /// The stamp is the row's, not the projection's: it rides every `--with`
    /// selection, and only the fields actually stamped appear under it.
    #[test]
    fn a_stamped_row_carries_the_plan_values_the_patch_replaced() {
        let mut store = fixture();
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-12".to_string(),
            files: Some(vec!["tomlctl/src/tasks/old.rs".to_string()]),
            needs: None,
        }];

        let out = show(&store, 12, &[]).expect("task 12 shows");
        assert_eq!(
            out["import_override"],
            json!({"files": ["tomlctl/src/tasks/old.rs"]}),
            "{out}"
        );
        assert_eq!(
            out["files"],
            json!(["tomlctl/src/tasks/t12.rs"]),
            "the row keeps the patch, the stamp keeps the base: {out}"
        );

        let projected = show(&store, 12, &[ShowPart::Body]).expect("task 12 shows");
        assert_eq!(
            projected["import_override"], out["import_override"],
            "{projected}"
        );
    }

    #[test]
    fn an_unstamped_row_carries_no_override_key() {
        let mut store = fixture();
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-13".to_string(),
            files: None,
            needs: Some(vec![10]),
        }];

        let out = show(&store, 12, &[]).expect("task 12 shows");
        assert!(out.get("import_override").is_none(), "{out}");

        let stamped = show(&store, 13, &[]).expect("task 13 shows");
        assert_eq!(
            stamped["import_override"],
            json!({"needs": [10]}),
            "{stamped}"
        );
        assert!(
            stamped["import_override"].get("files").is_none(),
            "an unstamped field is absent from the stamp: {stamped}"
        );
    }

    /// A nested summary is a different row's, and the stamp belongs to the row
    /// that was asked for.
    #[test]
    fn a_nested_dep_summary_carries_no_stamp() {
        let mut store = fixture();
        store.import_overrides = vec![ImportOverride {
            r#ref: "task-10".to_string(),
            files: Some(Vec::new()),
            needs: None,
        }];

        let out = show(&store, 12, &[ShowPart::Deps]).expect("task 12 shows");
        assert!(out["deps"][0].get("import_override").is_none(), "{out}");
        assert!(out.get("import_override").is_none(), "{out}");
    }

    #[test]
    fn an_unknown_id_is_not_found() {
        let err = show(&fixture(), 99, &[]).expect_err("task 99 is absent");
        assert!(err.to_string().contains("no task 99"), "{err}");
    }

    #[test]
    fn a_dangling_dep_target_is_named() {
        let mut store = fixture();
        store.items[2].needs = vec![99];
        let err = show(&store, 12, &[ShowPart::Deps]).expect_err("99 is absent");
        assert!(err.to_string().contains("absent task 99"), "{err}");
    }
}
