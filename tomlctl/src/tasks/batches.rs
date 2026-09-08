//! The `batches` verb — Kahn layers over the stored edges.
//!
//! A cycle is refused rather than layered: the layering drops what a cycle
//! strands, so the answer would silently omit tasks instead of naming them.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::graph::{Graph, Node};
use super::schema::Store;
use crate::errors::{ErrorKind, tagged_err};

/// `{batches[[…]]}` — dependency order, each layer ascending. In-degree is
/// `needs ∪ coupling`, so a coupling edge pushes its dependent a layer back.
pub(crate) fn batches(store: &Store) -> Result<JsonValue> {
    let nodes = nodes(store);
    let graph = Graph::build(&nodes).map_err(refuse)?;

    let cycle = graph.cycle_members();
    if !cycle.is_empty() {
        let members: Vec<String> = cycle.iter().map(u32::to_string).collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing the layering: the dependency graph contains a cycle through tasks {}",
                members.join(", ")
            ),
        ));
    }

    Ok(json!({ "batches": graph.kahn_rounds() }))
}

/// A graph the store cannot form is a `tasks check` finding, not a tool fault.
fn refuse(err: anyhow::Error) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, err.to_string())
}

fn nodes(store: &Store) -> Vec<Node> {
    store
        .items
        .iter()
        .map(|row| Node {
            id: row.id,
            files: row.files.clone(),
            needs: row.needs.clone(),
            coupling: row.coupling.clone(),
            status: row.status.as_str().to_string(),
            checkpoint: row.checkpoint.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Effort, Status, TaskRow};

    fn row(id: u32, needs: &[u32], coupling: &[u32]) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: "A".to_string(),
            files: vec![format!("t{id}.rs")],
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
            items,
            ..Store::default()
        }
    }

    /// 3 needs only 1, so it would share a layer with 2 were its coupling
    /// edge to 2 not counted; 4 is isolated and seeds the first layer.
    fn fixture() -> Store {
        store(vec![
            row(1, &[], &[]),
            row(2, &[1], &[]),
            row(3, &[1], &[2]),
            row(4, &[], &[]),
        ])
    }

    #[test]
    fn layers_run_in_dependency_order_with_coupling_counted_in_the_in_degree() {
        let layers = batches(&fixture()).expect("the fixture is acyclic");

        assert_eq!(layers, json!({"batches": [[1, 4], [2], [3]]}));
    }

    #[test]
    fn a_cycle_is_refused_rather_than_dropped_from_the_layering() {
        let cyclic = store(vec![
            row(1, &[], &[]),
            row(2, &[4], &[]),
            row(3, &[2], &[]),
            row(4, &[3], &[]),
        ]);

        let err = batches(&cyclic).expect_err("a cyclic store has no layering");
        assert!(
            err.to_string().contains("cycle through tasks 2, 3, 4"),
            "{err}"
        );
    }
}
