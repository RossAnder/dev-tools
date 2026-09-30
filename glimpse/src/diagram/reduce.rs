//! Transitive reduction of the `needs` graph.
//!
//! A `needs` edge `a -> c` is implied when `c` also needs some `b` that `a` already
//! reaches through `needs` edges: drawing it adds a line but no ordering. On a large
//! plan most long edges are of this kind, and each costs a dummy slot in every layer it
//! spans, so the diagram leaves them out unless asked. Coupling edges order nothing, so
//! they are never implied and never carry a path.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::model::Snapshot;

/// The `(from, to)` needs edges another needs path already implies, sorted.
///
/// The snapshot is expected to be acyclic. On a cycle an edge is kept when the path
/// that would imply it leads back to its own source, so no cycle loses every edge.
pub(crate) fn implied_edges(snapshot: &Snapshot) -> Vec<(u32, u32)> {
    let mut needs: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut down: HashMap<u32, Vec<u32>> = HashMap::new();
    for task in &snapshot.tasks {
        let list = needs.entry(task.id).or_default();
        for &from in &task.needs {
            if from != task.id && list.insert(from) {
                down.entry(from).or_default().push(task.id);
            }
        }
    }

    let mut reach: HashMap<u32, HashSet<u32>> = HashMap::new();
    let mut implied = Vec::new();
    for (&to, list) in &needs {
        if list.len() < 2 {
            continue;
        }
        for &from in list {
            let found = list.iter().any(|&via| {
                via != from
                    && reachable(&mut reach, &down, from).contains(&via)
                    && !reachable(&mut reach, &down, via).contains(&from)
            });
            if found {
                implied.push((from, to));
            }
        }
    }
    implied.sort_unstable();
    implied
}

/// Every task `from` reaches by one or more needs edges, computed once per source.
fn reachable<'a>(
    reach: &'a mut HashMap<u32, HashSet<u32>>,
    down: &HashMap<u32, Vec<u32>>,
    from: u32,
) -> &'a HashSet<u32> {
    reach.entry(from).or_insert_with(|| {
        let mut seen = HashSet::new();
        let mut stack: Vec<u32> = down.get(&from).cloned().unwrap_or_default();
        while let Some(id) = stack.pop() {
            if seen.insert(id) {
                stack.extend(down.get(&id).into_iter().flatten().copied());
            }
        }
        seen
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Task, fixture};

    fn snapshot(tasks: &[(u32, &[u32], &[u32])]) -> Snapshot {
        Snapshot {
            tasks: tasks
                .iter()
                .map(|&(id, needs, coupling)| Task {
                    id,
                    needs: needs.to_vec(),
                    coupling: coupling.to_vec(),
                    ..Task::default()
                })
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn a_shortcut_past_a_chain_is_implied() {
        // 1 -> 2 -> 3 -> 4, plus 1 -> 3, 1 -> 4 and 2 -> 4.
        let snap = snapshot(&[
            (1, &[], &[]),
            (2, &[1], &[]),
            (3, &[2, 1], &[]),
            (4, &[3, 1, 2], &[]),
        ]);
        assert_eq!(implied_edges(&snap), vec![(1, 3), (1, 4), (2, 4)]);
    }

    #[test]
    fn a_diamond_keeps_both_sides() {
        let snap = snapshot(&[
            (1, &[], &[]),
            (2, &[1], &[]),
            (3, &[1], &[]),
            (4, &[2, 3], &[]),
        ]);
        assert_eq!(implied_edges(&snap), vec![]);
    }

    #[test]
    fn coupling_neither_implies_nor_is_implied() {
        // 2 couples to 3, so 1 -> 3 has no needs path to stand in for it; the coupling
        // edge 1 ~ 3 alongside the needs chain 1 -> 2 -> 3 stays as well.
        let snap = snapshot(&[(1, &[], &[]), (2, &[1], &[]), (3, &[1], &[2])]);
        assert_eq!(implied_edges(&snap), vec![]);
        let snap = snapshot(&[(1, &[], &[]), (2, &[1], &[]), (3, &[2], &[1])]);
        assert_eq!(implied_edges(&snap), vec![]);
    }

    #[test]
    fn a_cycle_keeps_its_edges() {
        // 1 and 2 need each other and 3 needs both: neither edge into 3 goes.
        let snap = snapshot(&[(1, &[2], &[]), (2, &[1], &[]), (3, &[1, 2], &[])]);
        assert_eq!(implied_edges(&snap), vec![]);
    }

    #[test]
    fn duplicates_self_loops_and_unknown_ids_are_harmless() {
        let snap = snapshot(&[(1, &[1], &[]), (2, &[1, 1], &[]), (3, &[2, 1, 1, 99], &[])]);
        assert_eq!(implied_edges(&snap), vec![(1, 3)]);
    }

    #[test]
    fn the_fixture_has_nothing_implied() {
        assert_eq!(implied_edges(&fixture()), vec![]);
    }
}
