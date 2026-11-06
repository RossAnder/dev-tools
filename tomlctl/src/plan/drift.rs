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
use crate::tasks::schema::{Status, Store, normal_path};
use crate::tasks::{load_store, resolve_store_path};

/// Marks each commit in `git log --name-only -z` output, ahead of its files.
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

    refuse_unresolved(&root, &approval.commit)?;
    let owned = recorded_commits(&tasks);
    let changed = changed_files(&root, &approval.commit, &owned, slug)?;
    let affected = affected_tasks(&tasks, &changed);

    print_json_line(&json!({
        "ok": true,
        "approved_commit": approval.commit,
        "changed_files": changed,
        "affected": affected,
    }))
}

/// A rebase, a gc or a shallow clone can take the approved commit away, and
/// `git log`'s usage text would not say that is what happened.
fn refuse_unresolved(root: &Path, commit: &str) -> Result<()> {
    let object = format!("{commit}^{{commit}}");
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["cat-file", "-e", "--end-of-options", &object])
        .output()
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("git cat-file: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    Err(tagged_err(
        ErrorKind::Validation,
        None,
        format!(
            "the approved commit `{commit}` does not resolve in this repository, so there is \
             nothing to measure drift from: `plan approve --amend` stamps a new approval at HEAD"
        ),
    ))
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

/// `--no-renames` lists a rename's source too, which is the path a row names, and
/// `-z` hands over every path verbatim, where the line form C-quotes a tab, a
/// newline, a quote or a backslash whatever `core.quotePath` says.
fn changed_files(
    root: &Path,
    since: &str,
    owned: &[String],
    slug: &str,
) -> Result<BTreeSet<String>> {
    let range = format!("{since}..HEAD");
    let format = format!("--format={COMMIT_MARK}%H");
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            "--no-renames",
            "--name-only",
            "-z",
            &format,
            "--end-of-options",
            &range,
        ])
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
    let flow_dir = format!(".claude/flows/{slug}/");
    Ok(parse_log(
        &String::from_utf8_lossy(&out.stdout),
        owned,
        &flow_dir,
    ))
}

/// Each token is NUL-terminated: `<mark><sha>`, then a commit's paths, the first
/// of them after one newline git puts there. That newline alone is stripped,
/// since any other edge byte belongs to the path. A mark must carry a hex sha, so
/// a path that merely opens with the mark byte stays a path. Paths under
/// `flow_dir` are the flow's own stores, which are never drift.
fn parse_log(log: &str, owned: &[String], flow_dir: &str) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    let mut counted = false;
    let mut first_path = false;
    for token in log.split('\0') {
        let sha = token
            .strip_prefix(COMMIT_MARK)
            .filter(|sha| !sha.is_empty() && sha.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if let Some(sha) = sha {
            counted = !is_owned(owned, sha);
            first_path = true;
            continue;
        }
        let path = if first_path {
            token.strip_prefix('\n').unwrap_or(token)
        } else {
            token
        };
        first_path = false;
        if counted && !path.is_empty() && !path.starts_with(flow_dir) {
            files.insert(path.to_string());
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
            let hit: Vec<&String> = row
                .files
                .iter()
                .filter(|file| {
                    changed.contains(file.as_str()) || changed.contains(&normal_path(file))
                })
                .collect();
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

    const FLOW_DIR: &str = ".claude/flows/s/";

    #[test]
    fn owned_commits_contribute_no_files() {
        let log = "\u{1}aaa111\0\nsrc/a.rs\0\u{1}bbb222\0\nsrc/b.rs\0";
        let files = parse_log(log, &["aaa".to_string()], FLOW_DIR);
        assert_eq!(files.into_iter().collect::<Vec<_>>(), ["src/b.rs"]);
    }

    #[test]
    fn the_flows_own_stores_are_not_changed_files() {
        let log = "\u{1}aaa111\0\n.claude/flows/s/tasks.toml\0.claude/flows/s2/x.toml\0src/a.rs\0";
        let files = parse_log(log, &[], FLOW_DIR);
        assert_eq!(
            files.into_iter().collect::<Vec<_>>(),
            [".claude/flows/s2/x.toml", "src/a.rs"]
        );
    }

    #[test]
    fn a_commit_with_no_files_leaves_the_next_commit_readable() {
        let log = "\u{1}aaa111\0\u{1}bbb222\0\nsrc/b.rs\0";
        let files = parse_log(log, &[], FLOW_DIR);
        assert_eq!(files.into_iter().collect::<Vec<_>>(), ["src/b.rs"]);
    }

    #[test]
    fn only_the_newline_git_adds_is_stripped_from_a_path() {
        let log = "\u{1}aaa111\0\n\nlead.rs\0 edge \0nl\nname\0\u{1}odd name\0";
        let files = parse_log(log, &[], FLOW_DIR);
        assert_eq!(
            files.into_iter().collect::<Vec<_>>(),
            ["\u{1}odd name", "\nlead.rs", " edge ", "nl\nname"]
        );
    }
}
