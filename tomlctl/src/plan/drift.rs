//! `plan drift`: files changed since the last approval by commits the plan
//! does not own, reported against the tasks still to run. It warns and never
//! fails on drift.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::store;
use crate::cli::PlanDriftArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::print_json_line;
use crate::tasks::schema::{Status, Store};
use crate::tasks::{load_store, resolve_store_path};

/// Marks each commit in `git log --name-only` output, ahead of its files.
const COMMIT_MARK: &str = "\u{1}";

pub(super) fn run(args: PlanDriftArgs) -> Result<()> {
    let slug = &args.target.slug;
    let plan = store::load(&store::resolve(slug)?, &args.integrity)?;
    let Some(approval) = plan.approvals.last() else {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("plan `{slug}` has no approval to measure drift from"),
        ));
    };
    let tasks = load_store(&resolve_store_path(Some(slug), None)?, &args.integrity)?;
    let root = repo_or_cwd_root()?;

    let owned = recorded_commits(&tasks);
    let changed = changed_files(&root, &approval.commit, &owned)?;
    let affected = affected_tasks(&tasks, &changed);

    print_json_line(&json!({
        "ok": true,
        "approved_commit": approval.commit,
        "changed_files": changed,
        "affected": affected,
    }))
}

fn recorded_commits(tasks: &Store) -> Vec<String> {
    tasks
        .items
        .iter()
        .map(|row| row.commit.trim().to_string())
        .filter(|commit| !commit.is_empty())
        .collect()
}

/// A recorded commit may be abbreviated, so either side may be the prefix.
fn is_owned(owned: &[String], sha: &str) -> bool {
    owned
        .iter()
        .any(|recorded| sha.starts_with(recorded.as_str()) || recorded.starts_with(sha))
}

fn changed_files(root: &Path, since: &str, owned: &[String]) -> Result<BTreeSet<String>> {
    let range = format!("{since}..HEAD");
    let format = format!("--format={COMMIT_MARK}%H");
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--name-only", &format, "--end-of-options", &range])
        .output()
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("git log: {e}")))?;
    if !out.status.success() {
        return Err(tagged_err(
            ErrorKind::Other,
            None,
            format!(
                "git log {range} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(parse_log(&String::from_utf8_lossy(&out.stdout), owned))
}

fn parse_log(log: &str, owned: &[String]) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut counted = false;
    for line in log.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(sha) = line.strip_prefix(COMMIT_MARK) {
            counted = !is_owned(owned, sha);
        } else if counted {
            files.insert(line.to_string());
        }
    }
    files
}

fn affected_tasks(tasks: &Store, changed: &BTreeSet<String>) -> Vec<JsonValue> {
    tasks
        .items
        .iter()
        .filter(|row| matches!(row.status, Status::Pending | Status::InProgress))
        .filter_map(|row| {
            let hit: Vec<&String> = row.files.iter().filter(|f| changed.contains(*f)).collect();
            if hit.is_empty() {
                return None;
            }
            let premises: Vec<JsonValue> = tasks
                .specs
                .iter()
                .filter(|spec| spec.r#ref == row.r#ref)
                .flat_map(|spec| spec.premises.iter())
                .map(|p| json!({ "claim": p.claim, "check": p.check, "at": p.at }))
                .collect();
            Some(json!({
                "id": row.id,
                "ref": row.r#ref,
                "files": hit,
                "premises": premises,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_commits_contribute_no_files() {
        let log = "\u{1}aaa111\nsrc/a.rs\n\n\u{1}bbb222\nsrc/b.rs\n";
        let files = parse_log(log, &["aaa".to_string()]);
        assert_eq!(files.into_iter().collect::<Vec<_>>(), ["src/b.rs"]);
    }
}
