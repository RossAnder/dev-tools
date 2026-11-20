//! The milestone schedule — each milestone's upstream milestones, its state, and
//! which milestone runs next.
//!
//! A milestone is a `[[checkpoints]]` group. Milestone N is upstream of M when a
//! row of M reaches a row of N through `needs` or `coupling` (the walk
//! `tasks closure --checkpoint` reports as `dependency_closure`), or when M's
//! `after` names N. Commit and run state stay out of the graph engine: this
//! module reads them off the rows and the caller's set of milestones under
//! elaboration, and the graph only answers reachability.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;

use super::graph::{Graph, nodes_of, refuse};
use super::schema::{MilestoneSpec, Status, Store, TaskRow};
use crate::errors::{ErrorKind, tagged_err};

/// The agents whose name on a row with an empty `commit` marks it started: its
/// edits are in the working tree and no gate has committed them yet.
pub(crate) const WORKER_AGENTS: [&str; 2] = ["plan-worker-lite", "plan-worker-deep"];

const ELABORATING: &str = "elaborating";
const DEFERRED: &str = "deferred";
const EMPTY: &str = "empty";
const DONE: &str = "done";
const UNSTAMPED: &str = "unstamped";
const STARTED: &str = "started";
const PARKED: &str = "parked";
const WAITING: &str = "waiting";
const RUNNABLE: &str = "runnable";
const HELD: &str = "held";

/// The pick and every milestone's state, milestones in `[[checkpoints]]` order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Schedule {
    /// `None` when nothing can run.
    pub(crate) pick: Option<String>,
    pub(crate) milestones: Vec<MilestoneState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MilestoneState {
    pub(crate) id: String,
    pub(crate) state: &'static str,
    /// For `waiting`, `deferred` and `elaborating`, the upstream milestones not
    /// `done` or `empty`; for `held`, what holds it. Store order.
    pub(crate) on: Vec<String>,
    /// Rows not `done`.
    pub(crate) left: usize,
}

/// Each milestone id to the ids of its upstream milestones. An `after` naming
/// no stored milestone, a dangling edge and a cycle are refused, since any of
/// them leaves the order unknown.
pub(crate) fn upstream(store: &Store) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let known: BTreeSet<&str> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    let mut up: BTreeMap<String, BTreeSet<String>> = known
        .iter()
        .map(|id| ((*id).to_string(), BTreeSet::new()))
        .collect();

    let nodes = nodes_of(&store.items);
    let graph = Graph::build(&nodes).map_err(refuse)?;
    let milestone_of: BTreeMap<u32, &str> = store
        .items
        .iter()
        .filter(|row| known.contains(row.checkpoint.as_str()))
        .map(|row| (row.id, row.checkpoint.as_str()))
        .collect();

    for row in &store.items {
        let Some(own) = milestone_of.get(&row.id) else {
            continue;
        };
        for reached in graph.closure_up(row.id).map_err(refuse)? {
            if let Some(other) = milestone_of.get(&reached)
                && other != own
                && let Some(set) = up.get_mut(*own)
            {
                set.insert((*other).to_string());
            }
        }
    }

    for spec in &store.milestones {
        let Some(set) = up.get_mut(spec.id.as_str()) else {
            continue;
        };
        for named in &spec.after {
            if !known.contains(named.as_str()) {
                return Err(tagged_err(
                    ErrorKind::Validation,
                    None,
                    format!(
                        "milestone `{}` is after `{named}`, which names no stored milestone",
                        spec.id
                    ),
                ));
            }
            set.insert(named.clone());
        }
    }

    Ok(up)
}

/// Every milestone's state and the pick, from the task store and the ids of
/// the milestones an open elaborate run holds.
pub(crate) fn schedule(store: &Store, elaborating: &BTreeSet<String>) -> Result<Schedule> {
    let up = upstream(store)?;
    let order: Vec<&str> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    let specs: BTreeMap<&str, &MilestoneSpec> = store
        .milestones
        .iter()
        .map(|spec| (spec.id.as_str(), spec))
        .collect();

    // The states a milestone's own rows and elaboration decide; `None` is
    // `waiting` or `runnable`, which depend on the upstream milestones.
    let own: Vec<Option<&'static str>> = order
        .iter()
        .map(|id| {
            let deferred = specs.get(id).is_some_and(|spec| spec.is_deferred());
            let rows: Vec<&TaskRow> = store
                .items
                .iter()
                .filter(|row| row.checkpoint == *id)
                .collect();
            own_state(id, deferred, elaborating, &rows)
        })
        .collect();

    let satisfied: BTreeSet<&str> = order
        .iter()
        .zip(&own)
        .filter(|(_, state)| matches!(state, Some(DONE) | Some(EMPTY)))
        .map(|(id, _)| *id)
        .collect();

    let mut milestones: Vec<MilestoneState> = order
        .iter()
        .zip(&own)
        .map(|(id, state)| {
            let ups = up.get(*id);
            let unmet: Vec<String> = order
                .iter()
                .filter(|other| {
                    ups.is_some_and(|set| set.contains(**other)) && !satisfied.contains(**other)
                })
                .map(|other| (*other).to_string())
                .collect();
            let (state, on) = match state {
                Some(s @ (ELABORATING | DEFERRED)) => (*s, unmet),
                Some(s) => (*s, Vec::new()),
                None if unmet.is_empty() => (RUNNABLE, Vec::new()),
                None => (WAITING, unmet),
            };
            let left = store
                .items
                .iter()
                .filter(|row| row.checkpoint == *id && row.status != Status::Done)
                .count();
            MilestoneState {
                id: (*id).to_string(),
                state,
                on,
                left,
            }
        })
        .collect();

    let ids_in = |state: &str, list: &[MilestoneState]| -> Vec<String> {
        list.iter()
            .filter(|m| m.state == state)
            .map(|m| m.id.clone())
            .collect()
    };

    let unstamped = ids_in(UNSTAMPED, &milestones);
    let started = ids_in(STARTED, &milestones);
    let pick = if !unstamped.is_empty() {
        hold(&mut milestones, None, &unstamped);
        None
    } else if let Some(first) = started.first().cloned() {
        hold(&mut milestones, Some(&first), std::slice::from_ref(&first));
        Some(first)
    } else {
        milestones
            .iter()
            .find(|m| m.state == RUNNABLE)
            .map(|m| m.id.clone())
    };

    Ok(Schedule { pick, milestones })
}

/// Reports every `started` and `runnable` milestone but `pick` as `held` on `on`.
fn hold(milestones: &mut [MilestoneState], pick: Option<&String>, on: &[String]) {
    for milestone in milestones {
        if Some(&milestone.id) == pick {
            continue;
        }
        if milestone.state == STARTED || milestone.state == RUNNABLE {
            milestone.state = HELD;
            milestone.on = on.to_vec();
        }
    }
}

/// The first of `elaborating` through `parked` that fits, in the design's order.
fn own_state(
    id: &str,
    deferred: bool,
    elaborating: &BTreeSet<String>,
    rows: &[&TaskRow],
) -> Option<&'static str> {
    if deferred {
        return Some(if elaborating.contains(id) {
            ELABORATING
        } else {
            DEFERRED
        });
    }
    if rows.is_empty() {
        return Some(EMPTY);
    }
    if rows.iter().all(|row| row.status == Status::Done) {
        return Some(if rows.iter().all(|row| !row.commit.is_empty()) {
            DONE
        } else {
            UNSTAMPED
        });
    }
    if rows.iter().any(|row| is_started(row)) {
        return Some(STARTED);
    }
    let parked = rows.iter().any(|row| row.status == Status::Parked)
        || rows
            .iter()
            .filter(|row| row.status != Status::Done)
            .all(|row| row.status == Status::Deferred);
    parked.then_some(PARKED)
}

fn is_started(row: &TaskRow) -> bool {
    row.commit.is_empty() && WORKER_AGENTS.contains(&row.agent.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Checkpoint, DEFAULT_HEADING_DEPTH, ELABORATION_DEFERRED, Effort};

    fn row(id: u32, checkpoint: &str, needs: &[u32]) -> TaskRow {
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

    fn committed(mut row: TaskRow) -> TaskRow {
        row.status = Status::Done;
        row.agent = "plan-worker-lite".to_string();
        row.commit = "abc1234".to_string();
        row
    }

    fn milestone(id: &str, deferred: bool, after: &[&str]) -> MilestoneSpec {
        MilestoneSpec {
            id: id.to_string(),
            gates: Vec::new(),
            elaboration: if deferred {
                ELABORATION_DEFERRED.to_string()
            } else {
                String::new()
            },
            defer_reason: String::new(),
            gate_probes: Vec::new(),
            after: after.iter().map(|a| (*a).to_string()).collect(),
            ui_states: Vec::new(),
            ui_controls: Vec::new(),
            external_gates: Vec::new(),
        }
    }

    fn store(groups: &[&str], milestones: Vec<MilestoneSpec>, items: Vec<TaskRow>) -> Store {
        Store {
            checkpoints: groups
                .iter()
                .map(|id| Checkpoint {
                    id: (*id).to_string(),
                    rationale: format!("group {id}"),
                })
                .collect(),
            milestones,
            items,
            ..Store::default()
        }
    }

    fn run(store: &Store) -> Schedule {
        schedule(store, &BTreeSet::new()).expect("the store forms a schedule")
    }

    fn state<'a>(schedule: &'a Schedule, id: &str) -> (&'static str, Vec<&'a str>) {
        let m = schedule
            .milestones
            .iter()
            .find(|m| m.id == id)
            .expect("the milestone is reported");
        (m.state, m.on.iter().map(String::as_str).collect())
    }

    /// A done, B deferred, C independent, D needing a row of B and one of C.
    fn four() -> Store {
        store(
            &["A", "B", "C", "D"],
            vec![milestone("B", true, &[])],
            vec![
                committed(row(1, "A", &[])),
                row(2, "B", &[1]),
                row(3, "C", &[]),
                row(4, "D", &[2, 3]),
            ],
        )
    }

    #[test]
    fn a_deferred_milestone_does_not_hold_an_independent_later_one() {
        let schedule = run(&four());

        assert_eq!(schedule.pick.as_deref(), Some("C"));
        assert_eq!(state(&schedule, "A"), (DONE, vec![]));
        assert_eq!(state(&schedule, "B"), (DEFERRED, vec![]));
        assert_eq!(state(&schedule, "C"), (RUNNABLE, vec![]));
        assert_eq!(state(&schedule, "D"), (WAITING, vec!["B", "C"]));
        let left: Vec<usize> = schedule.milestones.iter().map(|m| m.left).collect();
        assert_eq!(left, vec![0, 1, 1, 1]);
    }

    #[test]
    fn a_deferred_milestone_with_an_open_elaborate_run_reads_elaborating() {
        let elaborating: BTreeSet<String> = ["B".to_string()].into();
        let schedule = schedule(&four(), &elaborating).expect("forms a schedule");

        assert_eq!(state(&schedule, "B"), (ELABORATING, vec![]));
        assert_eq!(schedule.pick.as_deref(), Some("C"));
    }

    #[test]
    fn after_holds_a_milestone_no_row_edge_reaches() {
        let schedule = run(&store(
            &["A", "B"],
            vec![milestone("B", false, &["A"])],
            vec![row(1, "A", &[]), row(2, "B", &[])],
        ));

        assert_eq!(state(&schedule, "A"), (RUNNABLE, vec![]));
        assert_eq!(state(&schedule, "B"), (WAITING, vec!["A"]));
        assert_eq!(schedule.pick.as_deref(), Some("A"));
    }

    #[test]
    fn a_started_row_holds_every_other_milestone_on_its_own() {
        let mut started = row(3, "B", &[]);
        started.agent = "plan-worker-deep".to_string();
        started.status = Status::Done;
        let schedule = run(&store(
            &["A", "B", "C"],
            Vec::new(),
            vec![
                row(1, "A", &[]),
                row(2, "B", &[]),
                started,
                row(4, "C", &[]),
            ],
        ));

        assert_eq!(schedule.pick.as_deref(), Some("B"));
        assert_eq!(state(&schedule, "A"), (HELD, vec!["B"]));
        assert_eq!(state(&schedule, "B"), (STARTED, vec![]));
        assert_eq!(state(&schedule, "C"), (HELD, vec!["B"]));
    }

    #[test]
    fn an_unstamped_milestone_empties_the_pick() {
        let mut unstamped = committed(row(2, "A", &[]));
        unstamped.commit = String::new();
        let schedule = run(&store(
            &["A", "B"],
            Vec::new(),
            vec![committed(row(1, "A", &[])), unstamped, row(3, "B", &[])],
        ));

        assert_eq!(schedule.pick, None);
        assert_eq!(state(&schedule, "A"), (UNSTAMPED, vec![]));
        assert_eq!(state(&schedule, "B"), (HELD, vec!["A"]));
    }

    #[test]
    fn a_coupling_edge_makes_a_milestone_upstream() {
        let mut coupled = row(2, "B", &[]);
        coupled.coupling = vec![1];
        let store = store(&["A", "B"], Vec::new(), vec![row(1, "A", &[]), coupled]);

        let up = upstream(&store).expect("forms a graph");
        assert_eq!(up["B"], ["A".to_string()].into());
        assert!(up["A"].is_empty());
        assert_eq!(state(&run(&store), "B"), (WAITING, vec!["A"]));
    }

    #[test]
    fn an_empty_milestone_counts_as_satisfied() {
        let schedule = run(&store(
            &["A", "B"],
            vec![milestone("B", false, &["A"])],
            vec![row(1, "B", &[])],
        ));

        assert_eq!(state(&schedule, "A"), (EMPTY, vec![]));
        assert_eq!(state(&schedule, "B"), (RUNNABLE, vec![]));
        assert_eq!(schedule.pick.as_deref(), Some("B"));
    }

    #[test]
    fn a_deferred_milestone_with_no_rows_is_deferred_and_holds_what_follows_it() {
        let schedule = run(&store(
            &["A", "B"],
            vec![milestone("A", true, &[]), milestone("B", false, &["A"])],
            vec![row(1, "B", &[])],
        ));

        assert_eq!(state(&schedule, "A"), (DEFERRED, vec![]));
        assert_eq!(state(&schedule, "B"), (WAITING, vec!["A"]));
        assert_eq!(schedule.pick, None);
    }

    #[test]
    fn a_parked_unstarted_row_parks_its_milestone_and_the_pick_moves_on() {
        let mut parked = row(1, "A", &[]);
        parked.status = Status::Parked;
        let schedule = run(&store(
            &["A", "B", "C"],
            Vec::new(),
            vec![
                committed(row(4, "A", &[])),
                parked,
                row(2, "B", &[]),
                row(3, "C", &[1]),
            ],
        ));

        assert_eq!(state(&schedule, "A"), (PARKED, vec![]));
        assert_eq!(state(&schedule, "B"), (RUNNABLE, vec![]));
        assert_eq!(state(&schedule, "C"), (WAITING, vec!["A"]));
        assert_eq!(schedule.pick.as_deref(), Some("B"));
    }

    #[test]
    fn a_milestone_whose_unfinished_rows_are_all_deferred_reads_parked() {
        let mut first = row(2, "A", &[]);
        first.status = Status::Deferred;
        let mut second = row(3, "A", &[]);
        second.status = Status::Deferred;
        let schedule = run(&store(
            &["A"],
            Vec::new(),
            vec![committed(row(1, "A", &[])), first, second],
        ));

        assert_eq!(state(&schedule, "A"), (PARKED, vec![]));
        assert_eq!(schedule.milestones[0].left, 2);
        assert_eq!(schedule.pick, None);
    }

    #[test]
    fn a_deferred_milestone_names_the_unfinished_milestone_its_rows_need() {
        let schedule = run(&store(
            &["A", "B"],
            vec![milestone("B", true, &[])],
            vec![row(1, "A", &[]), row(2, "B", &[1])],
        ));

        assert_eq!(state(&schedule, "A"), (RUNNABLE, vec![]));
        assert_eq!(state(&schedule, "B"), (DEFERRED, vec!["A"]));
        assert_eq!(schedule.pick.as_deref(), Some("A"));
    }

    #[test]
    fn an_after_naming_no_stored_milestone_is_refused() {
        let store = store(
            &["A"],
            vec![milestone("A", false, &["Z"])],
            vec![row(1, "A", &[])],
        );

        let err = schedule(&store, &BTreeSet::new()).expect_err("Z is no milestone");
        assert!(err.to_string().contains("`Z`"), "{err}");
    }
}
