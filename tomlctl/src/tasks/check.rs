//! The `check` verb — DAG, checkpoint, policy and plan findings.
//!
//! Every class raised here is decidable from the store alone. The rest live
//! where their input does, and the input is always the plan document the store
//! was imported from: the `plan/*` classes come from `import_plan` and from the
//! task grammar in `parse_tasks`, `checkpoint/marker-mismatch` compares the
//! authored `Checkpoint after` bullet against the derived maximal elements —
//! and that bullet is never stored, so only the importer can raise it — and
//! `render/drift` needs the document itself and comes from the renderer. The
//! shared `Finding` type and the severity vocabulary are `finding.rs`'s.
//!
//! A duplicate id and an edge to an absent task are both graph-build errors,
//! so the rows are scanned for them before any graph exists — which is what
//! lets one run report every defect instead of dying on the first.
//!
//! `files/closure` and `dag/symbol-without-edge` read a row's prose rather
//! than its fields; what each is worth is stated at its own declaration.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;

use super::finding::{ERROR, Finding, INFO, NO_TASK, WARNING, quoted_list, task_list};
use super::graph::{Graph, nodes_of};
use super::parse_policy::{CHECKPOINTS_VALUES, GRANULARITY_VALUES, ORIGIN_VALUES};
use super::schema::{Status, Store};

const MAX_PARALLEL: RangeInclusive<u32> = 1..=8;

/// The path roots a `files/closure` candidate must sit under.
const CLOSURE_ROOTS: &[&str] = &["packages/", "apps/", "docs/", "scripts/"];

/// A line carrying one of these describes a path the row reads rather than
/// edits, which the plan format requires to stay off the `Files` line.
const CLOSURE_SKIP_CUES: &[&str] = &["read-only", "model of", "pattern", "cite"];

/// Verbs marking the backticked span after them as something the row brings
/// into being rather than consumes.
const INTRODUCERS: &[&str] = &[
    "add",
    "adds",
    "adding",
    "create",
    "creates",
    "creating",
    "export",
    "exports",
    "exporting",
];

/// Words that may stand between an introducer and its span, or between two
/// spans one introducer covers (`Add \`A\`, \`B\` and \`C\``).
const FILLERS: &[&str] = &[
    "a", "an", "the", "new", "and", "one", "two", "three", "its", "own",
];

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
    findings.extend(closure_findings(store));
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

/// `ids` carries the number alone, so the `ref`s are what name which rows
/// carry it — the store's own key, and the argument every write verb takes.
fn duplicate_findings(store: &Store) -> Vec<Finding> {
    let mut claimed: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for row in &store.items {
        claimed.entry(row.id).or_default().push(row.r#ref.as_str());
    }
    claimed
        .into_iter()
        .filter(|(_, refs)| refs.len() > 1)
        .map(|(id, refs)| Finding {
            class: "dag/duplicate-number",
            severity: ERROR,
            ids: vec![id],
            detail: format!(
                "task number {id} is claimed by {} rows: {}",
                refs.len(),
                quoted_list(&refs, "no row")
            ),
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
                detail: format!(
                    "task {} depends on absent {}",
                    row.id,
                    task_list(&absent, NO_TASK)
                ),
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
                task_list(&ungrouped, NO_TASK)
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
                task_list(&ids, NO_TASK)
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
    let pending: Vec<&super::schema::TaskRow> = orphaned
        .clone()
        .filter(|row| row.status == Status::Pending)
        .collect();
    let settled: Vec<&super::schema::TaskRow> = orphaned
        .filter(|row| row.status != Status::Pending)
        .collect();

    let mut findings = Vec::new();
    if !pending.is_empty() {
        findings.push(Finding {
            class: "plan/orphan-row",
            severity: WARNING,
            detail: orphan_row_detail(&pending, ""),
            ids: sorted_ids(pending.into_iter()),
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
            ids: sorted_ids(settled.into_iter()),
        });
    }
    findings
}

/// `ids` deduplicates and the `ref`s do not, so two rows sharing a number are
/// both named here even though the id list holds one entry.
fn orphan_row_detail(rows: &[&super::schema::TaskRow], suffix: &str) -> String {
    let refs: Vec<&str> = rows.iter().map(|row| row.r#ref.as_str()).collect();
    format!(
        "the last import did not produce {}, held by {} — renamed or deleted in the \
         plan{suffix}",
        quoted_list(&refs, "no `ref`"),
        task_list(&sorted_ids(rows.iter().copied()), NO_TASK)
    )
}

/// Info severity, never warning: as a gate this flagged 80 of 117 path tokens
/// over 28 tasks (`docs/ideas/plan-flow-mechanical-verification.md`) — the
/// plan format requires a read-only reference to stay off the `Files` line.
/// Four roots and no gating leave a list a reader judges.
fn closure_findings(store: &Store) -> Vec<Finding> {
    store
        .items
        .iter()
        .filter_map(|row| {
            let claimed: BTreeSet<&str> = row.files.iter().map(String::as_str).collect();
            let unclaimed: Vec<String> = row
                .action
                .lines()
                .chain(row.detail.lines())
                .filter(|line| !skipped_line(line))
                .flat_map(backticked)
                .filter(|span| {
                    CLOSURE_ROOTS.iter().any(|root| span.starts_with(root))
                        && !claimed.contains(span)
                })
                .map(str::to_string)
                .collect::<BTreeSet<String>>()
                .into_iter()
                .collect();
            if unclaimed.is_empty() {
                return None;
            }
            Some(Finding {
                class: "files/closure",
                severity: INFO,
                ids: vec![row.id],
                detail: format!(
                    "task {}'s prose names {}, which its `files` does not claim",
                    row.id,
                    quoted_list(&unclaimed, "no path")
                ),
            })
        })
        .collect()
}

fn skipped_line(line: &str) -> bool {
    let lowered = line.to_ascii_lowercase();
    CLOSURE_SKIP_CUES.iter().any(|cue| lowered.contains(cue))
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
            detail: format!("{} form a dependency cycle", task_list(&cycle, NO_TASK)),
            ids: cycle,
        }];
    }

    let mut findings = overlap_findings(store, &graph);
    findings.extend(symbol_findings(store, &graph));
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
                task_list(&dependents, NO_TASK)
            ),
        })
        .collect()
}

/// The edge `coupling` was meant to carry and nothing populates: a symbol one
/// row's `Action` introduces, named by a row no path reaches. A heuristic over
/// prose, so it warns rather than gates, and it reads only spans an introducing
/// verb covers — an untargeted token scan is what measured unusable.
///
/// Two suppressions, each measured against `docs/plans/`: a row whose `files`
/// are all markdown adds a catalogue entry, not a symbol; and a name more than
/// one row introduces strands nobody who already reaches one of them.
fn symbol_findings(store: &Store, graph: &Graph<'_>) -> Vec<Finding> {
    let introducers: Vec<(u32, BTreeSet<String>)> = store
        .items
        .iter()
        .filter(|row| !documents_only(row))
        .map(|row| (row.id, introduced_symbols(&row.action)))
        .filter(|(_, symbols)| !symbols.is_empty())
        .collect();
    if introducers.is_empty() {
        return Vec::new();
    }
    let mut minted: BTreeMap<&str, BTreeSet<u32>> = BTreeMap::new();
    for (id, symbols) in &introducers {
        for symbol in symbols {
            minted.entry(symbol).or_default().insert(*id);
        }
    }
    let mut named: Vec<(u32, BTreeSet<String>, BTreeSet<u32>)> = Vec::new();
    for row in &store.items {
        let (Ok(up), Ok(down)) = (graph.closure_up(row.id), graph.closure_down(row.id)) else {
            return Vec::new();
        };
        named.push((
            row.id,
            named_symbols(row),
            up.into_iter().chain(down).collect(),
        ));
    }

    let mut findings = Vec::new();
    for (introducer, symbols) in &introducers {
        for (user, uses, linked) in &named {
            if linked.contains(introducer) {
                continue;
            }
            let shared: Vec<String> = symbols
                .intersection(uses)
                .filter(|symbol| {
                    !minted
                        .get(symbol.as_str())
                        .is_some_and(|ids| ids.iter().any(|other| linked.contains(other)))
                })
                .cloned()
                .collect();
            if shared.is_empty() {
                continue;
            }
            let mut ids = vec![*introducer, *user];
            ids.sort_unstable();
            findings.push(Finding {
                class: "dag/symbol-without-edge",
                severity: WARNING,
                ids,
                detail: format!(
                    "task {introducer} introduces {} and task {user} names it, with no \
                     dependency path either way",
                    quoted_list(&shared, "no symbol")
                ),
            });
        }
    }
    findings
}

/// A row claiming only markdown edits documentation, where "add `X`" names an
/// entry about `X` rather than `X` itself. A row claiming nothing is not one.
fn documents_only(row: &super::schema::TaskRow) -> bool {
    !row.files.is_empty() && row.files.iter().all(|file| file.ends_with(".md"))
}

/// Every backticked span an introducer covers, including the rest of a list
/// one introducer opens.
fn introduced_symbols(action: &str) -> BTreeSet<String> {
    let mut symbols = BTreeSet::new();
    let mut carried = false;
    for (lead, span) in spans_with_lead(action) {
        carried = introduces(lead) || (carried && all_fillers(lead));
        if carried && let Some(symbol) = symbol_of(span) {
            symbols.insert(symbol);
        }
    }
    symbols
}

/// A row's whole body, since a symbol is as much used in an acceptance command
/// as in an action.
fn named_symbols(row: &super::schema::TaskRow) -> BTreeSet<String> {
    [&row.action, &row.detail, &row.acceptance]
        .into_iter()
        .flat_map(|text| text.lines())
        .flat_map(backticked)
        .filter_map(symbol_of)
        .collect()
}

/// Each backticked span with the text since the previous one, which is where
/// the introducing verb sits.
fn spans_with_lead(text: &str) -> Vec<(&str, &str)> {
    let parts: Vec<&str> = text.split('`').collect();
    parts
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[lead, span]| (*lead, *span))
        .collect()
}

fn words_of(lead: &str) -> Vec<String> {
    lead.split_whitespace()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// The verb has to be within three words of the span, so `Add typed X` and
/// `Create the new X` introduce and `Create the guard that wraps X` does not.
/// A wider window reads any verb in the sentence as the span's own.
fn introduces(lead: &str) -> bool {
    words_of(lead)
        .iter()
        .rev()
        .take(3)
        .any(|word| INTRODUCERS.contains(&word.as_str()))
}

fn all_fillers(lead: &str) -> bool {
    !lead.contains(['.', ';', ':'])
        && words_of(lead)
            .iter()
            .all(|word| FILLERS.contains(&word.as_str()))
}

/// Identifier-shaped and carrying a code marker — `_`, an upper-case letter or
/// `::`. Without the marker every backticked English word in a plan is a
/// candidate symbol, which is the shape that measured unusable.
fn symbol_of(span: &str) -> Option<String> {
    let token = span.trim().trim_end_matches("()").trim_end_matches('!');
    if token.len() < 3 {
        return None;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
    {
        return None;
    }
    let first = token.chars().next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let marked = token.contains('_')
        || token.contains("::")
        || token.chars().any(|c| c.is_ascii_uppercase());
    marked.then(|| token.to_string())
}

fn backticked(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
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
                quoted_list(&shared_files(store, a, b), "no file")
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
                task_list(&ids, NO_TASK)
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

    /// The id list holds one entry for a number two rows claim, and a `ref` is
    /// what every write verb takes, so the refs are the only part of either
    /// finding a reader can act on.
    #[test]
    fn a_duplicate_number_and_an_orphaned_row_each_name_their_rows() {
        let mut store = store(
            vec![row(1, &[], &["a.rs"], "A"), row(1, &[], &["b.rs"], "A")],
            &["A"],
        );
        store.items[1].r#ref = "task-1-again".to_string();
        store.last_import_refs = vec!["task-1".to_string()];
        let findings = check(&store, &[]);

        let duplicate = findings
            .iter()
            .find(|finding| finding.class == "dag/duplicate-number")
            .expect("two rows claim task 1");
        assert_eq!(duplicate.ids, vec![1]);
        assert!(
            duplicate.detail.contains("`task-1`, `task-1-again`"),
            "{duplicate:?}"
        );

        let orphan = findings
            .iter()
            .find(|finding| finding.class == "plan/orphan-row")
            .expect("the second row's ref was not imported");
        assert_eq!(orphan.ids, vec![1], "the id list deduplicates");
        assert!(orphan.detail.contains("`task-1-again`"), "{orphan:?}");
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

    /// The pair the class exists for: 2 mints what 3 calls, and no edge runs
    /// between them.
    #[test]
    fn a_symbol_named_across_an_absent_edge_is_a_warning_that_an_edge_silences() {
        let mut store = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[], &["b.rs"], "A"),
                row(3, &[], &["c.rs"], "A"),
            ],
            &["A"],
        );
        store.items[1].action = "Add `read_token` beside the session reader.".to_string();
        store.items[2].acceptance = "`read_token` returns the parsed claims.".to_string();
        let findings = check(&store, &[]);

        assert_eq!(
            classes(&findings),
            vec!["dag/symbol-without-edge"],
            "{findings:?}"
        );
        assert_eq!(findings[0].severity, WARNING);
        assert_eq!(findings[0].ids, vec![2, 3]);
        assert!(findings[0].detail.contains("`read_token`"), "{findings:?}");
        assert_eq!(exit_code(&findings), 0, "{findings:?}");

        store.items[2].needs = vec![2];
        assert_eq!(classes(&check(&store, &[])), Vec::<&str>::new());
    }

    /// The two narrowings that separate this from the check measured at a 68%
    /// false-flag rate: only an introducing verb mints a symbol, and only a
    /// marked identifier is one.
    #[test]
    fn a_symbol_needs_an_introducing_verb_and_a_code_marker() {
        let mut store = store(
            vec![row(1, &[], &["a.rs"], "A"), row(2, &[], &["b.rs"], "A")],
            &["A"],
        );
        store.items[0].action = "Call `read_token` through the `session` module.".to_string();
        store.items[1].detail = "`read_token` and `session` are already there.".to_string();
        assert_eq!(
            classes(&check(&store, &[])),
            Vec::<&str>::new(),
            "a consuming verb introduces nothing"
        );

        store.items[0].action = "Add the `session` module.".to_string();
        assert_eq!(
            classes(&check(&store, &[])),
            Vec::<&str>::new(),
            "an unmarked lower-case word is prose, not a symbol"
        );

        store.items[0].action = "Add `session_of` and `Session`.".to_string();
        store.items[1].detail = "`session_of` builds a `Session`.".to_string();
        let findings = check(&store, &[]);
        assert_eq!(findings.len(), 1, "one finding per pair: {findings:?}");
        assert!(findings[0].detail.contains("`Session`, `session_of`"));
    }

    /// Both suppressions, each measured against the plan corpus: a
    /// documentation row's "add `X`" names an entry about `X`, and a name two
    /// rows introduce strands nobody already ordered behind one of them.
    #[test]
    fn a_documentation_row_and_a_reached_second_minter_both_suppress() {
        let mut documented = store(
            vec![
                row(1, &[], &["docs/catalogue.md"], "A"),
                row(2, &[], &["b.rs"], "A"),
            ],
            &["A"],
        );
        documented.items[0].action = "Add `read_token` to the catalogue.".to_string();
        documented.items[1].detail = "`read_token` is already there.".to_string();
        assert_eq!(
            classes(&check(&documented, &[])),
            Vec::<&str>::new(),
            "a markdown-only row introduces nothing"
        );

        let mut layered = store(
            vec![
                row(1, &[], &["a.rs"], "A"),
                row(2, &[], &["b.rs"], "A"),
                row(3, &[1], &["c.rs"], "A"),
            ],
            &["A"],
        );
        layered.items[0].action = "Add `set_status` in the repo layer.".to_string();
        layered.items[1].action = "Add `set_status` to the tool router.".to_string();
        layered.items[2].acceptance = "`set_status` rejects an illegal transition.".to_string();
        assert_eq!(
            classes(&check(&layered, &[])),
            Vec::<&str>::new(),
            "3 already waits on a row that introduces the name"
        );

        layered.items[2].needs = vec![];
        assert_eq!(
            check(&layered, &[])
                .iter()
                .map(|finding| finding.ids.clone())
                .collect::<Vec<Vec<u32>>>(),
            vec![vec![1, 3], vec![2, 3]],
            "reaching neither minter leaves both pairs"
        );
    }

    /// The severity is the contract: a lens reads the list, and no carrier can
    /// gate on it.
    #[test]
    fn an_unclaimed_path_is_info_and_a_read_only_line_is_skipped() {
        let mut store = store(vec![row(1, &[], &["docs/kept.md"], "A")], &["A"]);
        store.items[0].action =
            "Rewrite `docs/kept.md` from `scripts/gen.sh`.\nThe read-only source is `docs/spec.md`."
                .to_string();
        store.items[0].detail = "Mirrors the `packages/shell/README.md` pattern.".to_string();
        let findings = check(&store, &[]);

        assert_eq!(classes(&findings), vec!["files/closure"], "{findings:?}");
        assert_eq!(findings[0].severity, INFO);
        assert_eq!(findings[0].ids, vec![1]);
        assert!(
            findings[0].detail.contains("`scripts/gen.sh`"),
            "{findings:?}"
        );
        assert!(
            !findings[0].detail.contains("spec.md") && !findings[0].detail.contains("README"),
            "a cue line names nothing: {findings:?}"
        );
        assert_eq!(exit_code(&findings), 0, "{findings:?}");
    }

    /// `src/` is out of scope by decision, so the roots are asserted rather
    /// than assumed from one member.
    #[test]
    fn only_the_four_declared_roots_are_reported() {
        let mut store = store(vec![row(1, &[], &[], "A")], &["A"]);
        store.items[0].action =
            "Touch `src/main.rs`, `apps/web/main.ts`, `packages/core/index.ts`, `docs/a.md` \
             and `scripts/b.sh`."
                .to_string();
        let findings = check(&store, &[]);

        assert_eq!(classes(&findings), vec!["files/closure"], "{findings:?}");
        assert!(!findings[0].detail.contains("src/main.rs"), "{findings:?}");
        for path in [
            "apps/web/main.ts",
            "packages/core/index.ts",
            "docs/a.md",
            "scripts/b.sh",
        ] {
            assert!(findings[0].detail.contains(path), "{path}: {findings:?}");
        }
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
