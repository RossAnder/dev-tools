//! The `check` verb — DAG, checkpoint, policy and plan findings.
//!
//! Every class raised here is decidable from the store alone. Two are not and
//! live where their input does: `checkpoint/marker-mismatch` compares the
//! authored `Checkpoint after` bullet against the derived maximal elements,
//! and that bullet is never stored, so only the importer can raise it;
//! `render/drift` needs the plan document and comes from the renderer.
//!
//! A duplicate id and an edge to an absent task are both graph-build errors,
//! so the rows are scanned for them before any graph exists — which is what
//! lets one run report every defect instead of dying on the first.
//!
//! `files/closure` is deliberately absent: measured at a 68% false-flag rate.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use super::graph::{Graph, Node};
use super::render::Finding;
use super::schema::{Status, Store};

const ERROR: &str = "error";
const WARNING: &str = "warning";

const MAX_PARALLEL: RangeInclusive<u32> = 1..=8;

/// Infallible: a store too broken to build a graph from still reports why,
/// and a cycle only suppresses the classes that need reachability.
pub(crate) fn check(store: &Store) -> Vec<Finding> {
    let mut findings = policy_findings(store);
    findings.extend(duplicate_findings(store));
    findings.extend(dangling_findings(store));
    findings.extend(orphan_task_findings(store));
    findings.extend(orphan_row_findings(store));
    findings.extend(graph_findings(store));
    findings.sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
    findings
}

pub(crate) fn exit_code(findings: &[Finding]) -> i32 {
    i32::from(findings.iter().any(|finding| finding.severity == ERROR))
}

fn policy_findings(store: &Store) -> Vec<Finding> {
    let parallel = store.policy.max_parallel;
    if MAX_PARALLEL.contains(&parallel) {
        return Vec::new();
    }
    vec![Finding {
        class: "policy/max-parallel-range",
        severity: ERROR,
        ids: Vec::new(),
        detail: format!(
            "`policy.max_parallel` is {parallel}, outside the supported range {}–{}",
            MAX_PARALLEL.start(),
            MAX_PARALLEL.end()
        ),
    }]
}

fn duplicate_findings(store: &Store) -> Vec<Finding> {
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for row in &store.items {
        *counts.entry(row.id).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, count)| Finding {
            class: "dag/duplicate-number",
            severity: ERROR,
            ids: vec![id],
            detail: format!("task number {id} is claimed by {count} rows"),
        })
        .collect()
}

fn dangling_findings(store: &Store) -> Vec<Finding> {
    let known: BTreeSet<u32> = store.items.iter().map(|row| row.id).collect();
    store
        .items
        .iter()
        .filter_map(|row| {
            let absent: Vec<u32> = row
                .needs
                .iter()
                .chain(row.coupling.iter())
                .filter(|dep| !known.contains(dep))
                .copied()
                .collect::<BTreeSet<u32>>()
                .into_iter()
                .collect();
            if absent.is_empty() {
                return None;
            }
            Some(Finding {
                class: "dag/dangling-ref",
                severity: ERROR,
                ids: vec![row.id],
                detail: format!("task {} depends on absent {}", row.id, task_list(&absent)),
            })
        })
        .collect()
}

fn orphan_task_findings(store: &Store) -> Vec<Finding> {
    let ids = sorted_ids(store.items.iter().filter(|row| row.checkpoint.is_empty()));
    if ids.is_empty() {
        return Vec::new();
    }
    vec![Finding {
        class: "checkpoint/orphan-task",
        severity: WARNING,
        detail: format!(
            "no checkpoint group holds {} — only the final commit train does",
            task_list(&ids)
        ),
        ids,
    }]
}

/// An empty `last_import_refs` means no import has happened yet, not that
/// every row is orphaned.
fn orphan_row_findings(store: &Store) -> Vec<Finding> {
    if store.last_import_refs.is_empty() {
        return Vec::new();
    }
    let imported: BTreeSet<&str> = store
        .last_import_refs
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<&str>>();
    let orphaned = store
        .items
        .iter()
        .filter(|row| !imported.contains(row.r#ref.as_str()));
    let pending = sorted_ids(orphaned.clone().filter(|row| row.status == Status::Pending));
    let settled = sorted_ids(orphaned.filter(|row| row.status != Status::Pending));

    let mut findings = Vec::new();
    if !pending.is_empty() {
        findings.push(Finding {
            class: "plan/orphan-row",
            severity: WARNING,
            detail: orphan_row_detail(&pending, ""),
            ids: pending,
        });
    }
    if !settled.is_empty() {
        findings.push(Finding {
            class: "plan/orphan-row",
            severity: ERROR,
            detail: orphan_row_detail(
                &settled,
                ", and its status is not pending — a render would resurrect it into `## Tasks`",
            ),
            ids: settled,
        });
    }
    findings
}

fn orphan_row_detail(ids: &[u32], suffix: &str) -> String {
    format!(
        "the last import did not produce the `ref` of {} — renamed or deleted in the plan{suffix}",
        task_list(ids)
    )
}

/// Skipped wholesale when the graph will not build: the scans above already
/// name both causes, and a partial answer would read as a clean bill.
fn graph_findings(store: &Store) -> Vec<Finding> {
    let nodes = nodes(store);
    let Ok(graph) = Graph::build(&nodes) else {
        return Vec::new();
    };

    let cycle = graph.cycle_members();
    if !cycle.is_empty() {
        return vec![Finding {
            class: "dag/cycle",
            severity: ERROR,
            detail: format!("{} form a dependency cycle", task_list(&cycle)),
            ids: cycle,
        }];
    }

    let mut findings = overlap_findings(store, &graph);
    findings.extend(cut_findings(store, &graph));
    findings
}

fn overlap_findings(store: &Store, graph: &Graph<'_>) -> Vec<Finding> {
    let Ok(pairs) = graph.overlap_pairs() else {
        return Vec::new();
    };
    pairs
        .into_iter()
        .map(|(a, b)| Finding {
            class: "dag/unreachable-claim",
            severity: WARNING,
            ids: vec![a, b],
            detail: format!(
                "tasks {a} and {b} both claim {} with no dependency path either way",
                file_list(&shared_files(store, a, b))
            ),
        })
        .collect()
}

/// The ids named are the dependencies that must move earlier — the group's
/// own members are already in the prefix that failed.
fn cut_findings(store: &Store, graph: &Graph<'_>) -> Vec<Finding> {
    let order: Vec<String> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.clone())
        .collect();
    let Ok(groups) = graph.groups(&order) else {
        return Vec::new();
    };

    let mut prefix: BTreeSet<u32> = BTreeSet::new();
    let mut findings = Vec::new();
    for group in &groups {
        prefix.extend(group.members.iter().copied());
        if group.valid_cut {
            continue;
        }
        let mut outside: BTreeSet<u32> = BTreeSet::new();
        for member in &prefix {
            let Ok(up) = graph.closure_up(*member) else {
                return findings;
            };
            outside.extend(up.into_iter().filter(|dep| !prefix.contains(dep)));
        }
        let ids: Vec<u32> = outside.into_iter().collect();
        findings.push(Finding {
            class: "checkpoint/invalid-cut",
            severity: ERROR,
            detail: format!(
                "checkpoint `{}` is not downward-closed: it depends on {}, which no group up to it contains",
                group.id,
                task_list(&ids)
            ),
            ids,
        });
    }
    findings
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

fn shared_files(store: &Store, a: u32, b: u32) -> Vec<String> {
    let files = |id: u32| -> BTreeSet<String> {
        store
            .find(id)
            .map(|row| row.files.iter().cloned().collect())
            .unwrap_or_default()
    };
    let (mine, theirs) = (files(a), files(b));
    mine.intersection(&theirs).cloned().collect()
}

fn sorted_ids<'a>(rows: impl Iterator<Item = &'a super::schema::TaskRow>) -> Vec<u32> {
    let ids: BTreeSet<u32> = rows.map(|row| row.id).collect();
    ids.into_iter().collect()
}

fn task_list(ids: &[u32]) -> String {
    let joined = ids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<String>>()
        .join(", ");
    if ids.len() == 1 {
        format!("task {joined}")
    } else {
        format!("tasks {joined}")
    }
}

fn file_list(files: &[String]) -> String {
    files
        .iter()
        .map(|file| format!("`{file}`"))
        .collect::<Vec<String>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::schema::{Checkpoint, Effort, TaskRow};

    fn row(id: u32, needs: &[u32], files: &[&str], checkpoint: &str) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: checkpoint.to_string(),
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

    /// `last_import_refs` covers every row, so `plan/orphan-row` stays quiet
    /// unless a test removes a ref.
    fn store(items: Vec<TaskRow>, checkpoints: &[&str]) -> Store {
        Store {
            last_import_refs: items.iter().map(|row| row.r#ref.clone()).collect(),
            checkpoints: checkpoints
                .iter()
                .map(|id| Checkpoint {
                    id: (*id).to_string(),
                    rationale: String::new(),
                })
                .collect(),
            items,
            ..Store::default()
        }
    }

    fn classes(findings: &[Finding]) -> Vec<&str> {
        findings.iter().map(|finding| finding.class).collect()
    }

    #[test]
    fn an_orphan_task_and_a_shared_file_pair_are_warnings_that_still_exit_zero() {
        let store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[1], &["b.rs"], "A"),
                row(3, &[], &["a.rs"], "A"),
                row(4, &[], &["d.rs"], ""),
            ],
            &["A"],
        );
        let findings = check(&store);

        assert_eq!(
            classes(&findings),
            vec!["checkpoint/orphan-task", "dag/unreachable-claim"],
            "{findings:?}"
        );
        assert_eq!(exit_code(&findings), 0, "{findings:?}");
        assert!(
            findings.iter().all(|finding| finding.severity == WARNING),
            "{findings:?}"
        );
        assert_eq!(findings[0].ids, vec![4]);
        assert_eq!(findings[1].ids, vec![1, 3]);
        assert!(findings[1].detail.contains("`a.rs`"), "{findings:?}");
    }

    #[test]
    fn a_group_depending_on_a_later_group_is_an_invalid_cut() {
        let store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[3], &["b.rs"], "B"),
                row(3, &[], &["c.rs"], "C"),
            ],
            &["A", "B", "C"],
        );
        let findings = check(&store);

        assert_eq!(classes(&findings), vec!["checkpoint/invalid-cut"]);
        assert_eq!(findings[0].severity, ERROR);
        assert_eq!(findings[0].ids, vec![3]);
        assert!(findings[0].detail.contains("`B`"), "{findings:?}");
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn an_unimported_ref_is_an_error_once_the_row_has_left_pending() {
        let mut store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[], &["b.rs"], "A"),
                row(3, &[], &["c.rs"], "A"),
            ],
            &["A"],
        );
        store.last_import_refs = vec!["task-1".to_string()];
        store.items[1].status = Status::Done;
        let findings = check(&store);

        assert_eq!(
            classes(&findings),
            vec!["plan/orphan-row", "plan/orphan-row"],
            "{findings:?}"
        );
        assert_eq!(findings[0].ids, vec![2]);
        assert_eq!(findings[0].severity, ERROR);
        assert!(findings[0].detail.contains("## Tasks"), "{findings:?}");
        assert_eq!(findings[1].ids, vec![3]);
        assert_eq!(findings[1].severity, WARNING);
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn a_duplicate_number_and_a_dangling_edge_are_reported_together() {
        let store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(1, &[], &["b.rs"], "A"),
                row(2, &[99], &["c.rs"], "A"),
            ],
            &["A"],
        );
        let findings = check(&store);

        assert_eq!(
            classes(&findings),
            vec!["dag/dangling-ref", "dag/duplicate-number"],
            "{findings:?}"
        );
        assert_eq!(findings[0].ids, vec![2]);
        assert!(
            findings[0].detail.contains("absent task 99"),
            "{findings:?}"
        );
        assert_eq!(findings[1].ids, vec![1]);
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn a_cycle_is_reported_without_the_classes_that_need_reachability() {
        let store = store(
            vec![row(1, &[2], &["a.rs"], "A"), row(2, &[1], &["a.rs"], "A")],
            &["A"],
        );
        let findings = check(&store);

        assert_eq!(classes(&findings), vec!["dag/cycle"], "{findings:?}");
        assert_eq!(findings[0].ids, vec![1, 2]);
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn max_parallel_is_an_error_outside_one_through_eight() {
        let mut store = store(vec![row(1, &[], &["a.rs"], "A")], &["A"]);
        for (parallel, expected) in [(0, 1), (1, 0), (8, 0), (9, 1)] {
            store.policy.max_parallel = parallel;
            let findings = check(&store);
            assert_eq!(exit_code(&findings), expected, "{parallel}: {findings:?}");
        }
        store.policy.max_parallel = 9;
        assert_eq!(classes(&check(&store)), vec!["policy/max-parallel-range"]);
    }
}
