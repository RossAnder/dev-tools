//! `review reconcile` against a sandbox seeded by hand: a v2 flow whose task
//! store carries `[[finding_links]]`, and a flow-less review store whose
//! findings are `scheduled` into that flow.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{TASKS_SLUG, cli, sandbox, stage_tasks_flow, stdout_json};
use serde_json::Value as JsonValue;
use toml::Value;

const SCOPE: &str = "t";
const REVIEW_STORE: &str = ".claude/review-v2/t.toml";

const HEADER: &str = r#"schema_version = 2

[[runs]]
id = "U1"
command = "review-v2"
family = "code"
depth = "standard"
started = "2026-10-01T10:00:00Z"

[[cells]]
id = "C1"
run = "U1"
wave = 1
slice = ["src/**"]
concerns = ["correctness"]
agent = "review-finder"
reason = "proposed"
status = "covered"
sealed = true
note = "done"
"#;

/// A finding scheduled into `flow` on the task `task`.
fn scheduled(id: &str, flow: &str, task: &str) -> String {
    format!(
        r#"
[[findings]]
id = "{id}"
run = "U1"
cell = "C1"
seen_in = ["U1"]
family = "code"
kind = "defect"
lens = "correctness"
route = "verifier"
file = "src/a.rs"
line = 1
symbol = "alpha"
summary = "alpha is off by one"
severity = "major"
failure_scenario = "a two-row input returns one row"
fix = "count from zero"
effort = "small"
status = "scheduled"
apply_flow = "{flow}"
apply_task = "{task}"
"#
    )
}

struct Task<'a> {
    r#ref: &'a str,
    status: &'a str,
    commit: &'a str,
}

fn task<'a>(r#ref: &'a str, status: &'a str, commit: &'a str) -> Task<'a> {
    Task {
        r#ref,
        status,
        commit,
    }
}

struct Link<'a> {
    r#ref: &'a str,
    fixes: &'a [&'a str],
    settles: &'a [&'a str],
}

fn fixes<'a>(r#ref: &'a str, ids: &'a [&'a str]) -> Link<'a> {
    Link {
        r#ref,
        fixes: ids,
        settles: &[],
    }
}

fn quoted(ids: &[&str]) -> String {
    let ids: Vec<String> = ids.iter().map(|id| format!("{id:?}")).collect();
    format!("[{}]", ids.join(", "))
}

fn tasks_store(tasks: &[Task<'_>], links: &[Link<'_>]) -> String {
    let mut out = String::from(
        "schema_version = 2\nlast_updated = 2026-09-07\n\n[policy]\ncheckpoints = \"milestones\"\nmax_parallel = 6\ncommit_granularity = \"per-task\"\nnote = \"\"\n\n[[checkpoints]]\nid = \"A\"\nrationale = \"the fixes\"\n",
    );
    for (at, t) in tasks.iter().enumerate() {
        out.push_str(&format!(
            r#"
[[items]]
id = {id}
ref = "{r}"
title = "{r}"
effort = "S"
status = "{status}"
checkpoint = "A"
files = ["src/a.rs"]
needs = []
coupling = []
deps_note = ""
action = ""
detail = ""
acceptance = ""
agent = ""
commit = "{commit}"
"#,
            id = at + 1,
            r = t.r#ref,
            status = t.status,
            commit = t.commit,
        ));
    }
    for l in links {
        out.push_str(&format!(
            "\n[[finding_links]]\nref = \"{}\"\nstore = \"{REVIEW_STORE}\"\nfixes = {}\nsettles = {}\n",
            l.r#ref,
            quoted(l.fixes),
            quoted(l.settles),
        ));
    }
    out
}

struct Repo {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Repo {
    fn new(findings: &[String], tasks: &[Task<'_>], links: &[Link<'_>]) -> Self {
        let (dir, root) = sandbox();
        let path = stage_tasks_flow(
            &root,
            TASKS_SLUG,
            &format!("docs/plans/{TASKS_SLUG}.md"),
            Some(&tasks_store(tasks, links)),
        );
        fs::write(path.parent().unwrap().join("plan.toml"), "").unwrap();
        let store = root.join(REVIEW_STORE);
        fs::create_dir_all(store.parent().unwrap()).unwrap();
        fs::write(store, format!("{HEADER}{}", findings.concat())).unwrap();
        Self { _dir: dir, root }
    }

    fn reconcile(&self) -> JsonValue {
        let assert = cli(&self.root)
            .current_dir(&self.root)
            .args(["--error-format", "json", "review", "reconcile"])
            .args(["--scope", SCOPE, "--flow", TASKS_SLUG])
            .assert()
            .success();
        stdout_json(&assert)
    }

    fn review_bytes(&self) -> Vec<u8> {
        fs::read(self.root.join(REVIEW_STORE)).unwrap()
    }

    fn finding(&self, id: &str) -> toml::map::Map<String, Value> {
        let doc: Value =
            toml::from_str(&fs::read_to_string(self.root.join(REVIEW_STORE)).unwrap()).unwrap();
        doc["findings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
            .unwrap_or_else(|| panic!("no finding {id}"))
            .as_table()
            .unwrap()
            .clone()
    }
}

fn ids(out: &JsonValue, key: &str) -> Vec<String> {
    out[key]
        .as_array()
        .unwrap_or_else(|| panic!("no {key} in {out}"))
        .iter()
        .map(|e| e["finding"].as_str().unwrap().to_string())
        .collect()
}

fn text<'a>(f: &'a toml::map::Map<String, Value>, key: &str) -> Option<&'a str> {
    f.get(key).and_then(Value::as_str)
}

#[test]
fn a_committed_fix_resolves_its_finding_and_keeps_the_link() {
    let repo = Repo::new(
        &[scheduled("F1", TASKS_SLUG, "fix-alpha")],
        &[task("fix-alpha", "done", "4f0c2aa")],
        &[fixes("fix-alpha", &["F1"])],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "resolved"), ["F1"], "{out}");
    assert_eq!(out["resolved"][0]["commit"], "4f0c2aa");

    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("resolved"));
    assert_eq!(text(&f, "apply_flow"), Some(TASKS_SLUG));
    assert_eq!(text(&f, "apply_task"), Some("fix-alpha"));
    let resolution = text(&f, "resolution").unwrap();
    for part in [TASKS_SLUG, "fix-alpha", "4f0c2aa"] {
        assert!(resolution.contains(part), "{resolution}");
    }
    assert!(f.contains_key("resolved"), "{f:?}");
}

#[test]
fn unfinished_tasks_leave_their_findings_scheduled() {
    let repo = Repo::new(
        &[
            scheduled("F1", TASKS_SLUG, "done-uncommitted"),
            scheduled("F2", TASKS_SLUG, "failed-task"),
            scheduled("F3", TASKS_SLUG, "parked-task"),
            scheduled("F4", TASKS_SLUG, "pending-task"),
        ],
        &[
            task("done-uncommitted", "done", ""),
            task("failed-task", "failed", ""),
            task("parked-task", "parked", ""),
            task("pending-task", "pending", ""),
        ],
        &[
            fixes("done-uncommitted", &["F1"]),
            fixes("failed-task", &["F2"]),
            fixes("parked-task", &["F3"]),
            fixes("pending-task", &["F4"]),
        ],
    );
    let before = repo.review_bytes();
    let out = repo.reconcile();
    assert_eq!(ids(&out, "in_flight"), ["F1", "F2", "F3", "F4"], "{out}");
    assert!(ids(&out, "resolved").is_empty(), "{out}");
    assert!(ids(&out, "reopened").is_empty(), "{out}");
    assert_eq!(
        repo.review_bytes(),
        before,
        "nothing moved, nothing written"
    );
}

#[test]
fn a_removed_or_deferred_task_reopens_its_finding_and_clears_the_link() {
    let repo = Repo::new(
        &[
            scheduled("F1", TASKS_SLUG, "gone-task"),
            scheduled("F2", TASKS_SLUG, "deferred-task"),
        ],
        &[task("deferred-task", "deferred", "")],
        &[fixes("deferred-task", &["F2"])],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "reopened"), ["F1", "F2"], "{out}");
    for id in ["F1", "F2"] {
        let f = repo.finding(id);
        assert_eq!(text(&f, "status"), Some("open"), "{f:?}");
        assert!(
            text(&f, "reopen_reason").unwrap().starts_with("apply: "),
            "{f:?}"
        );
        assert_eq!(text(&f, "apply_flow"), None, "{f:?}");
        assert_eq!(text(&f, "apply_task"), None, "{f:?}");
    }
}

#[test]
fn a_committed_settle_task_reopens_its_finding() {
    let repo = Repo::new(
        &[scheduled("F1", TASKS_SLUG, "settle-alpha")],
        &[task("settle-alpha", "done", "4f0c2aa")],
        &[Link {
            r#ref: "settle-alpha",
            fixes: &[],
            settles: &["F1"],
        }],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "reopened"), ["F1"], "{out}");
    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("open"));
    assert!(text(&f, "reopen_reason").unwrap().starts_with("apply: "));
    assert_eq!(text(&f, "apply_task"), None);
}

#[test]
fn a_retitled_task_is_followed_and_apply_task_rewritten() {
    let repo = Repo::new(
        &[scheduled("F1", TASKS_SLUG, "fix-alpha")],
        &[task("fix-alpha-bound", "in-progress", "")],
        &[fixes("fix-alpha-bound", &["F1"])],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "in_flight"), ["F1"], "{out}");
    assert_eq!(ids(&out, "retitled"), ["F1"], "{out}");
    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("scheduled"));
    assert_eq!(text(&f, "apply_task"), Some("fix-alpha-bound"));
    assert_eq!(text(&f, "apply_flow"), Some(TASKS_SLUG));
}

#[test]
fn findings_scheduled_into_another_flow_are_left_alone() {
    let repo = Repo::new(
        &[scheduled("F1", "other-flow", "gone-task")],
        &[task("fix-alpha", "done", "4f0c2aa")],
        &[],
    );
    let before = repo.review_bytes();
    let out = repo.reconcile();
    for key in ["resolved", "reopened", "in_flight"] {
        assert!(ids(&out, key).is_empty(), "{out}");
    }
    assert_eq!(repo.review_bytes(), before);
}

#[test]
fn a_rescheduled_finding_whose_new_task_is_gone_reopens() {
    let finding = format!(
        "{}apply_task_id = 2\n",
        scheduled("F1", TASKS_SLUG, "fix-alpha-again")
    );
    let repo = Repo::new(
        &[finding],
        &[task("fix-alpha", "done", "abc1234")],
        &[fixes("fix-alpha", &["F1"])],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "reopened"), ["F1"], "{out}");
    assert!(ids(&out, "resolved").is_empty(), "{out}");
    let f = repo.finding("F1");
    assert_eq!(text(&f, "status"), Some("open"));
    assert!(f.get("apply_task_id").is_none(), "{f:?}");
}

#[test]
fn a_retitled_task_is_followed_by_its_recorded_id() {
    let finding = format!(
        "{}apply_task_id = 2\n",
        scheduled("F1", TASKS_SLUG, "fix-alpha-again")
    );
    let repo = Repo::new(
        &[finding],
        &[
            task("fix-alpha", "done", "abc1234"),
            task("fix-alpha-bound", "done", "def5678"),
        ],
        &[
            fixes("fix-alpha", &["F1"]),
            fixes("fix-alpha-bound", &["F1"]),
        ],
    );
    let out = repo.reconcile();
    assert_eq!(ids(&out, "retitled"), ["F1"], "{out}");
    assert_eq!(ids(&out, "resolved"), ["F1"], "{out}");
    let f = repo.finding("F1");
    assert_eq!(text(&f, "apply_task"), Some("fix-alpha-bound"));
    assert_eq!(
        text(&f, "resolution").map(|r| r.ends_with("def5678")),
        Some(true)
    );
}
