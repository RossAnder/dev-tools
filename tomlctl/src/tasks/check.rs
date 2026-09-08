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

use super::graph::{Graph, nodes_of};
use super::parse_policy::{CHECKPOINTS_VALUES, GRANULARITY_VALUES, ORIGIN_VALUES};
use super::render::Finding;
use super::schema::{Status, Store};

const ERROR: &str = "error";
const WARNING: &str = "warning";

const MAX_PARALLEL: RangeInclusive<u32> = 1..=8;

/// Infallible: a store too broken to build a graph from still reports why,
/// and a cycle only suppresses the classes that need reachability.
pub(crate) fn check(store: &Store, in_flight: &[u32]) -> Vec<Finding> {
    let mut findings = policy_findings(store);
    let scanned = {
        let mut scanned = duplicate_findings(store);
        scanned.extend(dangling_findings(store));
        scanned
    };
    let build_error_named = !scanned.is_empty();
    findings.extend(scanned);
    findings.extend(orphan_task_findings(store));
    findings.extend(orphan_row_findings(store));
    findings.extend(graph_findings(store, build_error_named, in_flight));
    findings.sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
    findings
}

pub(crate) fn exit_code(findings: &[Finding]) -> i32 {
    i32::from(findings.iter().any(|finding| finding.severity == ERROR))
}

/// The vocabulary fields are stored as strings so that this — not the reader —
/// decides what is out of vocabulary, which is why a hand-edited or
/// externally-produced store is the only one that reaches here in error.
fn policy_findings(store: &Store) -> Vec<Finding> {
    let mut findings = Vec::new();
    let parallel = store.policy.max_parallel;
    if !MAX_PARALLEL.contains(&parallel) {
        findings.push(Finding {
            class: "policy/max-parallel-range",
            severity: ERROR,
            ids: Vec::new(),
            detail: format!(
                "`policy.max_parallel` is {parallel}, outside the supported range {}–{}",
                MAX_PARALLEL.start(),
                MAX_PARALLEL.end()
            ),
        });
    }
    findings.extend(vocabulary_finding(
        "policy/checkpoints-value",
        "policy.checkpoints",
        &store.policy.checkpoints,
        &CHECKPOINTS_VALUES,
    ));
    findings.extend(vocabulary_finding(
        "policy/commit-granularity-value",
        "policy.commit_granularity",
        &store.policy.commit_granularity,
        &GRANULARITY_VALUES,
    ));
    findings.extend(vocabulary_finding(
        "policy/origin-value",
        "policy.origin",
        &store.policy.origin,
        &ORIGIN_VALUES,
    ));
    findings
}

fn vocabulary_finding(
    class: &'static str,
    field: &str,
    value: &str,
    allowed: &[&str],
) -> Option<Finding> {
    if allowed.contains(&value) {
        return None;
    }
    Some(Finding {
        class,
        severity: ERROR,
        ids: Vec::new(),
        detail: format!(
            "`{field}` is `{value}`, outside the vocabulary {}",
            allowed.join(", ")
        ),
    })
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

/// A grouping key that no `[[checkpoints]]` entry declares holds its row no
/// more firmly than an empty one does: the row is in no group's closure and in
/// no milestone drain, so it lands in the same class.
fn orphan_task_findings(store: &Store) -> Vec<Finding> {
    let declared: BTreeSet<&str> = store
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.id.as_str())
        .collect();
    let undeclared = store
        .items
        .iter()
        .filter(|row| !row.checkpoint.is_empty() && !declared.contains(row.checkpoint.as_str()))
        .collect::<Vec<_>>();

    let mut findings = Vec::new();
    let ungrouped = sorted_ids(store.items.iter().filter(|row| row.checkpoint.is_empty()));
    if !ungrouped.is_empty() {
        findings.push(Finding {
            class: "checkpoint/orphan-task",
            severity: WARNING,
            detail: format!(
                "no checkpoint group holds {} — only the final commit train does",
                task_list(&ungrouped)
            ),
            ids: ungrouped,
        });
    }

    let ids = sorted_ids(undeclared.iter().copied());
    if !ids.is_empty() {
        let groups: BTreeSet<&str> = undeclared
            .iter()
            .map(|row| row.checkpoint.as_str())
            .collect();
        let named = groups
            .iter()
            .map(|group| format!("`{group}`"))
            .collect::<Vec<String>>()
            .join(", ");
        findings.push(Finding {
            class: "checkpoint/orphan-task",
            severity: WARNING,
            detail: format!(
                "no `[[checkpoints]]` entry declares {named}, named by {} — only the final commit train holds them",
                task_list(&ids)
            ),
            ids,
        });
    }
    findings
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

/// Skipped wholesale when the graph will not build, since a partial answer
/// would read as a clean bill — but a refusal the row scans did not name is
/// raised as an error, or silence would certify as green a store every graph
/// verb refuses to read.
fn graph_findings(store: &Store, build_error_named: bool, in_flight: &[u32]) -> Vec<Finding> {
    let nodes = nodes_of(&store.items);
    let graph = match Graph::build(&nodes) {
        Ok(graph) => graph,
        Err(_) if build_error_named => return Vec::new(),
        Err(err) => {
            return vec![Finding {
                class: "dag/unbuildable",
                severity: ERROR,
                ids: Vec::new(),
                detail: format!("the task graph cannot be built: {err}"),
            }];
        }
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
    findings.extend(stalled_findings(
        &graph,
        &declared_in_flight(store, in_flight),
    ));
    findings
}

/// An id no row carries is dropped, where the frontier refuses it: `check`
/// reports on the store it was handed, and a typo that emptied a class would
/// read as a clean bill.
fn declared_in_flight(store: &Store, in_flight: &[u32]) -> Vec<u32> {
    let known: BTreeSet<u32> = store.items.iter().map(|row| row.id).collect();
    in_flight
        .iter()
        .copied()
        .filter(|id| known.contains(id))
        .collect()
}

/// The frontier's `blocked` bucket, so a row the caller declares in flight is
/// busy here exactly as it is there. Undeclared, a blocker a live run is
/// working on and one a crashed run abandoned read alike from the store alone,
/// so this is a diagnostic for a human rather than a refusal — the halt
/// trigger for a real stall is the frontier itself.
fn stalled_findings(graph: &Graph<'_>, in_flight: &[u32]) -> Vec<Finding> {
    let Ok(frontier) = graph.frontier(in_flight) else {
        return Vec::new();
    };
    let mut waiting: BTreeMap<u32, (Status, Vec<u32>)> = BTreeMap::new();
    for blocked in &frontier.blocked {
        waiting
            .entry(blocked.blocker)
            .or_insert_with(|| (blocked.blocker_status, Vec::new()))
            .1
            .push(blocked.id);
    }
    waiting
        .into_iter()
        .map(|(blocker, (status, dependents))| Finding {
            class: "dag/stalled-dependency",
            severity: WARNING,
            ids: vec![blocker],
            detail: format!(
                "task {blocker} is `{status}`, so {} cannot be reached by any wave until it is done",
                task_list(&dependents)
            ),
        })
        .collect()
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
    use crate::tasks::schema::{Checkpoint, DEFAULT_HEADING_DEPTH, Effort, TaskRow};

    fn row(id: u32, needs: &[u32], files: &[&str], checkpoint: &str) -> TaskRow {
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
        let findings = check(&store, &[]);

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
        let findings = check(&store, &[]);

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
        let findings = check(&store, &[]);

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
        let findings = check(&store, &[]);

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
        let findings = check(&store, &[]);

        assert_eq!(classes(&findings), vec!["dag/cycle"], "{findings:?}");
        assert_eq!(findings[0].ids, vec![1, 2]);
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn a_checkpoint_that_no_entry_declares_orphans_its_task() {
        let store = store(
            vec![row(1, &[], &["a.rs"], "A"), row(2, &[], &["b.rs"], "Z")],
            &["A"],
        );
        let findings = check(&store, &[]);

        assert_eq!(
            classes(&findings),
            vec!["checkpoint/orphan-task"],
            "{findings:?}"
        );
        assert_eq!(findings[0].ids, vec![2]);
        assert_eq!(findings[0].severity, WARNING);
        assert!(findings[0].detail.contains("`Z`"), "{findings:?}");
        assert_eq!(exit_code(&findings), 0);
    }

    #[test]
    fn a_store_the_engine_refuses_to_load_is_an_error_not_a_clean_bill() {
        let items: Vec<TaskRow> = (1..=257).map(|id| row(id, &[], &[], "A")).collect();
        let findings = check(&store(items, &["A"]), &[]);

        assert_eq!(classes(&findings), vec!["dag/unbuildable"], "{findings:?}");
        assert_eq!(exit_code(&findings), 1, "{findings:?}");
    }

    /// 3 sits two hops behind the stall, so a bucket naming only direct
    /// predecessors would leave it out of the count entirely.
    #[test]
    fn a_stalled_row_with_dependents_is_a_warning_naming_the_row_and_its_wake() {
        let mut store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[1], &["b.rs"], "A"),
                row(3, &[2], &["c.rs"], "A"),
            ],
            &["A"],
        );
        store.items[0].status = Status::Failed;
        let findings = check(&store, &[]);

        assert_eq!(
            classes(&findings),
            vec!["dag/stalled-dependency"],
            "{findings:?}"
        );
        assert_eq!(findings[0].severity, WARNING);
        assert_eq!(findings[0].ids, vec![1]);
        assert!(findings[0].detail.contains("`failed`"), "{findings:?}");
        assert!(findings[0].detail.contains("tasks 2, 3"), "{findings:?}");
        assert_eq!(exit_code(&findings), 0, "{findings:?}");

        store.items[0].status = Status::Done;
        assert_eq!(classes(&check(&store, &[])), Vec::<&str>::new());
    }

    /// The mid-run shape of a healthy dispatch, which is why the caller that
    /// knows what it sent has to be able to say so.
    #[test]
    fn a_declared_in_flight_blocker_does_not_stall_the_rows_behind_it() {
        let mut store = store(
            vec![row(1, &[], &["a.rs"], "A"), row(2, &[1], &["b.rs"], "A")],
            &["A"],
        );
        store.items[0].status = Status::InProgress;

        assert_eq!(
            classes(&check(&store, &[])),
            vec!["dag/stalled-dependency"],
            "undeclared, the same row still stalls its dependent"
        );
        assert_eq!(classes(&check(&store, &[1])), Vec::<&str>::new());
        assert_eq!(
            classes(&check(&store, &[99])),
            vec!["dag/stalled-dependency"],
            "an id no row carries is dropped, so it cannot empty the class"
        );
    }

    #[test]
    fn an_out_of_vocabulary_policy_value_names_the_field_and_the_vocabulary() {
        let mut store = store(vec![row(1, &[], &["a.rs"], "A")], &["A"]);
        store.policy.checkpoints = "hourly".to_string();
        store.policy.commit_granularity = "per-hour".to_string();
        let findings = check(&store, &[]);

        assert_eq!(
            classes(&findings),
            vec![
                "policy/checkpoints-value",
                "policy/commit-granularity-value"
            ],
            "{findings:?}"
        );
        assert!(findings[0].detail.contains("hourly"), "{findings:?}");
        assert!(findings[0].detail.contains("milestones"), "{findings:?}");
        assert!(findings[1].detail.contains("per-task"), "{findings:?}");
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn an_out_of_vocabulary_origin_is_an_error() {
        let mut store = store(vec![row(1, &[], &["a.rs"], "A")], &["A"]);
        store.policy.origin = "planned".to_string();
        let findings = check(&store, &[]);

        assert_eq!(
            classes(&findings),
            vec!["policy/origin-value"],
            "{findings:?}"
        );
        assert!(findings[0].detail.contains("planned"), "{findings:?}");
        assert!(findings[0].detail.contains("default"), "{findings:?}");
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn max_parallel_is_an_error_outside_one_through_eight() {
        let mut store = store(vec![row(1, &[], &["a.rs"], "A")], &["A"]);
        for (parallel, expected) in [(0, 1), (1, 0), (8, 0), (9, 1)] {
            store.policy.max_parallel = parallel;
            let findings = check(&store, &[]);
            assert_eq!(exit_code(&findings), expected, "{parallel}: {findings:?}");
        }
        store.policy.max_parallel = 9;
        assert_eq!(
            classes(&check(&store, &[])),
            vec!["policy/max-parallel-range"]
        );
    }
}
