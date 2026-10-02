//! Discovery of the repo's flows and selection of the freshest one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use serde::Deserialize;
use tomlctl::LedgerKind;

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

/// Parses a `flow list` envelope, keeping only flows for which `mtime` yields a
/// `tasks.toml` modification time.
pub(crate) fn parse_flows(
    value: &serde_json::Value,
    mtime: impl Fn(&str) -> Option<SystemTime>,
) -> Result<Vec<FlowEntry>, String> {
    let env = Envelope::deserialize(value).map_err(|e| format!("bad `flow list` envelope: {e}"))?;
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

/// One flow-less ledger file, `.claude/<kind dir>/<scope>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeEntry {
    pub(crate) kind: LedgerKind,
    pub(crate) scope: String,
}

/// What the selector lists beyond [`FlowEntry`]: flows holding ledgers but no task store,
/// and the flow-less ledgers. Never consulted when choosing the freshest flow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scopes {
    pub(crate) ledger_only: Vec<String>,
    pub(crate) scopes: Vec<ScopeEntry>,
}

#[derive(Deserialize)]
struct RawScopes {
    #[serde(default)]
    flows: Vec<RawScopeFlow>,
    #[serde(default)]
    scopes: Vec<RawScope>,
}

#[derive(Deserialize)]
struct RawScopeFlow {
    slug: String,
    has_tasks: bool,
}

#[derive(Deserialize)]
struct RawScope {
    kind: String,
    scope: String,
}

impl Scopes {
    /// Parses a `tomlctl::ledger_scopes` document. A scope of an unknown kind is an error,
    /// since it could be neither shown nor read.
    pub(crate) fn from_value(value: &serde_json::Value) -> Result<Scopes, String> {
        let raw = RawScopes::deserialize(value)
            .map_err(|e| format!("bad ledger scopes document: {e}"))?;
        let mut ledger_only: Vec<String> = raw
            .flows
            .into_iter()
            .filter(|flow| !flow.has_tasks)
            .map(|flow| flow.slug)
            .collect();
        ledger_only.sort();
        let mut scopes = raw
            .scopes
            .into_iter()
            .map(|raw| {
                let kind = LedgerKind::ALL
                    .into_iter()
                    .find(|kind| kind.as_str() == raw.kind)
                    .ok_or_else(|| format!("unknown ledger kind `{}`", raw.kind))?;
                Ok(ScopeEntry {
                    kind,
                    scope: raw.scope,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        scopes.sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.scope.cmp(&b.scope)));
        Ok(Scopes {
            ledger_only,
            scopes,
        })
    }
}

/// `<root>/.claude/flows`, the directory holding one subdirectory per flow.
pub(crate) fn flows_root(root: &Path) -> PathBuf {
    root.join(".claude").join("flows")
}

/// Lists the repo's flows named in `task_stores`, the `tasks.toml` mtime of every flow that
/// has one, freshest first. No other flow's `context.toml` is read.
pub(crate) fn list(
    root: &Path,
    task_stores: &BTreeMap<String, SystemTime>,
) -> Result<Vec<FlowEntry>, String> {
    let value = tomlctl::flow_list_matching(root, |slug| task_stores.contains_key(slug))
        .map_err(|e| format!("{e:#}"))?;
    let mut entries = parse_flows(&value, |slug| task_stores.get(slug).copied())?;
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
        .find(|dir| flows_root(dir).is_dir())
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

    fn sample() -> serde_json::Value {
        serde_json::from_str(SAMPLE).expect("sample is JSON")
    }

    #[test]
    fn parse_keeps_only_flows_with_a_task_store() {
        let got = parse_flows(&sample(), |slug| (slug == "b").then(|| at(5))).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slug, "b");
        assert_eq!(got[0].status, "in-progress");
        assert_eq!(got[0].plan_path, "docs/plans/b.md");
        assert_eq!(got[0].tasks_mtime, at(5));
    }

    #[test]
    fn parse_rejects_a_malformed_envelope() {
        let bad = serde_json::json!({"flows": [{"status": "review"}]});
        assert!(parse_flows(&bad, |_| Some(at(1))).is_err());
        assert!(parse_flows(&serde_json::json!("not an envelope"), |_| Some(at(1))).is_err());
    }

    #[test]
    fn rank_orders_newest_first_with_slug_tiebreak() {
        let mut v = vec![entry("old", 1), entry("z", 9), entry("a", 9)];
        rank(&mut v);
        let slugs: Vec<_> = v.iter().map(|e| e.slug.as_str()).collect();
        assert_eq!(slugs, ["a", "z", "old"]);
    }

    #[test]
    fn flows_without_a_task_store_are_listed() {
        let value = serde_json::json!({
            "flows": [
                {"slug": "tracked", "has_tasks": true, "ledgers": ["review"]},
                {"slug": "zeta", "has_tasks": false, "ledgers": ["optimise"]},
                {"slug": "alpha", "has_tasks": false, "ledgers": ["review", "plan-review"]}
            ],
            "scopes": [
                {"kind": "plan-review", "scope": "p"},
                {"kind": "review", "scope": "b"},
                {"kind": "optimise", "scope": "a"},
                {"kind": "review", "scope": "a"}
            ]
        });
        let got = Scopes::from_value(&value).unwrap();
        assert_eq!(got.ledger_only, ["alpha", "zeta"]);
        let scopes: Vec<_> = got
            .scopes
            .iter()
            .map(|s| (s.kind.as_str(), s.scope.as_str()))
            .collect();
        assert_eq!(
            scopes,
            [
                ("review", "a"),
                ("review", "b"),
                ("optimise", "a"),
                ("plan-review", "p")
            ]
        );

        let bad = serde_json::json!({"scopes": [{"kind": "backlog", "scope": "x"}]});
        assert!(Scopes::from_value(&bad).is_err());
        assert_eq!(
            Scopes::from_value(&serde_json::json!({})).unwrap(),
            Scopes::default()
        );
    }

    #[test]
    fn freshest_agrees_with_rank_head() {
        let v = vec![entry("old", 1), entry("z", 9), entry("a", 9)];
        assert_eq!(freshest(&v).map(|e| e.slug.as_str()), Some("a"));
        assert!(freshest(&[]).is_none());
    }
}
