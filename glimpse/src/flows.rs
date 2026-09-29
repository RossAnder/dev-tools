//! Discovery of the repo's flows and selection of the freshest one.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::Deserialize;

/// One flow that has a task store on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FlowEntry {
    pub(crate) slug: String,
    pub(crate) status: String,
    pub(crate) updated: String,
    pub(crate) plan_path: String,
    /// Modification time of the flow's `tasks.toml`; the freshness key.
    pub(crate) tasks_mtime: SystemTime,
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    flows: Vec<RawFlow>,
}

#[derive(Deserialize)]
struct RawFlow {
    slug: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    updated: String,
    #[serde(default)]
    plan_path: String,
}

/// Parses `tomlctl flow list` output, keeping only flows for which `mtime`
/// yields a `tasks.toml` modification time.
pub(crate) fn parse_flows(
    json: &str,
    mtime: impl Fn(&str) -> Option<SystemTime>,
) -> Result<Vec<FlowEntry>, String> {
    let env: Envelope =
        serde_json::from_str(json).map_err(|e| format!("bad `flow list` output: {e}"))?;
    Ok(env
        .flows
        .into_iter()
        .filter_map(|f| {
            let tasks_mtime = mtime(&f.slug)?;
            Some(FlowEntry {
                slug: f.slug,
                status: f.status,
                updated: f.updated,
                plan_path: f.plan_path,
                tasks_mtime,
            })
        })
        .collect())
}

/// Sorts newest `tasks.toml` first; equal times fall back to slug order so the
/// result is deterministic.
pub(crate) fn rank(entries: &mut [FlowEntry]) {
    entries.sort_by(|a, b| {
        b.tasks_mtime
            .cmp(&a.tasks_mtime)
            .then_with(|| a.slug.cmp(&b.slug))
    });
}

/// The flow whose task store changed most recently.
pub(crate) fn freshest(entries: &[FlowEntry]) -> Option<&FlowEntry> {
    entries.iter().max_by(|a, b| {
        a.tasks_mtime
            .cmp(&b.tasks_mtime)
            .then_with(|| b.slug.cmp(&a.slug))
    })
}

/// Lists the repo's flows that have a `tasks.toml`, freshest first.
pub(crate) fn list(root: &Path, tomlctl: &str) -> Result<Vec<FlowEntry>, String> {
    let out = Command::new(tomlctl)
        .args(["flow", "list"])
        .current_dir(root)
        .env("TOMLCTL_ROOT", root)
        .output()
        .map_err(|e| format!("cannot run `{tomlctl} flow list`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`{tomlctl} flow list` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json = String::from_utf8_lossy(&out.stdout);
    let mut entries = parse_flows(&json, |slug| {
        std::fs::metadata(root.join(".claude/flows").join(slug).join("tasks.toml"))
            .and_then(|m| m.modified())
            .ok()
    })?;
    rank(&mut entries);
    Ok(entries)
}

/// The repository root for `cwd`: `git rev-parse --show-toplevel`, else the
/// nearest ancestor holding `.claude/flows`.
pub(crate) fn repo_root(cwd: &Path) -> Option<PathBuf> {
    let git = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(top) = git {
        return Some(PathBuf::from(top));
    }
    cwd.ancestors()
        .find(|dir| dir.join(".claude").join("flows").is_dir())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn entry(slug: &str, secs: u64) -> FlowEntry {
        FlowEntry {
            slug: slug.to_string(),
            status: "in-progress".to_string(),
            updated: "2026-09-29".to_string(),
            plan_path: format!("docs/plans/{slug}.md"),
            tasks_mtime: at(secs),
        }
    }

    const SAMPLE: &str = r#"{"ok":true,"flows":[
        {"slug":"a","status":"review","updated":"2026-06-09","plan_path":"docs/plans/a.md","branch":"main","scope":["x/**"]},
        {"slug":"b","status":"in-progress","updated":"2026-09-29","plan_path":"docs/plans/b.md","scope":[]}
    ],"skipped":[]}"#;

    #[test]
    fn parse_keeps_only_flows_with_a_task_store() {
        let got = parse_flows(SAMPLE, |slug| (slug == "b").then(|| at(5))).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slug, "b");
        assert_eq!(got[0].status, "in-progress");
        assert_eq!(got[0].plan_path, "docs/plans/b.md");
        assert_eq!(got[0].tasks_mtime, at(5));
    }

    #[test]
    fn parse_rejects_malformed_json() {
        assert!(parse_flows("not json", |_| Some(at(1))).is_err());
    }

    #[test]
    fn rank_orders_newest_first_with_slug_tiebreak() {
        let mut v = vec![entry("old", 1), entry("z", 9), entry("a", 9)];
        rank(&mut v);
        let slugs: Vec<_> = v.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, ["a", "z", "old"]);
    }

    #[test]
    fn freshest_agrees_with_rank_head() {
        let v = vec![entry("old", 1), entry("z", 9), entry("a", 9)];
        assert_eq!(freshest(&v).map(|e| e.slug.as_str()), Some("a"));
        assert!(freshest(&[]).is_none());
    }
}
