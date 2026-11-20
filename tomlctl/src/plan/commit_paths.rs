//! `plan emit commit-paths`: the paths one of the drain's two commits stages,
//! from a `git status` read of its own that skips no directory. `--for work`
//! takes the changed files a milestone's rows name, wherever they live; `--for
//! flow` takes the flow's stores, its run notes, the backlog and inputs stores
//! and the review stores the milestone's finding links name, less the work
//! list. Both list the paths the ignore rules hide, which a plain status read
//! never shows, so they are not silently left out of a commit.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Result;
use serde_json::{Value as JsonValue, json};

use super::drift::{git_status, status_entries};
use super::emit::load_milestone_store;
use crate::cli::PlanEmitCommitPathsArgs;
use crate::errors::{ErrorKind, tagged_err};
use crate::io::repo_or_cwd_root;
use crate::output::print_json_line;
use crate::tasks::schema::{Store, normal_path};

const BACKLOG_STORE: &str = ".claude/backlog.toml";
const INPUTS_STORE: &str = ".claude/inputs.toml";
const SIDECAR_SUFFIX: &str = ".sha256";
const IGNORED_CODE: &str = "!!";

pub(super) fn run(args: PlanEmitCommitPathsArgs) -> Result<()> {
    let PlanEmitCommitPathsArgs {
        target,
        milestone,
        for_commit,
        integrity,
    } = args;
    let slug = target.slug.as_str();
    if !matches!(for_commit.as_str(), "work" | "flow") {
        return Err(tagged_err(
            ErrorKind::Validation,
            None,
            format!("`--for {for_commit}` is not one of work, flow"),
        ));
    }
    let tasks = load_milestone_store(slug, &milestone, integrity.verify_integrity)?;
    let root = repo_or_cwd_root()?;
    let changed: BTreeSet<String> = status_entries(&git_status(&root, &[], &[])?)
        .map(|(_, path)| path.to_string())
        .collect();
    let work = Work::of(&tasks, &milestone, &changed);

    let report = if for_commit == "work" {
        let hidden = ignore_rules(&root, &work.unmatched)?;
        let hidden_paths: BTreeSet<&str> = hidden.iter().map(|(path, _)| path.as_str()).collect();
        let unmatched: Vec<&String> = work
            .unmatched
            .iter()
            .filter(|file| !hidden_paths.contains(file.as_str()))
            .collect();
        json!({
            "paths": work.paths,
            "ignored": ignored_json(&hidden),
            "unmatched": unmatched,
        })
    } else {
        let scope = FlowScope::of(&tasks, slug, &milestone);
        let paths: Vec<&String> = changed
            .iter()
            .filter(|path| scope.holds(path) && !work.paths.contains(path))
            .collect();
        let status = git_status(&root, &["--ignored=matching"], &scope.pathspecs())?;
        let hidden_paths: Vec<String> = status_entries(&status)
            .filter(|(code, _)| *code == IGNORED_CODE)
            .map(|(_, path)| path.to_string())
            .collect();
        let hidden = ignore_rules(&root, &hidden_paths)?;
        json!({ "paths": paths, "ignored": ignored_json(&hidden) })
    };
    print_json_line(&report)
}

/// The changed paths, in git's spelling, that a row of the milestone names in
/// its `files`, and the row files, normalised, that match none of them.
struct Work {
    paths: Vec<String>,
    unmatched: Vec<String>,
}

impl Work {
    fn of(tasks: &Store, milestone: &str, changed: &BTreeSet<String>) -> Self {
        let files: BTreeSet<String> = tasks
            .items
            .iter()
            .filter(|row| row.checkpoint == milestone)
            .flat_map(|row| row.files.iter().map(|file| normal_path(file)))
            .filter(|file| !file.is_empty())
            .collect();
        let paths: Vec<String> = changed
            .iter()
            .filter(|path| files.contains(&normal_path(path)))
            .cloned()
            .collect();
        let hit: BTreeSet<String> = paths.iter().map(|path| normal_path(path)).collect();
        let unmatched = files
            .into_iter()
            .filter(|file| !hit.contains(file))
            .collect();
        Self { paths, unmatched }
    }
}

/// What the flow-store commit holds: the flow's store directory and run notes,
/// and the backlog, inputs and linked review stores with their sidecars.
struct FlowScope {
    dirs: [String; 2],
    files: BTreeSet<String>,
}

impl FlowScope {
    /// A linked review store inside the flow's store directory is already held
    /// by it, so only one outside it is named on its own.
    fn of(tasks: &Store, slug: &str, milestone: &str) -> Self {
        let dirs = [
            format!(".claude/flows/{slug}/"),
            format!("docs/flows/{slug}/notes/run/"),
        ];
        let refs: BTreeSet<&str> = tasks
            .items
            .iter()
            .filter(|row| row.checkpoint == milestone)
            .map(|row| row.r#ref.as_str())
            .collect();
        let review_stores = tasks
            .finding_links
            .iter()
            .filter(|link| refs.contains(link.r#ref.as_str()))
            .map(|link| normal_path(&link.store))
            .filter(|store| !store.is_empty() && !store.starts_with(dirs[0].as_str()));
        let files = [BACKLOG_STORE.to_string(), INPUTS_STORE.to_string()]
            .into_iter()
            .chain(review_stores)
            .flat_map(|store| [format!("{store}{SIDECAR_SUFFIX}"), store])
            .collect();
        Self { dirs, files }
    }

    fn holds(&self, path: &str) -> bool {
        let path = normal_path(path);
        self.files.contains(&path) || self.dirs.iter().any(|dir| path.starts_with(dir.as_str()))
    }

    /// Literal, so a path holding a glob character names only itself.
    fn pathspecs(&self) -> Vec<String> {
        self.dirs
            .iter()
            .map(|dir| dir.trim_end_matches('/'))
            .chain(self.files.iter().map(String::as_str))
            .map(|path| format!(":(literal){path}"))
            .collect()
    }
}

fn ignored_json(hidden: &[(String, String)]) -> JsonValue {
    hidden
        .iter()
        .map(|(path, rule)| json!({ "path": path, "rule": rule }))
        .collect()
}

/// Each of `paths` an ignore rule hides, with the rule as `git check-ignore -v`
/// prints it, `<source>:<line>:<pattern>`. A tracked path is never hidden, and
/// a path matched only by a negated (`!`) rule is not ignored.
fn ignore_rules(root: &Path, paths: &[String]) -> Result<Vec<(String, String)>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "-v", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("git check-ignore: {e}")))?;
    let input: Vec<u8> = paths
        .iter()
        .flat_map(|path| path.bytes().chain([0]))
        .collect();
    let mut stdin = child.stdin.take().expect("stdin is piped");
    // Written from its own thread so a full stdout pipe cannot stall the write.
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child
        .wait_with_output()
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("git check-ignore: {e}")))?;
    writer
        .join()
        .map_err(|_| {
            tagged_err(
                ErrorKind::Other,
                None,
                "git check-ignore: the stdin writer panicked".to_string(),
            )
        })?
        .map_err(|e| tagged_err(ErrorKind::Other, None, format!("git check-ignore: {e}")))?;
    // Exit 1 means no path is ignored.
    if !matches!(out.status.code(), Some(0 | 1)) {
        return Err(tagged_err(
            ErrorKind::Other,
            None,
            format!(
                "git check-ignore failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ),
        ));
    }
    Ok(parse_check_ignore(&String::from_utf8_lossy(&out.stdout)))
}

/// `-v -z` output is four NUL-terminated fields per path: source, line number,
/// pattern, path.
fn parse_check_ignore(output: &str) -> Vec<(String, String)> {
    let fields: Vec<&str> = output.split('\0').collect();
    fields
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|[_, _, pattern, path]| !pattern.starts_with('!') && !path.is_empty())
        .map(|[source, line, pattern, path]| {
            (path.to_string(), format!("{source}:{line}:{pattern}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_ignore_records_drop_negated_rules() {
        let output = ".gitignore\x003\x00*.log\x00a.log\x00\
                      .gitignore\x004\x00!keep.log\x00keep.log\x00";
        assert_eq!(
            parse_check_ignore(output),
            [("a.log".to_string(), ".gitignore:3:*.log".to_string())]
        );
    }
}
