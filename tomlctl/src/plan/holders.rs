//! Which other row of a milestone could edit a path at the same time as a
//! requesting task, and which paths no task may ever be granted.
//!
//! The milestone Workflow never starts a task beside one in its `needs`
//! closure, up or down, and has released every `done` row's files, so only the
//! remaining rows of the requester's milestone can collide with it. A path is
//! compared by its folded key, which also catches a letter-case variant that a
//! case-insensitive file system would resolve to the same file.

use std::collections::BTreeSet;

use anyhow::Result;

use super::schema::PlanStore;
use super::schema_request::Ruling;
use crate::errors::{ErrorKind, tagged_err};
use crate::tasks::schema::{Status, Store, TaskRow, normal_path};

/// The one key paths are compared by: slashes normalised, a leading `./`
/// dropped, letter case folded. Stricter than the milestone Workflow's
/// `fileKey`, which keeps a leading `./`.
pub(crate) fn fold_key(path: &str) -> String {
    normal_path(path).to_lowercase()
}

/// Why `path` can never be granted to a task of flow `slug`, or `None` when
/// it can be: an absolute path, a `..` segment, or a path under `.claude/` or
/// the flow's own `docs/flows/<slug>/`, whose files only the orchestrator
/// writes.
pub(crate) fn forbidden(slug: &str, path: &str) -> Option<String> {
    let key = fold_key(path);
    let bytes = key.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if key.starts_with('/') || drive {
        return Some(format!(
            "`{path}` is absolute; a request names a repo-relative path"
        ));
    }
    if key.split('/').any(|segment| segment == "..") {
        return Some(format!("`{path}` has a `..` segment"));
    }
    if under(&key, ".claude") {
        return Some(format!(
            "`{path}` is under `.claude/`, which only tomlctl writes"
        ));
    }
    let flow_dir = fold_key(&format!("docs/flows/{slug}"));
    if under(&key, &flow_dir) {
        return Some(format!(
            "`{path}` is under `docs/flows/{slug}/`, which only the orchestrator writes"
        ));
    }
    None
}

/// Whether the folded `key` is the folded directory `dir` or lies beneath it.
fn under(key: &str, dir: &str) -> bool {
    key.strip_prefix(dir)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// A row that could edit a requested path alongside the requester.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holder {
    pub(crate) task: u32,
    /// The holding row's status as stored.
    pub(crate) status: String,
    /// The id of the unapplied request that granted the row the path, or
    /// `None` when the path is in the row's own `files`.
    pub(crate) grant: Option<String>,
}

/// The lowest-id `pending`, `in-progress`, `failed` or `parked` row of
/// `requester`'s milestone, outside its `needs` closure both ways, whose files
/// or still-unapplied granted path items hold `path` by folded key; `None`
/// when no row does.
pub(crate) fn holder(
    tasks: &Store,
    plan: &PlanStore,
    requester: u32,
    path: &str,
) -> Result<Option<Holder>> {
    let Some(own) = tasks.items.iter().find(|row| row.id == requester) else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("no task {requester} in the task store"),
        ));
    };
    let key = fold_key(path);
    let related = closure(&tasks.items, requester);

    let mut candidates: Vec<&TaskRow> = tasks
        .items
        .iter()
        .filter(|row| row.checkpoint == own.checkpoint)
        .filter(|row| !related.contains(&row.id))
        .filter(|row| {
            matches!(
                row.status,
                Status::Pending | Status::InProgress | Status::Failed | Status::Parked
            )
        })
        .collect();
    candidates.sort_by_key(|row| row.id);

    for row in candidates {
        if row.files.iter().any(|file| fold_key(file) == key) {
            return Ok(Some(Holder {
                task: row.id,
                status: row.status.as_str().to_string(),
                grant: None,
            }));
        }
        let granted = plan
            .requests
            .iter()
            .filter(|request| request.task == row.id && !request.applied)
            .find(|request| {
                request
                    .paths
                    .iter()
                    .any(|item| item.ruling == Ruling::Granted && fold_key(&item.path) == key)
            });
        if let Some(request) = granted {
            return Ok(Some(Holder {
                task: row.id,
                status: row.status.as_str().to_string(),
                grant: Some(request.id.clone()),
            }));
        }
    }
    Ok(None)
}

/// `id` with every task it transitively depends on and every task that
/// transitively depends on it, over the `needs` and `coupling` edges the task
/// graph's `closure_up` and `closure_down` walk. A dangling edge is skipped and
/// a cycle ends the walk; `tasks check` reports both.
fn closure(rows: &[TaskRow], id: u32) -> BTreeSet<u32> {
    let edges = |row: &TaskRow| -> Vec<u32> {
        row.needs
            .iter()
            .chain(row.coupling.iter())
            .copied()
            .collect()
    };
    let mut related = BTreeSet::from([id]);

    let mut seen = BTreeSet::from([id]);
    let mut queue = vec![id];
    while let Some(current) = queue.pop() {
        if let Some(row) = rows.iter().find(|row| row.id == current) {
            for dep in edges(row) {
                if seen.insert(dep) {
                    queue.push(dep);
                }
            }
        }
    }
    related.extend(seen);

    let mut seen = BTreeSet::from([id]);
    let mut queue = vec![id];
    while let Some(current) = queue.pop() {
        for row in rows {
            if edges(row).contains(&current) && seen.insert(row.id) {
                queue.push(row.id);
            }
        }
    }
    related.extend(seen);
    related
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::schema_request::{PathItem, Request, RequestStatus, RuledBy};
    use crate::tasks::schema::Effort;
    use toml::value::Datetime;

    fn row(id: u32, status: Status, files: &[&str], needs: &[u32]) -> TaskRow {
        TaskRow {
            id,
            r#ref: format!("task-{id}"),
            title: format!("Task {id}"),
            effort: Effort::S,
            status,
            checkpoint: "A".to_string(),
            phase: String::new(),
            phase_depth: 0,
            heading_depth: 4,
            files: files.iter().map(|f| (*f).to_string()).collect(),
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

    fn tasks(items: Vec<TaskRow>) -> Store {
        Store {
            items,
            ..Store::default()
        }
    }

    fn grant(id: &str, task: u32, path: &str, ruling: Ruling, applied: bool) -> Request {
        let at: Datetime = "2026-11-18T10:00:00Z".parse().expect("a datetime");
        Request {
            id: id.to_string(),
            run_id: "wf_test".to_string(),
            milestone: "A".to_string(),
            task,
            at,
            status: RequestStatus::Ruled,
            ruled_at: Some(at),
            applied,
            paths: vec![PathItem {
                path: path.to_string(),
                why: "It needs it.".to_string(),
                ruling,
                by: RuledBy::Tomlctl,
                reason: String::new(),
            }],
            changes: Vec::new(),
        }
    }

    fn plan(requests: Vec<Request>) -> PlanStore {
        PlanStore {
            requests,
            ..PlanStore::default()
        }
    }

    #[test]
    fn fold_key_normalises_slashes_drops_a_leading_dot_slash_and_folds_case() {
        assert_eq!(fold_key(".\\Src\\Foo.TS"), "src/foo.ts");
        assert_eq!(fold_key("./src/foo.ts"), fold_key("src/FOO.ts"));
    }

    #[test]
    fn a_pending_unrelated_row_holds_a_case_variant_of_its_file() {
        let store = tasks(vec![
            row(1, Status::InProgress, &["src/a.ts"], &[]),
            row(2, Status::Pending, &["src/Foo.ts"], &[]),
        ]);
        let found = holder(&store, &plan(Vec::new()), 1, "src/foo.ts").expect("resolves");
        assert_eq!(
            found,
            Some(Holder {
                task: 2,
                status: "pending".to_string(),
                grant: None,
            })
        );
    }

    #[test]
    fn a_failed_or_parked_unrelated_row_holds_and_the_lowest_id_wins() {
        let store = tasks(vec![
            row(1, Status::InProgress, &["src/a.ts"], &[]),
            row(4, Status::Parked, &["src/x.ts"], &[]),
            row(3, Status::Failed, &["./src/x.ts"], &[]),
        ]);
        let found = holder(&store, &plan(Vec::new()), 1, "src/x.ts")
            .expect("resolves")
            .expect("held");
        assert_eq!((found.task, found.status.as_str()), (3, "failed"));
    }

    #[test]
    fn a_dependent_or_a_dependency_of_the_requester_never_holds() {
        let store = tasks(vec![
            row(1, Status::Pending, &["src/x.ts"], &[]),
            row(2, Status::InProgress, &["src/a.ts"], &[1]),
            row(3, Status::Pending, &["src/X.ts"], &[2]),
        ]);
        let none = plan(Vec::new());
        assert_eq!(
            holder(&store, &none, 2, "src/x.ts").expect("resolves"),
            None
        );
        assert_eq!(
            holder(&store, &none, 3, "src/x.ts").expect("resolves"),
            None
        );
    }

    #[test]
    fn a_done_or_deferred_row_never_holds() {
        let store = tasks(vec![
            row(1, Status::InProgress, &["src/a.ts"], &[]),
            row(2, Status::Done, &["src/x.ts"], &[]),
            row(3, Status::Deferred, &["src/x.ts"], &[]),
        ]);
        assert_eq!(
            holder(&store, &plan(Vec::new()), 1, "src/x.ts").expect("resolves"),
            None
        );
    }

    #[test]
    fn a_row_of_another_milestone_never_holds() {
        let mut other = row(2, Status::Pending, &["src/x.ts"], &[]);
        other.checkpoint = "B".to_string();
        let store = tasks(vec![row(1, Status::InProgress, &["src/a.ts"], &[]), other]);
        assert_eq!(
            holder(&store, &plan(Vec::new()), 1, "src/x.ts").expect("resolves"),
            None
        );
    }

    #[test]
    fn an_earlier_unapplied_grant_to_an_unrelated_pending_row_holds() {
        let store = tasks(vec![
            row(1, Status::InProgress, &["src/a.ts"], &[]),
            row(2, Status::Pending, &["src/b.ts"], &[]),
        ]);
        let found = holder(
            &store,
            &plan(vec![grant(
                "Q1",
                2,
                "SRC/Shared.ts",
                Ruling::Granted,
                false,
            )]),
            1,
            "src/shared.ts",
        )
        .expect("resolves");
        assert_eq!(
            found,
            Some(Holder {
                task: 2,
                status: "pending".to_string(),
                grant: Some("Q1".to_string()),
            })
        );
    }

    #[test]
    fn an_applied_denied_or_open_grant_does_not_hold() {
        let store = tasks(vec![
            row(1, Status::InProgress, &["src/a.ts"], &[]),
            row(2, Status::Pending, &["src/b.ts"], &[]),
        ]);
        for request in [
            grant("Q1", 2, "src/shared.ts", Ruling::Granted, true),
            grant("Q1", 2, "src/shared.ts", Ruling::Denied, false),
            grant("Q1", 2, "src/shared.ts", Ruling::Open, false),
        ] {
            assert_eq!(
                holder(&store, &plan(vec![request]), 1, "src/shared.ts").expect("resolves"),
                None
            );
        }
    }

    #[test]
    fn an_unknown_requester_is_refused() {
        let store = tasks(vec![row(1, Status::Pending, &["src/a.ts"], &[])]);
        let message = holder(&store, &plan(Vec::new()), 9, "src/a.ts")
            .expect_err("no task 9")
            .to_string();
        assert!(message.contains("task 9"), "{message}");
    }

    #[test]
    fn paths_under_claude_or_the_flow_directory_are_forbidden() {
        for path in [
            ".claude/flows/demo/tasks.toml",
            "./.Claude/backlog.toml",
            "docs/flows/demo/notes/run/task-1.md",
            "Docs\\Flows\\Demo\\brief.md",
        ] {
            assert!(forbidden("demo", path).is_some(), "{path}");
        }
        assert!(forbidden("demo", "docs/flows/other/brief.md").is_none());
        assert!(forbidden("demo", "docs/flows/demo-two/brief.md").is_none());
        assert!(forbidden("demo", ".claudette/x.md").is_none());
        assert!(forbidden("demo", "src/foo.ts").is_none());
    }

    #[test]
    fn absolute_paths_and_parent_segments_are_forbidden() {
        for path in [
            "/etc/passwd",
            "C:\\repo\\a.rs",
            "c:/repo/a.rs",
            "src/../../x.rs",
            "..",
        ] {
            assert!(forbidden("demo", path).is_some(), "{path}");
        }
        assert!(forbidden("demo", "src/..hidden/x.rs").is_none());
    }
}
