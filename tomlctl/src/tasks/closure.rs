//! The `closure` verb — checkpoint groups and per-task ancestor/descendant sets.
//!
//! Reachability is undefined through a cycle, so both modes surface the
//! engine's refusal instead of returning a set that omits its members.

use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::graph::{Graph, nodes_of};
use super::schema::Store;
use crate::errors::{ErrorKind, tagged_err};

/// The two modes, held apart so a caller cannot name a checkpoint and a
/// direction at once.
pub(crate) enum Target {
    Checkpoint(String),
    Task { id: u32, direction: Direction },
}

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    /// Transitive dependencies (ancestors), inclusive.
    Up,
    /// Transitive dependents (successors), inclusive.
    Down,
}

/// `{checkpoint, members[], maximal[], dependency_closure[], valid_cut}` for a
/// group, or `{task, direction, ids[]}` for one task's walk. Every id list
/// ascending.
pub(crate) fn closure(store: &Store, target: Target) -> Result<JsonValue> {
    let nodes = nodes_of(&store.items);
    let graph = Graph::build(&nodes).map_err(refuse)?;

    match target {
        Target::Checkpoint(id) => checkpoint_closure(store, &graph, &id),
        Target::Task { id, direction } => task_closure(&graph, id, direction),
    }
}

/// `valid_cut` covers the union of this group with every earlier one, so it
/// reads as "committable here", not "self-contained".
///
/// `members` and `dependency_closure` are different sets — the group, and
/// everything the group reaches upward — and both are reported because the
/// rendered `CHECKPOINT` marker prints the second under that same name. A
/// group whose dependencies all sit in earlier groups is the case where they
/// coincide, which is exactly the case a reader cannot use to tell them apart.
fn checkpoint_closure(store: &Store, graph: &Graph<'_>, id: &str) -> Result<JsonValue> {
    let order: Vec<String> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect();
    let groups = graph.groups(&order).map_err(refuse)?;

    let group = groups
        .into_iter()
        .find(|group| group.id == id)
        .ok_or_else(|| {
            let known = if order.is_empty() {
                "no groups".to_string()
            } else {
                order.join(", ")
            };
            tagged_err(
                ErrorKind::Validation,
                None,
                format!("no checkpoint `{id}` in the store: `[[checkpoints]]` names {known}"),
            )
        })?;

    let mut members = group.members;
    members.sort_unstable();
    let mut maximal = group.maximal;
    maximal.sort_unstable();

    let mut reaches: BTreeSet<u32> = BTreeSet::new();
    for member in &members {
        reaches.extend(graph.closure_up(*member).map_err(refuse)?);
    }

    Ok(json!({
        "checkpoint": group.id,
        "members": members,
        "maximal": maximal,
        "dependency_closure": reaches.into_iter().collect::<Vec<u32>>(),
        "valid_cut": group.valid_cut,
    }))
}

fn task_closure(graph: &Graph<'_>, id: u32, direction: Direction) -> Result<JsonValue> {
    let mut ids = match direction {
        Direction::Up => graph.closure_up(id),
        Direction::Down => graph.closure_down(id),
    }
    .map_err(refuse)?;
    ids.sort_unstable();

    Ok(json!({
        "task": id,
        "direction": direction.as_str(),
        "ids": ids,
    }))
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// A graph the store cannot form is a `tasks check` finding, not a tool fault.
fn refuse(err: anyhow::Error) -> anyhow::Error {
    tagged_err(ErrorKind::Validation, None, err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Checkpoint, DEFAULT_HEADING_DEPTH, Effort, Status, TaskRow};

    fn row(id: u32, needs: &[u32], checkpoint: &str) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: checkpoint.to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: DEFAULT_HEADING_DEPTH,
            files: vec![format!("t{id}.rs")],
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

    fn group(id: &str) -> Checkpoint {
        Checkpoint {
            id: id.to_string(),
            rationale: format!("group {id}"),
        }
    }

    /// A diamond over two groups: 2 and 3 are group A's antichain, and 5 is
    /// the only task nothing in the store depends on.
    fn fixture() -> Store {
        Store {
            checkpoints: vec![group("A"), group("B")],
            items: vec![
                row(1, &[], "A"),
                row(2, &[1], "A"),
                row(3, &[1], "A"),
                row(4, &[2, 3], "B"),
                row(5, &[4], "B"),
            ],
            ..Store::default()
        }
    }

    #[test]
    fn a_checkpoint_reports_its_members_its_antichain_and_a_downward_closed_cut() {
        let group =
            closure(&fixture(), Target::Checkpoint("A".to_string())).expect("A is a stored group");

        assert_eq!(
            group,
            json!({
                "checkpoint": "A",
                "members": [1, 2, 3],
                "maximal": [2, 3],
                "dependency_closure": [1, 2, 3],
                "valid_cut": true,
            })
        );
    }

    /// Moving the sink into A leaves 4 — which 5 needs — in the later group.
    #[test]
    fn a_group_reaching_past_its_prefix_is_not_a_valid_cut() {
        let mut store = fixture();
        store.items[4].checkpoint = "A".to_string();

        let group =
            closure(&store, Target::Checkpoint("A".to_string())).expect("A is a stored group");

        assert_eq!(
            group,
            json!({
                "checkpoint": "A",
                "members": [1, 2, 3, 5],
                "maximal": [5],
                "dependency_closure": [1, 2, 3, 4, 5],
                "valid_cut": false,
            })
        );
    }

    #[test]
    fn a_task_walks_up_to_its_dependencies_and_down_to_its_dependents() {
        let store = fixture();

        let up = closure(
            &store,
            Target::Task {
                id: 4,
                direction: Direction::Up,
            },
        )
        .expect("4 is a stored task");
        assert_eq!(
            up,
            json!({"task": 4, "direction": "up", "ids": [1, 2, 3, 4]})
        );

        let down = closure(
            &store,
            Target::Task {
                id: 2,
                direction: Direction::Down,
            },
        )
        .expect("2 is a stored task");
        assert_eq!(
            down,
            json!({"task": 2, "direction": "down", "ids": [2, 4, 5]})
        );
    }

    #[test]
    fn an_unknown_checkpoint_and_an_unknown_task_are_both_refused() {
        let store = fixture();

        let err = closure(&store, Target::Checkpoint("Z".to_string()))
            .expect_err("Z is not a stored group");
        assert!(err.to_string().contains("no checkpoint `Z`"), "{err}");

        let err = closure(
            &store,
            Target::Task {
                id: 99,
                direction: Direction::Up,
            },
        )
        .expect_err("99 is not a stored task");
        assert!(err.to_string().contains("no task 99"), "{err}");
    }

    #[test]
    fn a_cycle_is_refused_rather_than_closed_over() {
        let mut store = fixture();
        store.items[0].needs = vec![5];

        let err = closure(
            &store,
            Target::Task {
                id: 1,
                direction: Direction::Up,
            },
        )
        .expect_err("reachability is undefined through a cycle");
        assert!(err.to_string().contains("contains a cycle"), "{err}");
    }
}
