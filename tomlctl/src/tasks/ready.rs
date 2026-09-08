//! The `ready` verb — the dispatchable frontier given a set of in-flight tasks.
//!
//! A cycle is refused rather than answered: Kahn strands its members, and a
//! stranded task reads in a frontier exactly like one that is merely waiting.

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::graph::{Graph, Node};
use super::schema::Store;
use crate::errors::{ErrorKind, tagged_err};

/// `{ready[], held[{id, blocked_on_file, holder}], next[]}`, every id list
/// ascending. `ready` excludes what `held` names.
pub(crate) fn ready(store: &Store, in_flight: &[u32]) -> Result<JsonValue> {
    let nodes = nodes(store);
    let graph = Graph::build(&nodes).map_err(refuse)?;

    let cycle = graph.cycle_members();
    if !cycle.is_empty() {
        let members: Vec<String> = cycle.iter().map(u32::to_string).collect();
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!(
                "refusing the frontier: the dependency graph contains a cycle through tasks {}",
                members.join(", ")
            ),
        ));
    }

    let mut frontier = graph.frontier(in_flight).map_err(refuse)?;
    frontier.ready.sort_unstable();
    frontier.next.sort_unstable();
    frontier.held.sort_by_key(|held| held.id);

    let held: Vec<JsonValue> = frontier
        .held
        .iter()
        .map(|held| {
            json!({
                "id": held.id,
                "blocked_on_file": held.blocked_on_file,
                "holder": held.holder,
            })
        })
        .collect();

    Ok(json!({
        "ready": frontier.ready,
        "held": held,
        "next": frontier.next,
    }))
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

    fn row(id: u32, needs: &[u32], files: &[&str], status: Status) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status,
            checkpoint: "A".to_string(),
            files: files.iter().map(|file| (*file).to_string()).collect(),
            needs: needs.to_vec(),
            coupling: Vec::new(),
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

    /// 1 is done and 2 is in flight; 3 is dispatchable by dependency but
    /// claims 2's file, 4 is dispatchable outright, 5 waits behind the held
    /// task and 6 waits behind 5.
    fn fixture() -> Store {
        store(vec![
            row(1, &[], &["scaffold.rs"], Status::Done),
            row(2, &[1], &["engine.rs"], Status::InProgress),
            row(3, &[1], &["engine.rs", "render.rs"], Status::Pending),
            row(4, &[1], &["import.rs"], Status::Pending),
            row(5, &[3], &["corpus.rs"], Status::Pending),
            row(6, &[5], &["docs.rs"], Status::Pending),
        ])
    }

    #[test]
    fn a_file_claimed_by_an_in_flight_task_holds_an_otherwise_ready_row() {
        let frontier = ready(&fixture(), &[2]).expect("the fixture is acyclic");

        assert_eq!(
            frontier,
            json!({
                "ready": [4],
                "held": [{"id": 3, "blocked_on_file": "engine.rs", "holder": 2}],
                "next": [5],
            })
        );
    }

    #[test]
    fn a_cycle_is_refused_rather_than_answered_with_a_partial_frontier() {
        let cyclic = store(vec![
            row(1, &[3], &[], Status::Pending),
            row(2, &[1], &[], Status::Pending),
            row(3, &[2], &[], Status::Pending),
            row(4, &[], &["free.rs"], Status::Pending),
        ]);

        let err = ready(&cyclic, &[]).expect_err("a cyclic store has no frontier");
        assert!(
            err.to_string().contains("cycle through tasks 1, 2, 3"),
            "{err}"
        );
    }

    #[test]
    fn an_in_flight_id_no_row_carries_is_refused() {
        let err = ready(&fixture(), &[99]).expect_err("99 is not in the store");
        assert!(err.to_string().contains("no task 99"), "{err}");
    }
}
