//! The `backlog/*` findings: a store's `[[backlog_links]]` joined against
//! `.claude/backlog.toml`. `check::check` reads the store alone, so these are
//! raised beside it by the callers that can reach the backlog.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use toml::Value as TomlValue;

use super::finding::{ERROR, Finding, INFO, WARNING, quoted_list};
use super::schema::Store;
use crate::backlog::schema::{
    ARRAY_BACKLOG, ARRAY_COMPACTED, FIELD_PROMOTED_TO, FIELD_STATUS, STATUS_DISMISSED, STATUS_OPEN,
    STATUS_PROMOTED, STATUS_RESOLVED, read_store,
};
use crate::cli::ReadIntegrityArgs;
use crate::io::{item_id, items_array};

/// The task refs linking one backlog id, by link strength.
#[derive(Default)]
struct Linkers<'a> {
    closes: BTreeSet<&'a str>,
    refs: BTreeSet<&'a str>,
}

/// A missing store reads as an empty table, so every link in it is
/// `backlog/unknown-id` rather than an I/O error.
pub(crate) fn load_backlog() -> Result<TomlValue> {
    read_store(&ReadIntegrityArgs {
        verify_integrity: false,
        strict_read: false,
    })
}

/// `flow` is the store's slug; `None` suppresses `backlog/claimed-elsewhere`,
/// since a claim naming the slug cannot be told from one naming another flow.
pub(crate) fn backlog_findings(
    store: &Store,
    backlog: &TomlValue,
    flow: Option<&str>,
) -> Vec<Finding> {
    let mut linked: BTreeMap<&str, Linkers<'_>> = BTreeMap::new();
    for link in &store.backlog_links {
        for id in &link.closes {
            linked
                .entry(id.as_str())
                .or_default()
                .closes
                .insert(link.r#ref.as_str());
        }
        for id in &link.refs {
            linked
                .entry(id.as_str())
                .or_default()
                .refs
                .insert(link.r#ref.as_str());
        }
    }
    let mut findings: Vec<Finding> = linked
        .iter()
        .filter_map(|(id, linkers)| item_finding(store, backlog, flow, id, linkers))
        .collect();
    findings.sort_by(|a, b| (a.class, &a.ids).cmp(&(b.class, &b.ids)));
    findings
}

/// Reads `.claude/backlog.toml` only when the store links to an item, so a
/// link-free store never depends on the backlog parsing.
pub(super) fn linked_backlog_findings(store: &Store, flow: Option<&str>) -> Result<Vec<Finding>> {
    if store.backlog_links.is_empty() {
        return Ok(Vec::new());
    }
    Ok(backlog_findings(store, &load_backlog()?, flow))
}

/// At most one per item: the statuses the classes key on are exclusive.
fn item_finding(
    store: &Store,
    backlog: &TomlValue,
    flow: Option<&str>,
    id: &str,
    linkers: &Linkers<'_>,
) -> Option<Finding> {
    let every: Vec<&str> = linkers.closes.union(&linkers.refs).copied().collect();
    let Some((row, compacted)) = find_row(backlog, id) else {
        return Some(Finding {
            class: "backlog/unknown-id",
            severity: ERROR,
            ids: task_ids(store, &every),
            detail: format!(
                "`{id}`, linked by {}, is in neither the `{ARRAY_BACKLOG}` nor the \
                 `{ARRAY_COMPACTED}` array of the backlog",
                quoted_list(&every, "no task")
            ),
        });
    };
    let status = str_field(row, FIELD_STATUS);
    if compacted || status == STATUS_RESOLVED || status == STATUS_DISMISSED {
        let state = match compacted {
            true => format!("compacted as `{status}`"),
            false => format!("already `{status}`"),
        };
        return Some(Finding {
            class: "backlog/closed",
            severity: INFO,
            ids: task_ids(store, &every),
            detail: format!(
                "`{id}`, linked by {}, is {state}",
                quoted_list(&every, "no task")
            ),
        });
    }

    let closers: Vec<&str> = linkers.closes.iter().copied().collect();
    if closers.is_empty() {
        return None;
    }
    if status == STATUS_OPEN {
        return Some(Finding {
            class: "backlog/unpromoted",
            severity: WARNING,
            ids: task_ids(store, &closers),
            detail: format!(
                "`{id}` is closed by {} but is still `{STATUS_OPEN}` in the backlog",
                quoted_list(&closers, "no task")
            ),
        });
    }
    let target = str_field(row, FIELD_PROMOTED_TO);
    if status == STATUS_PROMOTED
        && let Some(flow) = flow
        && !claims(store, flow, target)
    {
        return Some(Finding {
            class: "backlog/claimed-elsewhere",
            severity: WARNING,
            ids: task_ids(store, &closers),
            detail: format!(
                "`{id}` is closed by {} but promoted to `{target}`, which is neither \
                 `{flow}` nor its plan",
                quoted_list(&closers, "no task")
            ),
        });
    }
    None
}

/// Whether a `promoted_to` names this flow, by slug or by plan path — a
/// target written on Windows carries `\` where the store's plan path has `/`.
fn claims(store: &Store, flow: &str, target: &str) -> bool {
    let target = target.replace('\\', "/");
    target == flow || (!store.plan_path.is_empty() && target == store.plan_path.replace('\\', "/"))
}

/// The row carrying `id`, and whether it sits in the compacted array.
fn find_row<'a>(backlog: &'a TomlValue, id: &str) -> Option<(&'a TomlValue, bool)> {
    [ARRAY_BACKLOG, ARRAY_COMPACTED]
        .into_iter()
        .find_map(|array| {
            items_array(backlog, array)
                .iter()
                .find(|row| item_id(row) == Some(id))
                .map(|row| (row, array == ARRAY_COMPACTED))
        })
}

fn str_field<'a>(row: &'a TomlValue, field: &str) -> &'a str {
    row.get(field)
        .and_then(TomlValue::as_str)
        .unwrap_or_default()
}

/// A ref no row carries contributes no id; the detail still names it.
fn task_ids(store: &Store, refs: &[&str]) -> Vec<u32> {
    refs.iter()
        .filter_map(|r#ref| store.find_ref(r#ref))
        .map(|row| row.id)
        .collect::<BTreeSet<u32>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::check::exit_code;
    use crate::tasks::schema::{BacklogLink, DEFAULT_HEADING_DEPTH, Effort, Status, TaskRow};
    use crate::test_support::with_root;

    const FLOW: &str = "unified-singing-gem";
    const PLAN: &str = "docs/plans/unified-singing-gem.md";

    fn row(id: u32) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status: Status::Pending,
            checkpoint: String::new(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: DEFAULT_HEADING_DEPTH,
            files: Vec::new(),
            needs: Vec::new(),
            coupling: Vec::new(),
            deps_note: String::new(),
            action: String::new(),
            detail: String::new(),
            acceptance: String::new(),
            agent: String::new(),
            commit: String::new(),
        }
    }

    /// Tasks 1 and 2, each linking the ids given against its ref.
    fn store(links: &[(u32, &[&str], &[&str])]) -> Store {
        Store {
            plan_path: PLAN.to_string(),
            backlog_links: links
                .iter()
                .map(|(id, closes, refs)| BacklogLink {
                    r#ref: format!("task-{id}"),
                    closes: closes.iter().map(|id| (*id).to_string()).collect(),
                    refs: refs.iter().map(|id| (*id).to_string()).collect(),
                })
                .collect(),
            items: vec![row(1), row(2)],
            ..Store::default()
        }
    }

    fn backlog(text: &str) -> TomlValue {
        toml::from_str(text).expect("the fixture parses")
    }

    const BACKLOG: &str = r#"
[[backlog]]
id = "B-0pen0001"
summary = "open"
status = "open"

[[backlog]]
id = "B-here0001"
summary = "promoted here by slug"
status = "promoted"
promoted = 2026-09-01
promoted_to = "unified-singing-gem"

[[backlog]]
id = "B-here0002"
summary = "promoted here by plan path"
status = "promoted"
promoted = 2026-09-01
promoted_to = 'docs\plans\unified-singing-gem.md'

[[backlog]]
id = "B-e1se0001"
summary = "promoted elsewhere"
status = "promoted"
promoted = 2026-09-01
promoted_to = "other-flow"

[[backlog]]
id = "B-d0ne0001"
summary = "resolved"
status = "resolved"
resolved = 2026-09-01
resolution = "fixed"

[[backlog]]
id = "B-d1sm0001"
summary = "dismissed"
status = "dismissed"
dismissed = 2026-09-01
dismiss_reason = "wontfix"

[[compacted]]
id = "B-c0mp0001"
summary = "folded while promoted"
status = "promoted"
promoted_to = "other-flow"
"#;

    fn classes(findings: &[Finding]) -> Vec<(&str, &str, Vec<u32>)> {
        findings
            .iter()
            .map(|finding| (finding.class, finding.severity, finding.ids.clone()))
            .collect()
    }

    #[test]
    fn an_id_in_neither_array_is_an_error_naming_every_linking_task() {
        let store = store(&[(1, &["B-m1ss1ng1"], &[]), (2, &[], &["B-m1ss1ng1"])]);
        let findings = backlog_findings(&store, &backlog(BACKLOG), Some(FLOW));

        assert_eq!(
            classes(&findings),
            vec![("backlog/unknown-id", ERROR, vec![1, 2])],
            "{findings:?}"
        );
        assert!(findings[0].detail.contains("`B-m1ss1ng1`"), "{findings:?}");
        assert_eq!(exit_code(&findings), 1);
    }

    #[test]
    fn a_closed_open_item_is_unpromoted() {
        let store = store(&[(2, &["B-0pen0001"], &[])]);
        let findings = backlog_findings(&store, &backlog(BACKLOG), Some(FLOW));

        assert_eq!(
            classes(&findings),
            vec![("backlog/unpromoted", WARNING, vec![2])],
            "{findings:?}"
        );
    }

    #[test]
    fn a_claim_on_another_flow_is_claimed_elsewhere_and_this_flow_is_not() {
        let store = store(&[(1, &["B-e1se0001", "B-here0001", "B-here0002"], &[])]);
        let findings = backlog_findings(&store, &backlog(BACKLOG), Some(FLOW));

        assert_eq!(
            classes(&findings),
            vec![("backlog/claimed-elsewhere", WARNING, vec![1])],
            "a claim by slug or by a `\\` plan path is this flow's: {findings:?}"
        );
        assert!(findings[0].detail.contains("`other-flow`"), "{findings:?}");
    }

    #[test]
    fn a_resolved_dismissed_or_compacted_item_is_closed_at_info() {
        let store = store(&[
            (1, &["B-d0ne0001", "B-d1sm0001"], &[]),
            (2, &["B-c0mp0001"], &[]),
        ]);
        let findings = backlog_findings(&store, &backlog(BACKLOG), Some(FLOW));

        assert_eq!(
            classes(&findings),
            vec![
                ("backlog/closed", INFO, vec![1]),
                ("backlog/closed", INFO, vec![1]),
                ("backlog/closed", INFO, vec![2]),
            ],
            "{findings:?}"
        );
        assert!(findings[2].detail.contains("compacted"), "{findings:?}");
        assert_eq!(exit_code(&findings), 0);
    }

    #[test]
    fn a_refs_link_gates_nothing_on_an_open_or_foreign_item() {
        let store = store(&[(1, &[], &["B-0pen0001", "B-e1se0001"])]);
        assert_eq!(
            backlog_findings(&store, &backlog(BACKLOG), Some(FLOW)),
            Vec::<Finding>::new()
        );
    }

    #[test]
    fn no_flow_slug_raises_no_claimed_elsewhere() {
        let store = store(&[(1, &["B-e1se0001"], &[]), (2, &["B-0pen0001"], &[])]);
        let findings = backlog_findings(&store, &backlog(BACKLOG), None);

        assert_eq!(
            classes(&findings),
            vec![("backlog/unpromoted", WARNING, vec![2])],
            "{findings:?}"
        );
    }

    #[test]
    fn a_link_whose_ref_no_row_carries_is_named_without_an_id() {
        let mut store = store(&[(1, &["B-m1ss1ng1"], &[])]);
        store.backlog_links[0].r#ref = "renamed-away".to_string();
        let findings = backlog_findings(&store, &backlog(BACKLOG), Some(FLOW));

        assert_eq!(
            classes(&findings),
            vec![("backlog/unknown-id", ERROR, vec![])],
            "{findings:?}"
        );
        assert!(
            findings[0].detail.contains("`renamed-away`"),
            "{findings:?}"
        );
    }

    #[test]
    fn load_backlog_reads_a_missing_store_as_empty_and_a_present_one_verbatim() {
        let (missing, present) = with_root(|root| {
            let missing = load_backlog().expect("a missing store reads");
            std::fs::write(root.join(".claude").join("backlog.toml"), BACKLOG)
                .expect("the store is written");
            (missing, load_backlog().expect("a present store reads"))
        });

        assert_eq!(missing, TomlValue::Table(toml::map::Map::new()));
        assert_eq!(items_array(&present, ARRAY_BACKLOG).len(), 6);
        assert_eq!(items_array(&present, ARRAY_COMPACTED).len(), 1);
    }
}
